/**
 * Volo 主应用
 */

<script setup lang="ts">
import { computed, onMounted, ref, nextTick, watch } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { check } from '@tauri-apps/plugin-updater';
import SearchInput from './components/SearchInput.vue';
import ResultList from './components/ResultList.vue';
import PluginView from './components/PluginView.vue';
import SubInput from './components/SubInput.vue';
import SettingsView from './components/SettingsView.vue';
import AgentView from './components/AgentView.vue';
import PluginManager from './components/PluginManager.vue';
import ApprovalDialog from './components/ApprovalDialog.vue';
import { useSearchStore } from './stores/search';
import { runCommand, runListCommand, type ListCommandHandle, type ListCommandItem } from './bridge/commandRunner';
import { initToolRunner } from './bridge/toolRunner';
import { nativeDialogOpen } from './composables/nativeDialog';
import { hideOnBlur, loadAppConfig } from './composables/appConfig';
import './api/rubick';

const mainWindow = getCurrentWindow();

const searchStore = useSearchStore();

// 插件状态
const pluginMode = ref(false);
const settingsMode = ref(false);
const pluginManagerMode = ref(false);
const agentMode = ref(false);
const agentQuery = ref('');
// AgentView 打开方式：chat（带 query 直接发问）/ history（直达会话历史列表）
const agentInitialMode = ref<'chat' | 'history'>('chat');
// @技能名 显式触发：进入 Agent 时携带的技能名（无则 undefined）
const agentSkill = ref<string | undefined>(undefined);
const currentPlugin = ref<{ pluginId: string; featureId: string } | null>(null);
const pluginViewRef = ref<InstanceType<typeof PluginView> | null>(null);
// 插件热重载：plugins-changed 事件到达时 +1，强制 PluginView 重建（重新拉取 HTML 并重发 onPluginEnter）
const pluginReloadKey = ref(0);

// 子输入框状态
const subInputVisible = ref(false);
const subInputPlaceholder = ref('');
const subInputValue = ref('');

// list 命令模式状态
const listCommandMode = ref(false);
let listHandle: ListCommandHandle | null = null;
let listQueryTimer: ReturnType<typeof setTimeout> | null = null;
// 二级动作面板：非空表示正展示该列表项的动作列表
const actionPanelItem = ref<ListCommandItem | null>(null);

// 计算窗口高度
const windowHeight = computed(() => {
  let height = 60; // 基础高度

  if (settingsMode.value) {
    return 450; // 设置模式固定高度
  }

  if (agentMode.value) {
    return 420; // Agent 模式固定高度
  }

  if (pluginManagerMode.value) {
    return 400; // 插件管理模式固定高度
  }

  if (pluginMode.value) {
    height = 400; // 插件模式固定高度
    if (subInputVisible.value) {
      height += 40; // 子输入框高度
    }
    return height;
  }

  if (searchStore.hasResults) {
    const count = Math.min(searchStore.results.length, 8);
    height += count * 50;
  }
  return height;
});

// 处理输入
function handleInput(value: string) {
  // list 命令模式：输入作为过滤词，防抖 150ms 后重触发命令的 onRun(query)
  if (listCommandMode.value) {
    // 面板打开时继续输入：先收回面板（随后的 setQuery 会恢复并过滤列表）
    if (actionPanelItem.value) {
      actionPanelItem.value = null;
    }
    if (listQueryTimer) {
      clearTimeout(listQueryTimer);
    }
    listQueryTimer = setTimeout(() => {
      listHandle?.setQuery(value);
    }, 150);
    return;
  }

  // 检查是否是设置命令
  if (value.toLowerCase() === 'settings' || value === '设置') {
    enterSettings();
    return;
  }

  // 检查是否是插件管理命令
  if (value.toLowerCase() === 'plugins' || value === '插件') {
    enterPluginManager();
    return;
  }

  searchStore.search(value);
  updateWindowSize();
}

// 处理清空
function handleClear() {
  if (listCommandMode.value) {
    // 动作面板打开时 Esc 只收起面板，不退出 list 模式
    if (actionPanelItem.value) {
      closeActionPanel();
      return;
    }
    // 退出 list 命令模式（销毁 iframe，停留在启动器）
    exitListCommand();
  } else if (agentMode.value) {
    // 退出 Agent 模式（AgentView 卸载时自动 agent_cancel）
    exitAgent();
  } else if (settingsMode.value) {
    // 退出设置模式
    exitSettings();
  } else if (pluginManagerMode.value) {
    // 退出插件管理模式
    exitPluginManager();
  } else if (pluginMode.value) {
    // 退出插件模式
    exitPlugin();
  } else {
    searchStore.clearSearch();
    updateWindowSize();
  }
}

