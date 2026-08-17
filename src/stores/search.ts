/**
 * 搜索状态管理
 */

import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { LlmConfig, ListCommandItem, SearchResult } from '../api/rubick';

export const useSearchStore = defineStore('search', () => {
  // 状态
  const query = ref('');
  const results = ref<SearchResult[]>([]);
  const selectedIndex = ref(0);
  const loading = ref(false);

  // LLM 配置缓存（初始化时拉一次，配置完整且有 API key 才显示"问 AI"入口）
  const llmConfigured = ref(false);

  async function refreshLlmStatus() {
    try {
      const config = await invoke<LlmConfig>('llm_get_config');
      const hasKey = await invoke<boolean>('llm_has_api_key');
      llmConfigured.value = !!config.baseUrl && !!config.model && hasKey;
    } catch {
      llmConfigured.value = false;
    }
  }

  refreshLlmStatus();

  // 计算属性
  const hasResults = computed(() => results.value.length > 0);
  const selectedResult = computed(() => 
    results.value[selectedIndex.value] ?? null
  );

  // 搜索防抖定时器
  let searchTimer: ReturnType<typeof setTimeout> | null = null;

  // 搜索方法
  async function doSearch(q: string) {
    if (!q.trim()) {
      results.value = [];
      selectedIndex.value = 0;
      return;
    }

    loading.value = true;
    try {
      const r = await invoke<SearchResult[]>('search', { query: q });
      // 始终追加"问 AI"伪结果（不进 Rust 搜索）；未配置时选中会引导去设置页
      results.value = [...r, { type: 'ai', query: q }];
      selectedIndex.value = 0;
    } catch (e) {
      console.error('Search error:', e);
      results.value = [];
    } finally {
      loading.value = false;
    }
  }

  // 防抖搜索
  function search(q: string) {
    query.value = q;
    
    if (searchTimer) {
      clearTimeout(searchTimer);
    }
    
    searchTimer = setTimeout(() => {
      doSearch(q);
    }, 150);
  }

  // 清空搜索
  function clearSearch() {
    query.value = '';
    results.value = [];
    selectedIndex.value = 0;
    if (searchTimer) {
      clearTimeout(searchTimer);
      searchTimer = null;
    }
  }

  // list 模式命令推送的结果列表（整体替换，重置选中）
  function setListItems(items: ListCommandItem[]) {
    results.value = items.map((item) => ({ type: 'command-item' as const, item }));
    selectedIndex.value = 0;
  }

  // 导航
  function selectNext() {
    if (selectedIndex.value < results.value.length - 1) {
      selectedIndex.value++;
    }
  }

  function selectPrev() {
    if (selectedIndex.value > 0) {
      selectedIndex.value--;
    }
  }

  // 选择结果
  function selectResult(index: number) {
    if (index >= 0 && index < results.value.length) {
      selectedIndex.value = index;
    }
  }

  return {
    query,
    results,
    selectedIndex,
    loading,
    hasResults,
    selectedResult,
    llmConfigured,
    refreshLlmStatus,
    search,
    clearSearch,
    setListItems,
    selectNext,
    selectPrev,
    selectResult,
  };
});
