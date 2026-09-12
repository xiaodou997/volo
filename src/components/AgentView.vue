/**
 * Agent 视图组件
 * 展示 AI Agent 会话时间线：用户问题 / 工具调用 / 工具结果 / 回答 / 错误
 */

<template>
  <div class="agent-view">
    <AgentHeader
      :title="headerTitle"
      :chat-mode="viewMode === 'chat'"
      :finished="finished"
      :stopping="stopping"
      @back="onBack"
      @stop="stopSession"
      @history="openHistory"
      @new-session="newSession"
    />

    <!-- 会话历史列表 -->
    <div v-if="viewMode === 'history'" class="agent-content">
      <div v-if="sessionsLoading" class="loading-hint">加载中…</div>
      <div v-else-if="sessionsError" class="session-error">{{ sessionsError }}</div>
      <div v-else-if="sessions.length === 0" class="loading-hint">暂无历史会话</div>
      <div
        v-for="s in sessions"
        :key="s.id"
        class="session-item"
        @click="openSession(s.id)"
      >
        <div class="session-time">{{ formatTime(s.startedAt) }}</div>
        <div class="session-preview">{{ s.preview }}</div>
      </div>
    </div>

    <!-- 会话时间线 / 历史回放 -->
    <div v-else class="agent-content" ref="contentRef">
      <!-- 用户问题（实时会话首轮；从回放续聊时不重复展示，时间线里已有） -->
      <div v-if="viewMode === 'chat' && !resumed" class="user-question">{{ query }}</div>

      <!-- 等待首个事件 -->
      <div v-if="viewMode === 'chat' && loading" class="loading-hint">思考中…</div>

      <!-- 事件时间线 -->
      <div
        v-for="(item, index) in displayTimeline"
        :key="index"
        class="timeline-item"
        :class="'timeline-' + item.kind"
      >
        <template v-if="item.kind === 'user'">
          <div class="user-question item-user">
            {{ item.text }}
            <div v-if="item.images?.length" class="user-attachments">
              <img v-for="(img, i) in item.images" :key="i" :src="img" class="user-attach-thumb" alt="图片附件" />
            </div>
            <span v-else-if="item.imageCount" class="user-attach-badge">🖼 图片 × {{ item.imageCount }}</span>
            <span v-for="name in item.files" :key="name" class="user-attach-badge">📎 {{ name }}</span>
          </div>
        </template>
        <template v-else-if="item.kind === 'tool_call'">
          <div class="item-title">🔧 调用 {{ item.text }}</div>
          <div v-if="item.detail" class="item-detail">{{ item.detail }}</div>
        </template>
        <template v-else-if="item.kind === 'tool_result'">
          <div class="item-detail">{{ item.text }}</div>
          <!-- 显示的是截断版，复制取完整结果 -->
          <button
            class="msg-action-btn tool-copy-btn"
            @click="copyMessage('tool-' + index, item.fullText ?? item.text)"
          >{{ copiedKey === 'tool-' + index ? '已复制 ✓' : '复制结果' }}</button>
        </template>
        <template v-else>
          <div class="item-body markdown-body" v-html="renderMarkdown(item.text)"></div>
          <span v-if="item.streaming" class="stream-cursor"></span>
          <div
            v-if="item.kind === 'message' && !item.streaming && item.text"
            class="msg-actions"
          >
            <button
              v-if="viewMode === 'chat' && finished"
              class="msg-action-btn"
              @click="quoteMessage(item.text)"
            >引用</button>
            <button
              class="msg-action-btn"
              @click="copyMessage('msg-' + index, item.text)"
            >{{ copiedKey === 'msg-' + index ? '已复制 ✓' : '复制' }}</button>
          </div>
          <button
            v-if="item.kind === 'error' && viewMode === 'chat' && finished && lastAsk"
            class="msg-action-btn retry-btn"
            :disabled="retrying"
            @click="retryLastAsk"
          >{{ retrying ? '重试中…' : '重试' }}</button>
        </template>
      </div>

      <div v-if="viewMode === 'replay'" class="done-marker">— 回放 —</div>
      <div v-else-if="finished && !hasError" class="done-marker">✓ 已完成</div>
    </div>

    <!-- 回放底栏：从该会话继续对话 -->
    <div v-if="viewMode === 'replay'" class="follow-up-bar">
      <span v-if="resumeError" class="session-error resume-error">{{ resumeError }}</span>
      <button
        class="follow-up-send resume-btn"
        :disabled="resuming"
        @click="resumeSession"
      >{{ resuming ? '恢复中…' : '继续对话' }}</button>
    </div>

    <AgentFollowUp
      v-if="viewMode === 'chat' && finished"
      ref="followUpRef"
      v-model="followUp"
      :finished="finished"
      :pending-images="pendingImages"
      :pending-files="pendingFiles"
      :attachment-error="attachmentError"
      @paste="onPaste"
      @send="sendFollowUp"
      @remove-image="pendingImages.splice($event, 1)"
      @remove-file="pendingFiles.splice($event, 1)"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { marked } from 'marked';
