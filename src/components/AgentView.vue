/**
 * Agent 视图组件
 * 展示 AI Agent 会话时间线：用户问题 / 工具调用 / 工具结果 / 回答 / 错误
 */

<template>
  <div class="agent-view">
    <!-- 顶部导航 -->
    <div class="agent-header">
      <button class="back-btn" @click="$emit('exit')">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M19 12H5M12 19l-7-7 7-7"/>
        </svg>
      </button>
      <h2 class="title">问 AI</h2>
      <span v-if="!finished" class="running-dot"></span>
    </div>

    <!-- 会话时间线 -->
    <div class="agent-content" ref="contentRef">
      <!-- 用户问题 -->
      <div class="user-question">{{ query }}</div>

      <!-- 等待首个事件 -->
      <div v-if="loading" class="loading-hint">思考中…</div>

      <!-- 事件时间线 -->
      <div
        v-for="(item, index) in timeline"
        :key="index"
        class="timeline-item"
        :class="'timeline-' + item.kind"
      >
        <template v-if="item.kind === 'tool_call'">
          <div class="item-title">🔧 调用 {{ item.text }}</div>
          <div v-if="item.detail" class="item-detail">{{ item.detail }}</div>
        </template>
        <template v-else-if="item.kind === 'tool_result'">
          <div class="item-detail">{{ item.text }}</div>
        </template>
        <template v-else>
          <div class="item-body">{{ item.text }}</div>
        </template>
      </div>

      <!-- 完成标记 -->
      <div v-if="finished && !hasError" class="done-marker">✓ 已完成</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import type { AgentEvent } from '../api/rubick';

const props = defineProps<{
  query: string;
}>();

const emit = defineEmits<{
  (e: 'exit'): void;
}>();

interface TimelineItem {
  kind: 'message' | 'tool_call' | 'tool_result' | 'error';
  text: string;
  detail?: string;
}

const timeline = ref<TimelineItem[]>([]);
const loading = ref(true);
const finished = ref(false);
const contentRef = ref<HTMLElement | null>(null);

const hasError = computed(() => timeline.value.some((item) => item.kind === 'error'));

function truncate(s: string, max: number): string {
  return s.length > max ? s.slice(0, max) + '…' : s;
}

async function scrollToBottom() {
  await nextTick();
  if (contentRef.value) {
    contentRef.value.scrollTop = contentRef.value.scrollHeight;
  }
}

async function handleEvent(event: AgentEvent) {
  loading.value = false;
  switch (event.kind) {
    case 'message':
      if (event.content) {
        timeline.value.push({ kind: 'message', text: event.content });
      }
      break;
    case 'tool_call':
      timeline.value.push({
        kind: 'tool_call',
        text: event.name ?? 'unknown',
        detail: truncate(JSON.stringify(event.args ?? {}), 100),
      });
      break;
    case 'tool_result':
      timeline.value.push({
        kind: 'tool_result',
        text: truncate(event.result ?? '', 200),
      });
      break;
    case 'error':
      timeline.value.push({ kind: 'error', text: event.content ?? '未知错误' });
      finished.value = true;
      break;
    case 'done':
      finished.value = true;
      break;
  }
  await scrollToBottom();
}

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  // 先监听再发起会话，避免漏掉早期事件
  unlisten = await listen<AgentEvent>('agent-event', (event) => {
    void handleEvent(event.payload);
  });
  try {
    await invoke('agent_ask', { query: props.query });
  } catch (e) {
    loading.value = false;
    finished.value = true;
    timeline.value.push({ kind: 'error', text: String(e) });
  }
});

onUnmounted(() => {
  unlisten?.();
  invoke('agent_cancel').catch(() => {});
});
</script>

<style scoped>
.agent-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
}

.agent-header {
  display: flex;
  align-items: center;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.back-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border: none;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 6px;
  transition: background 0.2s;
}

.back-btn:hover {
  background: var(--hover-bg);
}

.title {
  flex: 1;
  margin-left: 12px;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.running-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--accent-color);
  animation: pulse 1s infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.3; }
}

.agent-content {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
}

.user-question {
  padding: 10px 12px;
  font-size: 14px;
  font-weight: 500;
  color: var(--text-primary);
  background: var(--bg-secondary);
  border-radius: 8px;
  margin-bottom: 12px;
}

.loading-hint {
  font-size: 13px;
  color: var(--text-tertiary);
  padding: 4px 0;
}

.timeline-item {
  padding: 8px 0;
  border-bottom: 1px solid var(--border-color);
}

.timeline-item:last-child {
  border-bottom: none;
}

.item-title {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
}

.item-detail {
  font-size: 12px;
  font-family: monospace;
  color: var(--text-tertiary);
  margin-top: 4px;
  word-break: break-all;
  white-space: pre-wrap;
}

.item-body {
  font-size: 14px;
  color: var(--text-primary);
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-word;
}

.timeline-error .item-body {
  color: var(--danger-color);
}

.done-marker {
  padding: 8px 0;
  font-size: 12px;
  color: var(--text-tertiary);
}
</style>
