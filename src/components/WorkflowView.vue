<script setup lang="ts">
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import {
  DEFAULT_WORKFLOW_INPUT,
  DEFAULT_WORKFLOW_TEXT,
  formatWorkflowValue,
  parseWorkflowDefinition,
  parseWorkflowInput,
  workflowStepLabel,
  type WorkflowDefinition,
  type WorkflowExecution,
} from '../workflow/model';

defineEmits<{ back: [] }>();

const workflowText = ref(DEFAULT_WORKFLOW_TEXT);
const inputText = ref(DEFAULT_WORKFLOW_INPUT);
const running = ref(false);
const error = ref('');
const execution = ref<WorkflowExecution | null>(null);
const lastWorkflow = ref<WorkflowDefinition | null>(null);

function errorText(value: unknown): string {
  if (value instanceof Error) return value.message;
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

async function runWorkflow() {
  if (running.value) return;

  error.value = '';
  execution.value = null;

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
  }
}
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
        <div class="pane-title">
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
        <div class="pane-title">
          <strong>Execution</strong>
          <span v-if="execution" :class="['status-text', execution.status]">
            {{ execution.status === 'completed' ? 'Completed' : 'Failed' }}
          </span>
          <span v-else>运行后显示 step timeline</span>
        </div>

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

.pane-title {
  min-height: 24px;
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 7px;
}

.pane-title strong {
  font-size: 13px;
  font-weight: 650;
}

.pane-title span {
  font-size: 11px;
  color: var(--text-tertiary);
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

.status-text.completed,
.step-status.completed {
  color: #34c759;
}

.status-text.failed,
.step-status.failed {
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

.execution-content {
  min-height: 0;
  flex: 1;
  overflow-y: auto;
  padding-right: 2px;
}

.timeline {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.step-card,
.final-card {
  border: 1px solid var(--border-color);
  border-radius: 8px;
  background: var(--bg-secondary);
}

.step-card {
  padding: 9px 10px;
}

.step-line {
  display: flex;
  align-items: center;
  gap: 8px;
}

.step-dot {
  width: 8px;
  height: 8px;
  flex: 0 0 8px;
  border-radius: 50%;
}

.step-dot.completed {
  background: #34c759;
}

.step-dot.failed {
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
</style>
