/**
 * Volo 主应用
 */

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import SearchInput from './components/SearchInput.vue';
import ResultList from './components/ResultList.vue';
import PluginView from './components/PluginView.vue';
import SubInput from './components/SubInput.vue';
import { useSearchStore } from './stores/search';
import './api/rubick';

const mainWindow = getCurrentWindow();

const searchStore = useSearchStore();

// 插件状态
const pluginMode = ref(false);
const currentPlugin = ref<{ pluginId: string; featureId: string } | null>(null);
const pluginViewRef = ref<InstanceType<typeof PluginView> | null>(null);

// 子输入框状态
const subInputVisible = ref(false);
const subInputPlaceholder = ref('');
const subInputValue = ref('');

// 计算窗口高度
const windowHeight = computed(() => {
  let height = 60; // 基础高度

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
  searchStore.search(value);
  updateWindowSize();
}

// 处理清空
function handleClear() {
  if (pluginMode.value) {
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
  }
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
  await invoke('set_window_height', { height: windowHeight.value });
}

// 失焦隐藏
onMounted(async () => {
  const unlisten = await mainWindow.onFocusChanged(({ payload }: { payload: boolean }) => {
    if (!payload && searchStore.query === '' && !pluginMode.value) {
      // 失焦且无搜索内容时隐藏
      // invoke('hide_main_window');
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
    <!-- 搜索模式 -->
    <template v-if="!pluginMode">
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
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
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
