<script setup lang="ts">
import { onMounted, ref, shallowRef } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import {
  DEFAULT_WORKFLOW_INPUT,
  DEFAULT_WORKFLOW_TEXT,
  formatWorkflowRunDuration,
  formatWorkflowValue,
  parseWorkflowDefinition,
  parseWorkflowInput,
  toWorkflowOptions,
  workflowRunErrorText,
  workflowRunFailedStep,
  workflowStepLabel,
  type WorkflowDefinition,
  type WorkflowExecution,
  type WorkflowOption,
  type WorkflowRunRecord,
} from '../workflow/model';

defineEmits<{ back: [] }>();

const workflowText = ref(DEFAULT_WORKFLOW_TEXT);
const inputText = ref(DEFAULT_WORKFLOW_INPUT);
const running = ref(false);
const storageBusy = ref(false);
const historyBusy = ref(false);
const selectedWorkflowId = ref('');
const storageStatus = ref('');
const error = ref('');
const historyError = ref('');
const resultMode = ref<'current' | 'history'>('current');

// 完整定义、execution 和 history 都按整体值替换，不需要 Vue 深层代理。
const savedWorkflows = shallowRef<WorkflowDefinition[]>([]);
const savedOptions = shallowRef<WorkflowOption[]>([]);
const execution = shallowRef<WorkflowExecution | null>(null);
const lastWorkflow = shallowRef<WorkflowDefinition | null>(null);
const runHistory = shallowRef<WorkflowRunRecord[]>([]);

function errorText(value: unknown): string {
  if (value instanceof Error) return value.message;
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function formatRunStartedAt(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  });
}

async function refreshWorkflows(selectId?: string) {
  const workflows = await invoke<WorkflowDefinition[]>('workflow_list');
  savedWorkflows.value = workflows;
  savedOptions.value = toWorkflowOptions(workflows);

  if (selectId && workflows.some((workflow) => workflow.id === selectId)) {
    selectedWorkflowId.value = selectId;
  } else if (
    selectedWorkflowId.value &&
    !workflows.some((workflow) => workflow.id === selectedWorkflowId.value)
  ) {
    selectedWorkflowId.value = '';
  }
}

async function refreshRunHistory() {
  if (historyBusy.value) return;

  historyBusy.value = true;
  historyError.value = '';
  try {
    runHistory.value = await invoke<WorkflowRunRecord[]>('workflow_list_runs', {
      workflowId: null,
    });
  } catch (value) {
    historyError.value = errorText(value);
  } finally {
    historyBusy.value = false;
  }
}

function loadSelectedWorkflow() {
  const workflow = savedWorkflows.value.find(
    (item) => item.id === selectedWorkflowId.value,
  );
  if (!workflow) return;

  workflowText.value = JSON.stringify(workflow, null, 2);
  execution.value = null;
  lastWorkflow.value = null;
  error.value = '';
  storageStatus.value = `已加载 ${workflow.name}`;
}

async function saveWorkflowDefinition() {
  if (storageBusy.value) return;

  error.value = '';
  storageStatus.value = '';

  try {
    const workflow = parseWorkflowDefinition(workflowText.value);
    const previousId = selectedWorkflowId.value;
    storageBusy.value = true;

    await invoke('workflow_save', { workflow });
    await refreshWorkflows(workflow.id);
    storageStatus.value =
      previousId && previousId !== workflow.id
        ? `已另存为 ${workflow.name}`
        : `已保存 ${workflow.name}`;
  } catch (value) {
    error.value = errorText(value);
  } finally {
    storageBusy.value = false;
  }
}

