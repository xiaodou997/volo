/**
 * 搜索结果列表组件
 */

<template>
  <div class="result-list">
    <div
      v-for="(result, index) in results"
      :key="getKey(result, index)"
      class="result-item"
      :class="{ selected: index === selectedIndex }"
      @click="onSelect(index)"
      @mouseenter="onHover(index)"
    >
      <!-- 应用结果 -->
      <template v-if="result.type === 'app'">
        <div class="result-icon">
          <img
            v-if="result.icon"
            :src="result.icon"
            :alt="result.name"
            class="icon-image"
          />
          <svg v-else width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <rect x="3" y="3" width="18" height="18" rx="2"/>
            <path d="M9 9h6M9 13h6M9 17h4"/>
          </svg>
        </div>
        <div class="result-content">
          <div class="result-title">{{ result.name }}</div>
          <div class="result-subtitle">应用</div>
        </div>
      </template>

      <!-- 插件结果 -->
      <template v-else-if="result.type === 'plugin'">
        <div class="result-icon">
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/>
          </svg>
        </div>
        <div class="result-content">
          <div class="result-title">{{ result.feature.name }}</div>
          <div class="result-subtitle">{{ result.plugin.name }}</div>
        </div>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { SearchResult } from '../api/rubick';

const props = defineProps<{
  results: SearchResult[];
  selectedIndex: number;
}>();

const emit = defineEmits<{
  (e: 'select', index: number): void;
  (e: 'confirm', index: number): void;
}>();

function getKey(result: SearchResult, index: number): string {
  if (result.type === 'app') {
    return `app-${result.path}`;
  }
  return `plugin-${result.plugin.id}-${result.feature.id}-${index}`;
}

function onSelect(index: number) {
  emit('select', index);
  emit('confirm', index);
}

function onHover(index: number) {
  emit('select', index);
}

// 预加载可见结果的图标
async function preloadIcons(results: SearchResult[]) {
  for (const result of results.slice(0, 5)) {
    if (result.type === 'app' && !result.icon) {
      try {
        const icon = await invoke<string>('get_app_icon', { path: result.path });
        if (icon) {
          result.icon = icon;
        }
      } catch {
        // 忽略错误
      }
    }
  }
}

// 监听结果变化，预加载图标
watch(
  () => props.results,
  (newResults) => {
    if (newResults.length > 0) {
      preloadIcons(newResults);
    }
  },
  { immediate: true }
);
</script>

<style scoped>
.result-list {
  overflow-y: auto;
  max-height: 400px;
}

.result-item {
  display: flex;
  align-items: center;
  height: 50px;
  padding: 0 16px;
  cursor: pointer;
  transition: background-color 0.15s;
}

.result-item:hover {
  background: var(--bg-hover);
}

.result-item.selected {
  background: var(--bg-active);
}

.result-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  margin-right: 12px;
  color: var(--text-secondary);
  background: var(--bg-secondary);
  border-radius: 6px;
  overflow: hidden;
}

.icon-image {
  width: 24px;
  height: 24px;
  object-fit: contain;
}

.result-content {
  flex: 1;
  min-width: 0;
}

.result-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.result-subtitle {
  font-size: 12px;
  color: var(--text-tertiary);
  margin-top: 2px;
}
</style>