// 处理确认
  async function handleConfirm() {
    const result = searchStore.selectedResult;
    if (!result) return;

    if (listCommandMode.value) {
      // list 命令模式：回车选中某项，触发命令的 onSelect(id)；
      // 命令执行完动作后调 command.done()，由 onDone 回调退出模式并隐藏窗口
      if (result.type === 'command-item') {
        // 动作面板打开中：选中的是动作，触发 onAction(itemId, actionId)
        if (actionPanelItem.value) {
          listHandle?.action(actionPanelItem.value.id, result.item.id);
        } else {
          listHandle?.select(result.item.id);
        }
      }
      return;
    }

    if (result.type === 'app') {
      // 记录使用历史
      invoke('record_app_usage', { appPath: result.path }).catch(() => {});

      // 打开应用
      await invoke('shell_open_path', { path: result.path });
      // 隐藏窗口
      await invoke('hide_main_window');
      // 清空搜索
      searchStore.clearSearch();
      updateWindowSize();
    } else if (result.type === 'plugin') {
      // 进入插件模式
      enterPlugin(result.plugin.id, result.feature.id);
    } else if (result.type === 'command') {
      if (result.command.mode === 'list') {
        // list 模式命令：不隐藏窗口，进入 list 命令模式
        void enterListCommand(result.plugin.id, result.command.id);
      } else {
        // 后台执行无界面命令（错误由 commandRunner 通知），清空搜索并隐藏窗口
        void runCommand(result.plugin.id, result.command.id, searchStore.query);
        searchStore.clearSearch();
        updateWindowSize();
        await invoke('hide_main_window');
      }
    } else if (result.type === 'file') {
      // 打开文件或文件夹
      await invoke('shell_open_path', { path: result.path });
      // 隐藏窗口
      await invoke('hide_main_window');
      // 清空搜索
      searchStore.clearSearch();
      updateWindowSize();
    } else if (result.type === 'skill-entry') {
      // @技能名 候选：补全输入为 "@name "，继续输入问题后回车发问
      searchStore.search(`@${result.skill.name} `);
    } else if (result.type === 'ai') {
      // 未配置 LLM 时引导去设置页，否则进入 Agent 模式（skill 为 @技能名 显式触发）
      if (!searchStore.llmConfigured) {
        enterSettings();
      } else {
        enterAgent(result.query, result.skill);
      }
    } else if (result.type === 'ai-history') {
      // 空输入入口：直达 AI 会话历史（可回放、继续对话）
      enterAgentHistory();
    }
  }

// 进入 Agent 模式（启动器入口永远是新会话；清空失败也继续；skill 为 @技能名 显式触发）
async function enterAgent(query: string, skill?: string) {
  try {
    await invoke('agent_new_session');
  } catch (e) {
    console.warn('agent_new_session 失败，继续进入 Agent 模式', e);
  }
  agentQuery.value = query;
  agentSkill.value = skill;
  agentInitialMode.value = 'chat';
  agentMode.value = true;
  updateWindowSize();
}

// 直达 AI 会话历史（空输入入口；不动当前会话状态）
function enterAgentHistory() {
  agentQuery.value = '';
  agentSkill.value = undefined;
  agentInitialMode.value = 'history';
  agentMode.value = true;
  updateWindowSize();
}

// 退出 Agent 模式（AgentView 卸载时自动 agent_cancel）
function exitAgent() {
  agentMode.value = false;
  agentQuery.value = '';
  agentSkill.value = undefined;
  agentInitialMode.value = 'chat';
  updateWindowSize();
}

// 打开选中项的二级动作面板（仅 list 模式、该项声明了 actions 时生效）
function openActionPanel() {
  if (!listCommandMode.value || actionPanelItem.value) return;
  const result = searchStore.selectedResult;
  if (!result || result.type !== 'command-item') return;
  const actions = result.item.actions;
  if (!actions || actions.length === 0) return;
  actionPanelItem.value = result.item;
  // 动作复用 command-item 渲染（actionId 装在 item.id 里，确认时取回）
  searchStore.setListItems(actions.map((a) => ({ ...a })));
  updateWindowSize();
}

