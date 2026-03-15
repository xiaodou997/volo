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
import { useSearchStore } from './stores/search';
import './api/rubick';

const mainWindow = getCurrentWindow();

const searchStore = useSearchStore();

// 插件状态
const pluginMode = ref(false);
const currentPlugin = ref<{ pluginId: string; featureId: string } | null>(null);

// 计算窗口高度
const windowHeight = computed(() => {
  if (pluginMode.value) {
    return 400; // 插件模式固定高度
  }

  const baseHeight = 60;
  const resultHeight = 50;
  const maxResults = 8;

  if (searchStore.hasResults) {
    const count = Math.min(searchStore.results.length, maxResults);
    return baseHeight + count * resultHeight;
  }
  return baseHeight;
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
  updateWindowSize();
}

// 退出插件模式
function exitPlugin() {
  pluginMode.value = false;
  currentPlugin.value = null;
  searchStore.clearSearch();
  updateWindowSize();
}

// 插件退出回调
function onPluginExit() {
  exitPlugin();
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
      <PluginView
        :plugin-id="currentPlugin.pluginId"
        :feature-id="currentPlugin.featureId"
        :query="searchStore.query"
        @exit="onPluginExit"
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
