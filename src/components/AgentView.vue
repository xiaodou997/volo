/**
 * Agent 视图组件
 * 展示 AI Agent 会话时间线：用户问题 / 工具调用 / 工具结果 / 回答 / 错误
 */

<template>
  <div class="agent-view">
    <!-- 顶部导航 -->
    <div class="agent-header">
      <button class="back-btn" @click="onBack">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M19 12H5M12 19l-7-7 7-7"/>
        </svg>
      </button>
      <h2 class="title">{{ headerTitle }}</h2>
      <span v-if="viewMode === 'chat' && !finished" class="running-dot"></span>
      <div class="header-actions">
        <button
          v-if="viewMode === 'chat' && !finished"
          class="header-btn stop-btn"
          :disabled="stopping"
          @click="stopSession"
        >{{ stopping ? '停止中…' : '停止' }}</button>
        <button v-if="viewMode === 'chat'" class="header-btn" @click="openHistory">历史</button>
        <button class="header-btn" @click="newSession">新对话</button>
      </div>
    </div>

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
          <div class="user-question item-user">{{ item.text }}</div>
        </template>
        <template v-else-if="item.kind === 'tool_call'">
          <div class="item-title">🔧 调用 {{ item.text }}</div>
          <div v-if="item.detail" class="item-detail">{{ item.detail }}</div>
        </template>
        <template v-else-if="item.kind === 'tool_result'">
          <div class="item-detail">{{ item.text }}</div>
        </template>
        <template v-else>
          <div class="item-body markdown-body" v-html="renderMarkdown(item.text)"></div>
          <span v-if="item.streaming" class="stream-cursor"></span>
        </template>
      </div>

      <!-- 完成 / 回放标记 -->
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

    <!-- 追问输入栏（实时会话结束后才可用） -->
    <div v-if="viewMode === 'chat' && finished" class="follow-up-bar">
      <input
        v-model="followUp"
        type="text"
        class="follow-up-input"
        placeholder="继续追问…"
        :disabled="!finished"
        @keydown.enter="sendFollowUp"
      />
      <button
        class="follow-up-send"
        :disabled="!finished || !followUp.trim()"
        @click="sendFollowUp"
      >发送</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { marked } from 'marked';
import DOMPurify from 'dompurify';
import type { AgentEvent, ReplayEvent, SessionMeta } from '../api/rubick';

// LLM 输出按 Markdown 渲染；DOMPurify 消毒防注入（内容来自外部模型，不可信）
marked.setOptions({ breaks: true });

function renderMarkdown(text: string): string {
  const html = marked.parse(text, { async: false });
  return DOMPurify.sanitize(html);
}

const props = defineProps<{
  query: string;
  // 打开方式：chat（默认，带上 query 立即发问）/ history（直达历史列表，不发问）
  initialMode?: 'chat' | 'history';
  // @技能名 显式触发：首轮会话注入该技能完整指令
  skill?: string;
}>();

const emit = defineEmits<{
  (e: 'exit'): void;
}>();

interface TimelineItem {
  kind: 'user' | 'message' | 'tool_call' | 'tool_result' | 'error';
  text: string;
  detail?: string;
  // 流式输出进行中（末尾显示闪烁光标）
  streaming?: boolean;
}

// 视图模式：实时会话 / 历史列表 / 历史回放
type ViewMode = 'chat' | 'history' | 'replay';

const timeline = ref<TimelineItem[]>([]);
const loading = ref(true);
const finished = ref(false);
const contentRef = ref<HTMLElement | null>(null);

const viewMode = ref<ViewMode>('chat');
// 回放时间线独立于实时 timeline，返回实时会话时原样恢复
const replayTimeline = ref<TimelineItem[]>([]);
const sessions = ref<SessionMeta[]>([]);
const sessionsLoading = ref(false);
const sessionsError = ref('');
const followUp = ref('');
// 停止按钮状态（等待后端 done 事件落地）
const stopping = ref(false);
// 从回放继续会话：当前回放的会话 id / 是否已恢复为实时会话 / 恢复中与错误状态
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

