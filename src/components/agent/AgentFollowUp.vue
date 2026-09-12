<template>
  <div class="follow-up-area">
    <div v-if="pendingImages.length || pendingFiles.length" class="attachment-chips">
      <span v-for="(img, i) in pendingImages" :key="'img' + i" class="chip">
        <img :src="img" class="chip-thumb" alt="图片附件" />
        <button class="chip-remove" @click="$emit('removeImage', i)">×</button>
      </span>
      <span v-for="(file, i) in pendingFiles" :key="'file' + i" class="chip">
        📎 {{ file.name }}
        <button class="chip-remove" @click="$emit('removeFile', i)">×</button>
      </span>
    </div>

    <div v-if="attachmentError" class="attachment-error">{{ attachmentError }}</div>

    <div class="follow-up-bar">
      <input
        ref="inputRef"
        :value="modelValue"
        type="text"
        class="follow-up-input"
        placeholder="继续追问…（可直接粘贴截图 / 文本文件）"
        :disabled="!finished"
        @input="onInput"
        @keydown.enter="$emit('send')"
        @paste="$emit('paste', $event)"
      />
      <button
        class="follow-up-send"
        :disabled="sendDisabled"
        @click="$emit('send')"
      >发送</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import type { TextAttachment } from '../../agent/attachments';

const props = defineProps<{
  modelValue: string;
  finished: boolean;
  pendingImages: string[];
  pendingFiles: TextAttachment[];
  attachmentError: string;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
  (e: 'send'): void;
  (e: 'paste', event: ClipboardEvent): void;
  (e: 'removeImage', index: number): void;
  (e: 'removeFile', index: number): void;
}>();

const inputRef = ref<HTMLInputElement | null>(null);

const sendDisabled = computed(
  () =>
    !props.finished ||
    (!props.modelValue.trim() && !props.pendingImages.length && !props.pendingFiles.length),
);

function onInput(event: Event) {
  emit('update:modelValue', (event.target as HTMLInputElement).value);
}

function focus() {
  inputRef.value?.focus();
}

defineExpose({ focus });
</script>

<style scoped>
.follow-up-bar {
  display: flex;
  gap: 8px;
  padding: 10px 16px;
  border-top: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.follow-up-input {
  flex: 1;
  border: none;
  outline: none;
  font-size: 14px;
  padding: 8px 12px;
  border-radius: 6px;
  background: var(--bg-primary);
  color: var(--text-primary);
}

.follow-up-input::placeholder {
  color: var(--text-tertiary);
}

.follow-up-input:disabled {
  opacity: 0.6;
}

.follow-up-send {
  border: none;
  border-radius: 6px;
  padding: 0 14px;
  font-size: 13px;
  background: var(--accent-color);
  color: #fff;
  cursor: pointer;
  transition: opacity 0.2s;
}

.follow-up-send:disabled {
  opacity: 0.5;
  cursor: default;
}

.attachment-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  padding: 10px 16px 0;
  background: var(--bg-secondary);
}

.chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary);
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  padding: 4px 8px;
}

.chip-thumb {
  width: 32px;
  height: 32px;
  object-fit: cover;
  border-radius: 4px;
}

.chip-remove {
  border: none;
  background: none;
  color: var(--text-tertiary);
  cursor: pointer;
  font-size: 14px;
  padding: 0 2px;
}

.chip-remove:hover {
  color: var(--danger-color);
}

.attachment-error {
  font-size: 12px;
  color: var(--danger-color);
  padding: 6px 16px 0;
  background: var(--bg-secondary);
}
</style>
