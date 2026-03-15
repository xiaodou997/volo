/**
 * 子输入框组件
 * 在插件模式下显示，用于插件内二次输入
 */

<template>
  <div v-if="visible" class="sub-input-wrapper">
    <input
      ref="inputRef"
      v-model="inputValue"
      type="text"
      class="sub-input"
      :placeholder="placeholder"
      @input="onInput"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue';

const props = defineProps<{
  visible: boolean;
  placeholder?: string;
  modelValue?: string;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
  (e: 'change', value: string): void;
}>();

const inputRef = ref<HTMLInputElement>();
const inputValue = ref(props.modelValue || '');

// 输入变化
function onInput() {
  emit('update:modelValue', inputValue.value);
  emit('change', inputValue.value);
}

// 监听外部值变化
watch(() => props.modelValue, (newVal) => {
  if (newVal !== undefined && newVal !== inputValue.value) {
    inputValue.value = newVal;
  }
});

// 显示时聚焦
watch(() => props.visible, (visible) => {
  if (visible) {
    setTimeout(() => inputRef.value?.focus(), 0);
  }
});

// 暴露方法
defineExpose({
  focus: () => inputRef.value?.focus(),
  setValue: (value: string) => {
    inputValue.value = value;
  },
  clear: () => {
    inputValue.value = '';
  },
});
</script>

<style scoped>
.sub-input-wrapper {
  display: flex;
  padding: 0 16px;
  height: 40px;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.sub-input {
  flex: 1;
  height: 100%;
  border: none;
  outline: none;
  font-size: 14px;
  background: transparent;
  color: var(--text-primary);
}

.sub-input::placeholder {
  color: var(--text-tertiary);
}
</style>