async function deleteSelectedWorkflow() {
  if (storageBusy.value || !selectedWorkflowId.value) return;

  const workflow = savedWorkflows.value.find(
    (item) => item.id === selectedWorkflowId.value,
  );
  if (!workflow) return;
  if (!window.confirm(`删除 Workflow「${workflow.name}」？`)) return;

  error.value = '';
  storageStatus.value = '';

  try {
    storageBusy.value = true;
    await invoke('workflow_delete', { workflowId: workflow.id });
    selectedWorkflowId.value = '';
    await refreshWorkflows();
    storageStatus.value = `已删除 ${workflow.name}`;
  } catch (value) {
    error.value = errorText(value);
  } finally {
    storageBusy.value = false;
  }
}

async function runWorkflow() {
  if (running.value) return;

  error.value = '';
  execution.value = null;
  resultMode.value = 'current';

  try {
    const workflow = parseWorkflowDefinition(workflowText.value);
    const input = parseWorkflowInput(inputText.value);
    lastWorkflow.value = workflow;
    running.value = true;
    execution.value = await invoke<WorkflowExecution>('workflow_run', {
      workflow,
      input,
    });
  } catch (value) {
    error.value = errorText(value);
  } finally {
    running.value = false;
    void refreshRunHistory();
  }
}

onMounted(() => {
  void refreshWorkflows().catch((value) => {
    error.value = errorText(value);
  });
  void refreshRunHistory();
});
</script>

