/**
 * 搜索状态管理
 */

import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { SearchResult } from '../api/rubick';

export const useSearchStore = defineStore('search', () => {
  // 状态
  const query = ref('');
  const results = ref<SearchResult[]>([]);
  const selectedIndex = ref(0);
  const loading = ref(false);

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
      results.value = r;
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
    search,
    clearSearch,
    selectNext,
    selectPrev,
    selectResult,
  };
});