// 收起动作面板：重触发 onRun 让插件恢复并过滤原列表
function closeActionPanel() {
  if (!actionPanelItem.value) return;
  actionPanelItem.value = null;
  listHandle?.setQuery(searchStore.query);
}

// 进入 list 命令模式：清空搜索作为过滤输入，启动隐藏 iframe 并触发 onRun('')
async function enterListCommand(pluginId: string, commandId: string) {
  searchStore.clearSearch();
  listCommandMode.value = true;
  updateWindowSize();

  const handle = await runListCommand(pluginId, commandId, {
    onList: (items) => {
      // 动作面板打开期间忽略列表推送，避免动作项被覆盖
      if (!actionPanelItem.value) {
        searchStore.setListItems(items);
      }
    },
    onDone: () => {
      // 命令调 done()：销毁 iframe、退出模式并隐藏窗口（错误已由 commandRunner 通知）
      exitListCommand();
      void invoke('hide_main_window');
    },
    onError: () => {
      // 运行单元启动失败：退出模式，停留在启动器
      exitListCommand();
    },
  });

  if (!handle) {
    return; // onError 已退出模式
  }
  // await 期间可能已退出（Esc / 失焦），避免泄漏运行单元
  if (!listCommandMode.value) {
    handle.destroy();
    return;
  }
  listHandle = handle;
}

// 退出 list 命令模式（destroy 幂等；onDone 路径下 iframe 已由 commandRunner 销毁）
function exitListCommand() {
  listCommandMode.value = false;
  actionPanelItem.value = null;
  if (listQueryTimer) {
    clearTimeout(listQueryTimer);
    listQueryTimer = null;
  }
  listHandle?.destroy();
  listHandle = null;
  searchStore.clearSearch();
  updateWindowSize();
}

// 进入插件模式
function enterPlugin(pluginId: string, featureId: string) {  currentPlugin.value = { pluginId, featureId };
  pluginMode.value = true;
  subInputVisible.value = false;
  updateWindowSize();
}

// 退出插件模式
function exitPlugin() {
  pluginMode.value = false;
  currentPlugin.value = null;
  subInputVisible.value = false;
  searchStore.clearSearch();
  updateWindowSize();
}

// 进入设置模式
function enterSettings() {
  settingsMode.value = true;
  searchStore.clearSearch();
  updateWindowSize();
}

// 退出设置模式
function exitSettings() {
  settingsMode.value = false;
  updateWindowSize();
}

// 进入插件管理模式
function enterPluginManager() {
  pluginManagerMode.value = true;
  searchStore.clearSearch();
  updateWindowSize();
}

// 退出插件管理模式
function exitPluginManager() {
  pluginManagerMode.value = false;
  updateWindowSize();
}

// 从插件管理器打开插件
function onOpenPluginFromManager(pluginId: string, featureId: string) {
  pluginManagerMode.value = false;
  enterPlugin(pluginId, featureId);
}

// 插件退出回调
function onPluginExit() {
  exitPlugin();
}

// 子输入框变化
function onSubInputChange(value: string) {
  // 通知插件
  pluginViewRef.value?.handleSubInputChange(value);
}

// 子输入框显示
function onSubInputShow(data: { placeholder?: string }) {
  subInputVisible.value = true;
  subInputPlaceholder.value = data.placeholder || '';
  updateWindowSize();
}

// 子输入框隐藏
function onSubInputHide() {
  subInputVisible.value = false;
  updateWindowSize();
}

// 子输入框设置值
function onSubInputSetValue(text: string) {
  subInputValue.value = text;
}

// 更新窗口大小
async function updateWindowSize() {
  await nextTick();
  const height = windowHeight.value;
  await invoke('set_window_height', { height });
}

// 搜索结果到达（防抖后异步）时重新计算窗口高度，
// 否则单次输入（如粘贴一整段）会用空结果算出 60px，列表被裁掉
watch(() => searchStore.results, () => {
  if (!pluginMode.value && !settingsMode.value && !pluginManagerMode.value && !agentMode.value) {
    updateWindowSize();
  }
});