<template>
  <div class="workflow-view">
    <header class="workflow-header">
      <button class="back-btn" title="返回" @click="$emit('back')">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M19 12H5M12 19l-7-7 7-7" />
        </svg>
      </button>
      <div class="header-copy">
        <h2>Workflow</h2>
        <span>手动 · 顺序执行 · Fail-fast</span>
      </div>
      <button class="run-btn" :disabled="running" @click="runWorkflow">
        {{ running ? '运行中…' : '运行' }}
      </button>
    </header>

    <div class="workflow-body">
      <section class="editor-pane">
        <div class="library-bar">
          <select
            v-model="selectedWorkflowId"
            class="workflow-select"
            :disabled="storageBusy"
            aria-label="已保存 Workflow"
            @change="loadSelectedWorkflow"
          >
            <option value="">已保存 Workflow</option>
            <option
              v-for="workflow in savedOptions"
              :key="workflow.id"
              :value="workflow.id"
            >
              {{ workflow.name }}
            </option>
          </select>
          <button
            class="secondary-btn"
            :disabled="storageBusy"
            @click="saveWorkflowDefinition"
          >
            {{ storageBusy ? '处理中…' : '保存' }}
          </button>
          <button
            class="danger-btn"
            :disabled="storageBusy || !selectedWorkflowId"
            @click="deleteSelectedWorkflow"
          >
            删除
          </button>
        </div>

        <div v-if="storageStatus" class="storage-status">
          {{ storageStatus }}
        </div>

        <div class="pane-title workflow-title">
          <strong>Workflow JSON</strong>
          <span>Tool / AI steps</span>
        </div>
        <textarea
          v-model="workflowText"
          class="workflow-editor"
          spellcheck="false"
          aria-label="Workflow JSON"
        />

        <div class="pane-title input-title">
          <strong>Input JSON</strong>
          <span>可留空，等价于 null</span>
        </div>
        <textarea
          v-model="inputText"
          class="input-editor"
          spellcheck="false"
          aria-label="Workflow Input JSON"
        />

        <div class="binding-hint">
          <span>数据引用</span>
          <code>${input}</code>
          <code>${previous}</code>
          <code>${steps.stepId}</code>
        </div>
      </section>

      <section class="result-pane">
        <div class="result-header">
          <div class="result-tabs" role="tablist" aria-label="Workflow 结果视图">
            <button
              :class="['result-tab', { active: resultMode === 'current' }]"
              @click="resultMode = 'current'"
            >
              本次执行
            </button>
            <button
              :class="['result-tab', { active: resultMode === 'history' }]"
              @click="resultMode = 'history'"
            >
              最近运行
              <span v-if="runHistory.length" class="tab-count">{{ runHistory.length }}</span>
            </button>
          </div>
          <span
            v-if="resultMode === 'current' && execution"
            :class="['status-text', execution.status]"
          >
            {{ execution.status === 'completed' ? 'Completed' : 'Failed' }}
          </span>
          <span v-else-if="resultMode === 'history'" class="audit-hint">仅审计元数据</span>
        </div>

        <template v-if="resultMode === 'current'">
          <div v-if="error" class="command-error">
            {{ error }}
          </div>

          <div v-else-if="!execution" class="empty-state">
            <div class="empty-icon">▶</div>
            <strong>运行一个 Workflow</strong>
            <span>默认示例会读取剪贴板，并把结果传给系统通知。</span>
          </div>

          <div v-else class="execution-content">
            <div class="timeline">
              <article
                v-for="(step, index) in execution.steps"
                :key="`${step.stepId}-${index}`"
                class="step-card"
              >
                <div class="step-line">
                  <div :class="['step-dot', step.status]"></div>
                  <div class="step-heading">
                    <strong>{{ workflowStepLabel(lastWorkflow, step.stepId) }}</strong>
                    <span>#{{ index + 1 }}</span>
                  </div>
                  <span :class="['step-status', step.status]">
                    {{ step.status === 'completed' ? '完成' : '失败' }}
                  </span>
                </div>

                <pre v-if="step.error" class="step-detail error-detail">{{ step.error }}</pre>
                <pre v-else-if="step.output !== undefined" class="step-detail">{{ formatWorkflowValue(step.output) }}</pre>
              </article>
            </div>

            <div v-if="execution.error" class="final-card failed">
              <span>Workflow error</span>
              <pre>{{ execution.error }}</pre>
            </div>
            <div v-else-if="execution.output !== undefined" class="final-card completed">
              <span>Final output</span>
              <pre>{{ formatWorkflowValue(execution.output) }}</pre>
            </div>
          </div>
        </template>

        <div v-else class="history-content">
          <div v-if="historyError" class="command-error history-error">
            {{ historyError }}
            <button class="inline-retry" :disabled="historyBusy" @click="refreshRunHistory">
              重试
            </button>
          </div>

          <div v-else-if="historyBusy && runHistory.length === 0" class="history-empty">
            正在读取运行记录…
          </div>

          <div v-else-if="runHistory.length === 0" class="history-empty">
            还没有运行记录。执行一次 Workflow 后，这里会显示审计信息。
          </div>

          <div v-else class="history-list">
            <article v-for="run in runHistory" :key="run.id" class="run-card">
              <div class="run-heading">
                <div class="run-title">
                  <span :class="['run-dot', run.status]"></span>
                  <strong>{{ run.workflowName || run.workflowId }}</strong>
                </div>
                <span class="run-duration">{{ formatWorkflowRunDuration(run.durationMs) }}</span>
              </div>

              <div class="run-meta">
                <span>{{ formatRunStartedAt(run.startedAt) }}</span>
                <span>{{ run.steps.length }} steps</span>
                <span :class="['run-status', run.status]">
                  {{ run.status === 'completed' ? '完成' : '失败' }}
                </span>
              </div>

              <div v-if="run.status === 'failed'" class="run-failure">
                <code>{{ workflowRunFailedStep(run)?.stepId || 'workflow' }}</code>
                <span>{{ workflowRunErrorText(run) }}</span>
              </div>
            </article>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.workflow-view {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  color: var(--text-primary);
  background: var(--bg-primary);
}

.workflow-header {
  height: 58px;
  flex: 0 0 58px;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 16px;
  border-bottom: 1px solid var(--border-color);
}

.back-btn {
  width: 34px;
  height: 34px;
  display: grid;
  place-items: center;
  border: 0;
  border-radius: 8px;
  color: var(--text-secondary);
  background: transparent;
  cursor: pointer;
}

.back-btn:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.header-copy {
  min-width: 0;
  flex: 1;
  display: flex;
  align-items: baseline;
  gap: 10px;
}

.header-copy h2 {
  font-size: 17px;
  font-weight: 650;
}

