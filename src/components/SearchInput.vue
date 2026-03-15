/**
 * 搜索输入组件
 */

<template>
  <div class="search-input-wrapper">
    <div class="search-icon">
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="11" cy="11" r="8"/>
        <path d="m21 21-4.35-4.35"/>
      </svg>
    </div>
    <input
      ref="inputRef"
      :value="modelValue"
      type="text"
      class="search-input"
      :placeholder="placeholder"
      @input="onInput"
      @keydown="onKeydown"
    />
    <div v-if="modelValue" class="clear-btn" @click="onClear">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M18 6 6 18M6 6l12 12"/>
      </svg>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';

const mainWindow = getCurrentWindow();

defineProps<{
  modelValue: string;
  placeholder?: string;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
  (e: 'clear'): void;
  (e: 'selectNext'): void;
  (e: 'selectPrev'): void;
  (e: 'confirm'): void;
}>();

const inputRef = ref<HTMLInputElement>();

function onInput(e: Event) {
  const target = e.target as HTMLInputElement;
  emit('update:modelValue', target.value);
}

function onKeydown(e: KeyboardEvent) {
  switch (e.key) {
    case 'ArrowDown':
      e.preventDefault();
      emit('selectNext');
      break;
    case 'ArrowUp':
      e.preventDefault();
      emit('selectPrev');
      break;
    case 'Enter':
      e.preventDefault();
      emit('confirm');
      break;
    case 'Escape':
      e.preventDefault();
      emit('clear');
      break;
  }
}

function onClear() {
  emit('update:modelValue', '');
  emit('clear');
  inputRef.value?.focus();
}

// 窗口显示时聚焦输入框
onMounted(async () => {
  inputRef.value?.focus();
  
  const unlisten = await mainWindow.onFocusChanged(({ payload }: { payload: boolean }) => {
    if (payload) {
      inputRef.value?.focus();
    }
  });
  
  // 清理监听
  return () => {
    unlisten();
  };
});

// 暴露 focus 方法
defineExpose({
  focus: () => inputRef.value?.focus(),
});
</script>

<style scoped>
.search-input-wrapper {
  display: flex;
  align-items: center;
  height: 60px;
  padding: 0 16px;
  background: var(--bg-primary);
  border-bottom: 1px solid var(--border-color);
}

.search-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  margin-right: 12px;
  color: var(--text-tertiary);
}

.search-input {
  flex: 1;
  height: 100%;
  border: none;
  outline: none;
  font-size: 18px;
  font-weight: 500;
  background: transparent;
  color: var(--text-primary);
}

.search-input::placeholder {
  color: var(--text-tertiary);
}

.clear-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  cursor: pointer;
  color: var(--text-tertiary);
  transition: color 0.2s;
}

.clear-btn:hover {
  color: var(--text-primary);
}
</style>