// 启动时静默检查更新，有新版本发系统通知；失败静默
async function silentCheckUpdate() {
  try {
    const update = await check();
    if (update) {
      await invoke('notification_show', {
        options: {
          title: 'Volo 更新',
          body: `发现新版本 ${update.version}，可到设置页更新`,
        },
      });
    }
  } catch {
    // 静默失败（无网络、无 release 等）
  }
}

// 失焦隐藏
onMounted(async () => {
  void silentCheckUpdate();
  // 插件工具执行器：监听 Rust 侧 Agent 的 plugin-tool-call，常驻整个 App 生命周期
  void initToolRunner();
  // 加载失焦隐藏等共享配置
  void loadAppConfig();
  // 插件热重载：打开中的插件视图强制重建；否则刷新搜索结果（manifest/关键字可能变了）
  const unlistenPluginsChanged = await listen('plugins-changed', () => {
    if (pluginMode.value && currentPlugin.value) {
      pluginReloadKey.value++;
    } else if (!settingsMode.value && !pluginManagerMode.value && !agentMode.value) {
      searchStore.search(searchStore.query);
    }
  });
  const unlisten = await mainWindow.onFocusChanged(({ payload }: { payload: boolean }) => {
    if (!payload && hideOnBlur.value && !nativeDialogOpen.value && !settingsMode.value && !pluginManagerMode.value && !pluginMode.value && !agentMode.value) {
      // 失焦时隐藏（用户可在设置中关闭；排除原生对话框打开中、设置模式、插件管理模式、插件模式和 Agent 模式）
      // list 命令模式：失焦隐藏同时销毁 iframe、退出模式
      if (listCommandMode.value) {
        exitListCommand();
      }
      invoke('hide_main_window');
    }
  });

  // 清理
  return () => {
    unlisten();
    unlistenPluginsChanged();
  };
});
</script>

<template>
  <div class="app">
    <!-- 设置模式 -->
    <template v-if="settingsMode">
      <SettingsView @back="exitSettings" />
    </template>

    <!-- Agent 模式 -->
    <template v-else-if="agentMode">
      <AgentView :query="agentQuery" :skill="agentSkill" :initial-mode="agentInitialMode" @exit="exitAgent" />
    </template>

    <!-- 插件管理模式 -->
    <template v-else-if="pluginManagerMode">
      <PluginManager @back="exitPluginManager" @open="onOpenPluginFromManager" />
    </template>

    <!-- 搜索模式 -->
    <template v-else-if="!pluginMode">
      <SearchInput
        v-model="searchStore.query"
        placeholder="搜索应用、插件..."
        @update:model-value="handleInput"
        @clear="handleClear"
        @select-next="searchStore.selectNext"
        @select-prev="searchStore.selectPrev"
        @confirm="handleConfirm"
        @open-actions="openActionPanel"
        @close-actions="closeActionPanel"
      />

      <ResultList
        v-if="searchStore.hasResults"
        :results="searchStore.results"
        :selected-index="searchStore.selectedIndex"
        @select="searchStore.selectResult"
        @confirm="handleConfirm"
      />
    </template>

    <!-- 插件模式 -->
    <template v-else-if="currentPlugin">
      <!-- 子输入框 -->
      <SubInput
        :visible="subInputVisible"
        v-model="subInputValue"
        :placeholder="subInputPlaceholder"
        @change="onSubInputChange"
      />

      <PluginView
        ref="pluginViewRef"
        :key="pluginReloadKey"
        :plugin-id="currentPlugin.pluginId"
        :feature-id="currentPlugin.featureId"
        :query="searchStore.query"
        @exit="onPluginExit"
        @sub-input-show="onSubInputShow"
        @sub-input-hide="onSubInputHide"
        @sub-input-set-value="onSubInputSetValue"
      />
    </template>

    <!-- 权限审批弹窗（全局，不受当前视图影响） -->
    <ApprovalDialog />
  </div>
</template>

<style>
.app {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 60px;
  background: var(--bg-primary);
  border-radius: 10px;
  overflow: hidden;
  box-shadow:
    0 4px 6px -1px rgba(0, 0, 0, 0.1),
    0 2px 4px -1px rgba(0, 0, 0, 0.06),
    0 20px 25px -5px rgba(0, 0, 0, 0.1),
    0 10px 10px -5px rgba(0, 0, 0, 0.04);
}
</style>