.header-copy span {
  font-size: 12px;
  color: var(--text-tertiary);
}

.run-btn {
  height: 32px;
  padding: 0 18px;
  border: 0;
  border-radius: 8px;
  color: white;
  background: var(--accent-color);
  font-weight: 600;
  cursor: pointer;
}

.run-btn:disabled {
  cursor: default;
  opacity: 0.55;
}

.workflow-body {
  min-height: 0;
  flex: 1;
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
}

.editor-pane,
.result-pane {
  min-width: 0;
  min-height: 0;
  padding: 14px;
}

.editor-pane {
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border-color);
}

.result-pane {
  display: flex;
  flex-direction: column;
}

.library-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  min-height: 32px;
}

.workflow-select {
  min-width: 0;
  flex: 1;
  height: 30px;
  padding: 0 8px;
  border: 1px solid var(--border-color);
  border-radius: 7px;
  outline: none;
  color: var(--text-primary);
  background: var(--bg-secondary);
  font-size: 11px;
}

.workflow-select:focus {
  border-color: var(--accent-color);
}

.secondary-btn,
.danger-btn {
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--border-color);
  border-radius: 7px;
  background: var(--bg-secondary);
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
}

.secondary-btn {
  color: var(--text-primary);
}

.danger-btn {
  color: var(--danger-color);
}

.secondary-btn:hover:not(:disabled),
.danger-btn:hover:not(:disabled) {
  background: var(--bg-hover);
}

.secondary-btn:disabled,
.danger-btn:disabled,
.workflow-select:disabled {
  cursor: default;
  opacity: 0.55;
}

.storage-status {
  margin-top: 5px;
  color: var(--text-tertiary);
  font-size: 10px;
  line-height: 1.4;
}

.pane-title,
.result-header {
  min-height: 24px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 7px;
}

.pane-title strong {
  font-size: 13px;
  font-weight: 650;
}

.pane-title span,
.audit-hint {
  font-size: 11px;
  color: var(--text-tertiary);
}

.workflow-title {
  margin-top: 7px;
}

.input-title {
  margin-top: 10px;
}

.workflow-editor,
.input-editor {
  width: 100%;
  resize: none;
  border: 1px solid var(--border-color);
  border-radius: 8px;
  outline: none;
  color: var(--text-primary);
  background: var(--bg-secondary);
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 11px;
  line-height: 1.55;
  user-select: text;
  -webkit-user-select: text;
}

.workflow-editor:focus,
.input-editor:focus {
  border-color: var(--accent-color);
}

.workflow-editor {
  min-height: 0;
  flex: 1;
  padding: 10px;
}

.input-editor {
  height: 62px;
  flex: 0 0 62px;
  padding: 8px 10px;
}

.binding-hint {
  min-height: 27px;
  display: flex;
  align-items: center;
  gap: 5px;
  margin-top: 8px;
  color: var(--text-tertiary);
  font-size: 10px;
  overflow: hidden;
}

.binding-hint code {
  padding: 2px 5px;
  border-radius: 4px;
  color: var(--text-secondary);
  background: var(--bg-secondary);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  white-space: nowrap;
  user-select: text;
}

.result-tabs {
  display: flex;
  align-items: center;
  gap: 3px;
  padding: 2px;
  border-radius: 7px;
  background: var(--bg-secondary);
}

.result-tab {
  height: 25px;
  padding: 0 8px;
  border: 0;
  border-radius: 5px;
  color: var(--text-tertiary);
  background: transparent;
  font-size: 10px;
  font-weight: 600;
  cursor: pointer;
}

.result-tab.active {
  color: var(--text-primary);
  background: var(--bg-primary);
}

.tab-count {
  margin-left: 3px;
  color: var(--text-tertiary);
  font-size: 9px;
}

.status-text.completed,
.step-status.completed,
.run-status.completed {
  color: #34c759;
}