import DOMPurify from 'dompurify';
import type { AgentEvent, ReplayEvent, SessionMeta } from '../api/rubick';
import AgentFollowUp from './agent/AgentFollowUp.vue';
import AgentHeader from './agent/AgentHeader.vue';
import {
  buildQueryWithTextAttachments,
  classifyAttachment,
  type TextAttachment,
} from '../agent/attachments';
import {
  reduceTimelineEvent,
  replayEventsToTimeline,
  type TimelineItem,
} from '../agent/timeline';

marked.setOptions({ breaks: true });

function renderMarkdown(text: string): string {
  const html = marked.parse(text, { async: false });
  return DOMPurify.sanitize(html);
}

const props = defineProps<{
  query: string;
  initialMode?: 'chat' | 'history';
  skill?: string;
}>();

const emit = defineEmits<{
  (e: 'exit'): void;
}>();

type ViewMode = 'chat' | 'history' | 'replay';

const timeline = ref<TimelineItem[]>([]);
const loading = ref(true);
const finished = ref(false);
const contentRef = ref<HTMLElement | null>(null);

const viewMode = ref<ViewMode>('chat');
const replayTimeline = ref<TimelineItem[]>([]);
const sessions = ref<SessionMeta[]>([]);
const sessionsLoading = ref(false);
const sessionsError = ref('');
const followUp = ref('');
const pendingImages = ref<string[]>([]);
const pendingFiles = ref<TextAttachment[]>([]);
const attachmentError = ref('');