// 返回键：回放 → 历史列表 → 实时会话 → 退出；
// 历史直达入口（initialMode=history）没有实时会话可回，从历史列表直接退出
function onBack() {
  if (viewMode.value === 'replay') {
    viewMode.value = 'history';
  } else if (viewMode.value === 'history') {
    if (props.initialMode === 'history') {
      emit('exit');
    } else {
      viewMode.value = 'chat';
    }
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

function replayToTimeline(event: ReplayEvent): TimelineItem | null {
  switch (event.kind) {
    case 'user':
      return { kind: 'user', text: event.content ?? '' };
    case 'message':
      return event.content ? { kind: 'message', text: event.content } : null;
    case 'tool_call':
      return {
        kind: 'tool_call',
        text: event.name ?? 'unknown',
        detail: truncate(JSON.stringify(event.args ?? {}), 100),
      };
    case 'tool_result':
      return { kind: 'tool_result', text: truncate(event.result ?? '', 200) };
    case 'error':
      return { kind: 'error', text: event.content ?? '未知错误' };
  }
}

async function openSession(sessionId: string) {
  sessionsError.value = '';
  resumeError.value = '';
  try {
    const events = await invoke<ReplayEvent[]>('agent_read_session', { sessionId });
    replayTimeline.value = events
      .map(replayToTimeline)
      .filter((item): item is TimelineItem => item !== null);
    currentSessionId.value = sessionId;
    viewMode.value = 'replay';
  } catch (e) {
    sessionsError.value = String(e);
  }
}

// 从回放继续会话：后端恢复消息级历史后，把回放时间线并入实时时间线，
// 切到 chat 模式（finished=true 使追问输入栏可用）
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

// 新对话：清空后端会话历史后退出，回到启动器搜索框
async function newSession() {
  try {
    await invoke('agent_new_session');
  } catch (e) {
    console.warn('agent_new_session 失败', e);
  }
  emit('exit');
}

// 停止当前会话：置位取消标志，流式输出随下一个增量中断；
// 完成后端会 emit done，handleEvent 把 finished 置 true
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

// 多轮追问：本地先压入 user 气泡，再发起新一轮 agent_ask
async function sendFollowUp() {
  const q = followUp.value.trim();
  if (!q || !finished.value) return;
  followUp.value = '';
  timeline.value.push({ kind: 'user', text: q });
  loading.value = true;
  finished.value = false;
  await scrollToBottom();
  try {
    // 追问不带 skill：首轮已注入 system prompt，历史续接即可
    await invoke('agent_ask', { query: q, skill: null });
  } catch (e) {
    loading.value = false;
    finished.value = true;
    timeline.value.push({ kind: 'error', text: String(e) });
  }
}

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
  // 流式增量片段：追加到当前正在流式输出的气泡（没有则新建一个）
  if (event.kind === 'message' && event.delta) {
    const last = timeline.value[timeline.value.length - 1];
    if (last && last.kind === 'message' && last.streaming) {
      last.text += event.content ?? '';
    } else {
      timeline.value.push({ kind: 'message', text: event.content ?? '', streaming: true });
    }
    await scrollToBottom();
    return;
  }
  // 任何非增量事件到来，说明上一段流式输出已结束
  const last = timeline.value[timeline.value.length - 1];
  if (last?.streaming) {
    last.streaming = false;
  }
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
      stopping.value = false;
      break;
    case 'done':
      finished.value = true;
      stopping.value = false;
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
  // 历史直达：不发起会话，直接打开历史列表（之后可从回放恢复续聊）
  if (props.initialMode === 'history') {
    loading.value = false;
    finished.value = true;
    await openHistory();
    return;
  }
  try {
    await invoke('agent_ask', { query: props.query, skill: props.skill ?? null });
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
  word-break: break-word;
}

/* Markdown 渲染内容（v-html 注入，scoped 需 :deep） */
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
}

.markdown-body :deep(pre code) {
  background: none;
  padding: 0;
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

/* 流式输出中的闪烁光标 */
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

.header-actions {
  display: flex;
  gap: 4px;
  margin-left: 8px;
}

.header-btn {
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  padding: 4px 8px;
  border-radius: 6px;
  cursor: pointer;
  transition: background 0.2s, color 0.2s;
}

.header-btn:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

/* 停止按钮：hover 用警示色提示中断语义 */
.stop-btn:hover {
  color: var(--danger-color);
}

/* 追问/回放中的用户气泡在时间线内，去掉分隔线与额外内边距 */
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

/* 回放底栏：错误文本占满剩余空间，按钮靠右 */
.resume-error {
  flex: 1;
  padding: 0;
}

.resume-btn {
  margin-left: auto;
}
</style>
