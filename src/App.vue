/**
 * Volo 主应用
 */

<script setup lang="ts">
import { computed, onMounted, ref, nextTick, watch } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import SearchInput from './components/SearchInput.vue';
import ResultList from './components/ResultList.vue';
import PluginView from './components/PluginView.vue';
import SubInput from './components/SubInput.vue';
import SettingsView from './components/SettingsView.vue';
import AgentView from './components/AgentView.vue';
import PluginManager from './components/PluginManager.vue';
import ApprovalDialog from './components/ApprovalDialog.vue';
import { useSearchStore } from './stores/search';
import { runCommand } from './bridge/commandRunner';
import './api/rubick';

const mainWindow = getCurrentWindow();

const searchStore = useSearchStore();

// 插件状态
const pluginMode = ref(false);
const settingsMode = ref(false);
const pluginManagerMode = ref(false);
const agentMode = ref(false);
const agentQuery = ref('');
const currentPlugin = ref<{ pluginId: string; featureId: string } | null>(null);
const pluginViewRef = ref<InstanceType<typeof PluginView> | null>(null);

// 子输入框状态
const subInputVisible = ref(false);
const subInputPlaceholder = ref('');
const subInputValue = ref('');

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
  if (agentMode.value) {
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
      // 后台执行无界面命令（错误由 commandRunner 通知），清空搜索并隐藏窗口
      void runCommand(result.plugin.id, result.command.id, searchStore.query);
      searchStore.clearSearch();
      updateWindowSize();
      await invoke('hide_main_window');
    } else if (result.type === 'file') {
      // 打开文件或文件夹
      await invoke('shell_open_path', { path: result.path });
      // 隐藏窗口
      await invoke('hide_main_window');
      // 清空搜索
      searchStore.clearSearch();
      updateWindowSize();
    } else if (result.type === 'ai') {
      // 未配置 LLM 时引导去设置页，否则进入 Agent 模式
      if (!searchStore.llmConfigured) {
        enterSettings();
      } else {
        enterAgent(result.query);
      }
    }
  }

// 进入 Agent 模式
function enterAgent(query: string) {
  agentQuery.value = query;
  agentMode.value = true;
  updateWindowSize();
}

// 退出 Agent 模式（AgentView 卸载时自动 agent_cancel）
function exitAgent() {
  agentMode.value = false;
  agentQuery.value = '';
  updateWindowSize();
}

// 进入插件模式
function enterPlugin(pluginId: string, featureId: string) {
  currentPlugin.value = { pluginId, featureId };
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

// 失焦隐藏
onMounted(async () => {
  const unlisten = await mainWindow.onFocusChanged(({ payload }: { payload: boolean }) => {
    if (!payload && !settingsMode.value && !pluginManagerMode.value && !pluginMode.value && !agentMode.value) {
      // 失焦时隐藏（排除设置模式、插件管理模式、插件模式和 Agent 模式）
      invoke('hide_main_window');
    }
  });

  // 清理
  return () => {
    unlisten();
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
      <AgentView :query="agentQuery" @exit="exitAgent" />
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