.status-text.failed,
.step-status.failed,
.run-status.failed {
  color: var(--danger-color);
}

.empty-state {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 30px;
  text-align: center;
  color: var(--text-secondary);
}

.empty-state strong {
  color: var(--text-primary);
  font-size: 13px;
}

.empty-state span {
  max-width: 270px;
  color: var(--text-tertiary);
  font-size: 11px;
}

.empty-icon {
  width: 36px;
  height: 36px;
  display: grid;
  place-items: center;
  margin-bottom: 4px;
  border-radius: 50%;
  color: var(--accent-color);
  background: var(--bg-secondary);
  font-size: 13px;
}

.command-error {
  padding: 10px 12px;
  border: 1px solid color-mix(in srgb, var(--danger-color) 30%, transparent);
  border-radius: 8px;
  color: var(--danger-color);
  background: color-mix(in srgb, var(--danger-color) 8%, transparent);
  font-size: 12px;
  user-select: text;
}

.execution-content,
.history-content {
  min-height: 0;
  flex: 1;
  overflow-y: auto;
  padding-right: 2px;
}

.timeline,
.history-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.step-card,
.final-card,
.run-card {
  border: 1px solid var(--border-color);
  border-radius: 8px;
  background: var(--bg-secondary);
}

.step-card,
.run-card {
  padding: 9px 10px;
}

.step-line,
.run-heading {
  display: flex;
  align-items: center;
  gap: 8px;
}

.step-dot,
.run-dot {
  width: 8px;
  height: 8px;
  flex: 0 0 8px;
  border-radius: 50%;
}

.step-dot.completed,
.run-dot.completed {
  background: #34c759;
}

.step-dot.failed,
.run-dot.failed {
  background: var(--danger-color);
}

.step-heading {
  min-width: 0;
  flex: 1;
  display: flex;
  align-items: baseline;
  gap: 6px;
}

.step-heading strong {
  overflow: hidden;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 11px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.step-heading span,
.step-status {
  color: var(--text-tertiary);
  font-size: 10px;
}

.step-detail,
.final-card pre {
  margin-top: 8px;
  padding: 7px 8px;
  overflow: auto;
  border-radius: 6px;
  color: var(--text-secondary);
  background: var(--bg-primary);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 10px;
  line-height: 1.45;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  user-select: text;
  -webkit-user-select: text;
}

.error-detail,
.final-card.failed pre {
  color: var(--danger-color);
}

.final-card {
  margin-top: 10px;
  padding: 9px 10px;
}

.final-card > span {
  font-size: 10px;
  font-weight: 600;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.history-error {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.inline-retry {
  border: 0;
  color: inherit;
  background: transparent;
  font-size: 10px;
  font-weight: 600;
  cursor: pointer;
}

.history-empty {
  padding: 26px 16px;
  color: var(--text-tertiary);
  font-size: 11px;
  line-height: 1.6;
  text-align: center;
}

.run-title {
  min-width: 0;
  flex: 1;
  display: flex;
  align-items: center;
  gap: 7px;
}

.run-title strong {
  overflow: hidden;
  font-size: 11px;
  font-weight: 650;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.run-duration {
  color: var(--text-tertiary);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 10px;
}

.run-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 6px;
  color: var(--text-tertiary);
  font-size: 10px;
}

.run-status {
  margin-left: auto;
}

.run-failure {
  display: flex;
  align-items: flex-start;
  gap: 7px;
  margin-top: 7px;
  padding: 7px 8px;
  border-radius: 6px;
  color: var(--danger-color);
  background: color-mix(in srgb, var(--danger-color) 7%, transparent);
  font-size: 10px;
  line-height: 1.4;
  overflow-wrap: anywhere;
  user-select: text;
}

.run-failure code {
  flex: 0 0 auto;
  padding: 1px 4px;
  border-radius: 4px;
  color: var(--danger-color);
  background: color-mix(in srgb, var(--danger-color) 10%, transparent);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
</style>