function readAsDataURL(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

async function onPaste(e: ClipboardEvent) {
  const files = Array.from(e.clipboardData?.items ?? [])
    .filter((item) => item.kind === 'file')
    .map((item) => item.getAsFile())
    .filter((f): f is File => !!f);
  if (files.length === 0) return;
  e.preventDefault();

  for (const file of files) {
    const decision = classifyAttachment(file);
    if (decision.kind === 'reject') {
      attachmentError.value = decision.message;
      continue;
    }
    if (decision.kind === 'image') {
      pendingImages.value.push(await readAsDataURL(file));
      continue;
    }
    pendingFiles.value.push({ name: file.name || '未命名.txt', text: await file.text() });
  }
}

const stopping = ref(false);
const currentSessionId = ref<string | null>(null);
const resumed = ref(false);
const resuming = ref(false);
const resumeError = ref('');

const hasError = computed(() => timeline.value.some((item) => item.kind === 'error'));
const displayTimeline = computed(() =>
  viewMode.value === 'replay' ? replayTimeline.value : timeline.value
);

const headerTitle = computed(() => {
  if (viewMode.value === 'history') return '历史会话';
  if (viewMode.value === 'replay') return '会话回放';
  return props.skill ? `问 AI · @${props.skill}` : '问 AI';
});

function formatTime(iso: string): string {
  const d = new Date(iso);
  return isNaN(d.getTime()) ? iso : d.toLocaleString();
}

function onBack() {
  if (viewMode.value === 'replay') {
    viewMode.value = 'history';
  } else if (viewMode.value === 'history') {
    if (props.initialMode === 'history') emit('exit');
    else viewMode.value = 'chat';
  } else {
    emit('exit');
  }
}

async function openHistory() {
  viewMode.value = 'history';
  sessionsLoading.value = true;
  sessionsError.value = '';
  try {
    sessions.value = await invoke<SessionMeta[]>('agent_list_sessions');
  } catch (e) {
    sessionsError.value = String(e);
  } finally {
    sessionsLoading.value = false;
  }
}

async function openSession(sessionId: string) {
  sessionsError.value = '';
  resumeError.value = '';
  try {
    const events = await invoke<ReplayEvent[]>('agent_read_session', { sessionId });
    replayTimeline.value = replayEventsToTimeline(events);
    currentSessionId.value = sessionId;
    viewMode.value = 'replay';
    void enhanceCodeBlocks();
  } catch (e) {
    sessionsError.value = String(e);
  }
}

async function resumeSession() {
  if (!currentSessionId.value || resuming.value) return;
  resumeError.value = '';
  resuming.value = true;
  try {
    await invoke('agent_resume_session', { sessionId: currentSessionId.value });
    timeline.value = [...replayTimeline.value];
    replayTimeline.value = [];
    resumed.value = true;
    loading.value = false;
    finished.value = true;
    viewMode.value = 'chat';
    await scrollToBottom();
  } catch (e) {
    resumeError.value = String(e);
  } finally {
    resuming.value = false;
  }
}

async function newSession() {
  try {
    await invoke('agent_new_session');
  } catch (e) {
    console.warn('agent_new_session 失败', e);
  }
  emit('exit');
}

async function stopSession() {
  if (stopping.value) return;
  stopping.value = true;
  try {
    await invoke('agent_cancel');
  } catch (e) {
    console.warn('agent_cancel 失败', e);
    stopping.value = false;
  }
}

async function sendFollowUp() {
  const q = followUp.value.trim();
  const images = [...pendingImages.value];
  const files = [...pendingFiles.value];
  if ((!q && !images.length && !files.length) || !finished.value) return;

  const fullQuery = buildQueryWithTextAttachments(q, files);
  followUp.value = '';
  pendingImages.value = [];
  pendingFiles.value = [];
  attachmentError.value = '';
  lastAsk.value = { query: fullQuery, skill: null, images: images.length ? images : null };
  timeline.value.push({
    kind: 'user',
    text: q || '（见附件）',
    images,
    files: files.map((f) => f.name),
  });
  loading.value = true;
  finished.value = false;
  await scrollToBottom();
  try {
    await invoke('agent_ask', { query: fullQuery, skill: null, images });
  } catch (e) {
    loading.value = false;
    finished.value = true;
    timeline.value.push({ kind: 'error', text: String(e) });
  }
}

async function scrollToBottom() {
  await nextTick();
  if (contentRef.value) contentRef.value.scrollTop = contentRef.value.scrollHeight;
}

const copiedKey = ref('');

async function copyMessage(key: string, text: string) {
  try {
    await navigator.clipboard.writeText(text);
    copiedKey.value = key;
    setTimeout(() => {
      if (copiedKey.value === key) copiedKey.value = '';
    }, 1500);
  } catch (e) {
    console.warn('复制失败', e);
  }
}

const followUpRef = ref<InstanceType<typeof AgentFollowUp> | null>(null);

function quoteMessage(text: string) {
  const quote =
    text
      .split('\n')
      .map((line) => '> ' + line)
      .join('\n') + '\n\n';
  followUp.value = quote + followUp.value;
  void nextTick(() => followUpRef.value?.focus());
}

const lastAsk = ref<{ query: string; skill: string | null; images: string[] | null } | null>(null);
const retrying = ref(false);

async function retryLastAsk() {
  if (!lastAsk.value || retrying.value) return;
  retrying.value = true;
  while (timeline.value.length && timeline.value[timeline.value.length - 1].kind === 'error') {
    timeline.value.pop();
  }
  loading.value = true;
  finished.value = false;
  await scrollToBottom();
  try {
    await invoke('agent_ask', lastAsk.value);
  } catch (e) {
    loading.value = false;
    finished.value = true;
    timeline.value.push({ kind: 'error', text: String(e) });
  } finally {
    retrying.value = false;
  }
}

async function enhanceCodeBlocks() {
  await nextTick();
  contentRef.value?.querySelectorAll('pre:not(.copy-enhanced)').forEach((pre) => {
    pre.classList.add('copy-enhanced');
    const btn = document.createElement('button');
    btn.className = 'code-copy-btn';
    btn.textContent = '复制';
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      const codeEl = pre.querySelector('code');
      const code =
        codeEl?.textContent ??
        [...pre.childNodes]
          .filter((n) => n.nodeName !== 'BUTTON')
          .map((n) => n.textContent ?? '')
          .join('');
      void navigator.clipboard.writeText(code).then(() => {
        btn.textContent = '已复制 ✓';
        setTimeout(() => {
          btn.textContent = '复制';
        }, 1500);
      });
    });
    pre.appendChild(btn);
  });
}

async function handleEvent(event: AgentEvent) {
  loading.value = false;
  const transition = reduceTimelineEvent(timeline.value, event);
  timeline.value = transition.timeline;
  if (transition.finished !== undefined) finished.value = transition.finished;
  if (transition.stopping !== undefined) stopping.value = transition.stopping;
  await scrollToBottom();
  void enhanceCodeBlocks();
}

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  unlisten = await listen<AgentEvent>('agent-event', (event) => {
    void handleEvent(event.payload);
  });
  if (props.initialMode === 'history') {
    loading.value = false;
    finished.value = true;
    await openHistory();
    return;
  }
  try {
    lastAsk.value = { query: props.query, skill: props.skill ?? null, images: null };
    await invoke('agent_ask', lastAsk.value);
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
  word-break: break-word;
}

.markdown-body :deep(p) {
  margin: 0 0 8px;
}

.markdown-body :deep(p:last-child) {
  margin-bottom: 0;
}

.markdown-body :deep(h1),
.markdown-body :deep(h2),
.markdown-body :deep(h3),
.markdown-body :deep(h4) {
  margin: 12px 0 6px;
  font-size: 15px;
}

.markdown-body :deep(ul),
.markdown-body :deep(ol) {
  margin: 4px 0;
  padding-left: 20px;
}

.markdown-body :deep(code) {
  font-family: ui-monospace, monospace;
  font-size: 13px;
  background: var(--bg-secondary);
  padding: 1px 5px;
  border-radius: 4px;
}

.markdown-body :deep(pre) {
  background: var(--bg-secondary);
  padding: 10px 12px;
  border-radius: 6px;
  overflow-x: auto;
  margin: 8px 0;
  position: relative;
}

.markdown-body :deep(pre code) {
  background: none;
  padding: 0;
}

.markdown-body :deep(.code-copy-btn) {
  position: absolute;
  top: 6px;
  right: 8px;
  font-size: 12px;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-tertiary);
  border-radius: 4px;
  padding: 2px 8px;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s;
}

.markdown-body :deep(pre:hover .code-copy-btn) {
  opacity: 1;
}

.timeline-message {
  position: relative;
}

.msg-actions {
  position: absolute;
  top: 6px;
  right: 0;
  display: flex;
  gap: 6px;
  opacity: 0;
  transition: opacity 0.15s;
}

.timeline-message:hover .msg-actions {
  opacity: 1;
}

.msg-action-btn {
  font-size: 12px;
  border: 1px solid var(--border-color);
  background: var(--bg-secondary);
  color: var(--text-tertiary);
  border-radius: 4px;
  padding: 2px 8px;
  cursor: pointer;
}

.msg-action-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.timeline-tool_result {
  position: relative;
}

.tool-copy-btn {
  position: absolute;
  top: 8px;
  right: 0;
  opacity: 0;
  transition: opacity 0.15s;
}

.timeline-tool_result:hover .tool-copy-btn {
  opacity: 1;
}

.retry-btn {
  margin-top: 6px;
}

.markdown-body :deep(blockquote) {
  margin: 8px 0;
  padding-left: 12px;
  border-left: 3px solid var(--border-color);
  color: var(--text-secondary);
}

.markdown-body :deep(a) {
  color: var(--accent-color);
}

.markdown-body :deep(table) {
  border-collapse: collapse;
  margin: 8px 0;
}

.markdown-body :deep(th),
.markdown-body :deep(td) {
  border: 1px solid var(--border-color);
  padding: 4px 10px;
  font-size: 13px;
}

.stream-cursor {
  display: inline-block;
  width: 8px;
  height: 1em;
  margin-left: 2px;
  vertical-align: text-bottom;
  background: var(--accent-color);
  animation: blink 0.8s step-end infinite;
}

@keyframes blink {
  0%, 100% { opacity: 1; }
  50% { opacity: 0; }
}

.timeline-error .item-body {
  color: var(--danger-color);
}

.done-marker {
  padding: 8px 0;
  font-size: 12px;
  color: var(--text-tertiary);
}

.timeline-user {
  border-bottom: none;
  padding: 0;
}

.item-user {
  margin-bottom: 0;
}

.session-item {
  padding: 10px 12px;
  border-bottom: 1px solid var(--border-color);
  border-radius: 6px;
  cursor: pointer;
  transition: background 0.2s;
}

.session-item:hover {
  background: var(--hover-bg);
}

.session-time {
  font-size: 12px;
  color: var(--text-tertiary);
}

.session-preview {
  font-size: 14px;
  color: var(--text-primary);
  margin-top: 2px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.session-error {
  font-size: 13px;
  color: var(--danger-color);
  padding: 8px 0;
}

.follow-up-bar {
  display: flex;
  gap: 8px;
  padding: 10px 16px;
  border-top: 1px solid var(--border-color);
  background: var(--bg-secondary);
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

.resume-error {
  flex: 1;
  padding: 0;
}

.resume-btn {
  margin-left: auto;
}

.user-attachments {
  display: flex;
  gap: 6px;
  margin-top: 6px;
  flex-wrap: wrap;
}

.user-attach-thumb {
  max-width: 120px;
  max-height: 120px;
  border-radius: 6px;
}

.user-attach-badge {
  display: inline-block;
  font-size: 12px;
  opacity: 0.8;
  margin-top: 4px;
  margin-right: 6px;
}
</style>
