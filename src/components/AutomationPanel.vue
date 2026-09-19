<script setup lang="ts">
import { computed, onMounted, ref, shallowRef, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import {
  automationRetryLabel,
  automationTriggerLabel,
  buildDailyAutomation,
  buildIntervalAutomation,
  buildRetryPolicy,
  formatAutomationNextRun,
  formatDailyTime,
  parseDailyTime,
  type AutomationPermissionPreflight,
  type AutomationRecord,
  type PermissionPreflightStatus,
} from '../automation/model';
import type { WorkflowOption } from '../workflow/model';

const props = defineProps<{
  workflows: WorkflowOption[];
  permissionRevision: number;
}>();

const records = shallowRef<AutomationRecord[]>([]);
const selectedAutomationId = ref('');
const id = ref('');
const workflowId = ref('');
const triggerType = ref<'interval' | 'daily'>('interval');
const everyMinutes = ref(15);
const dailyTime = ref('09:00');
const retryEnabled = ref(false);
const maxRetries = ref(2);
const backoffMinutes = ref(1);
const enabled = ref(true);
const busy = ref(false);
const status = ref('');
const error = ref('');
const preflight = shallowRef<AutomationPermissionPreflight | null>(null);
const preflightBusy = ref(false);
const preflightError = ref('');

// ---- 通知权限引导（#49） ----
// tauri-plugin-notification 桌面端权限 API 是 Granted stub；
// macOS 在首条通知送达后才注册应用，因此以“可观察的测试通知”为验收动作。
interface NotificationPermissionStatus {
  pluginState: string;
  primed: boolean;
  settingsUrl: string | null;
}

const notifyStatus = shallowRef<NotificationPermissionStatus | null>(null);
const notifyBusy = ref(false);
const notifyMessage = ref('');

const notifyStatusText = computed(() => {
  const status = notifyStatus.value;
  if (!status) return '未知';
  if (!status.primed) return '尚未发送测试通知（macOS 首条通知后才会注册 Volo）';
  return '已发送测试通知 · 看不到请检查系统通知设置';
});

async function refreshNotifyStatus() {
  try {
    notifyStatus.value = await invoke<NotificationPermissionStatus>(
      'notification_permission_status',
    );
    notifyMessage.value = '';
  } catch (value) {
    notifyMessage.value = errorText(value);
  }
}

async function requestNotifyPermission() {
  notifyBusy.value = true;
  notifyMessage.value = '';
  try {
    notifyStatus.value = await invoke<NotificationPermissionStatus>(
      'notification_request_permission',
    );
    notifyMessage.value = '已发送测试通知，请看屏幕右上角。';
  } catch (value) {
    notifyMessage.value = errorText(value);
  } finally {
    notifyBusy.value = false;
  }
}

async function openNotifySettings() {
  try {
    await invoke('notification_open_settings');
  } catch (value) {
    notifyMessage.value = errorText(value);
  }
}

const selectedRecord = computed(
  () => records.value.find((record) => record.id === selectedAutomationId.value) ?? null,
);

const workflowName = computed(() => {
  const workflow = props.workflows.find((item) => item.id === workflowId.value);
  return workflow?.name || workflowId.value || '未选择';
});

function errorText(value: unknown): string {
  if (value instanceof Error) return value.message;
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function preflightStatusLabel(status: PermissionPreflightStatus): string {
  switch (status) {
    case 'ready':
      return 'Ready';
    case 'missing':
      return 'Missing';
    case 'runtime':
      return 'Runtime';
    case 'invalid':
      return 'Invalid';
    case 'unsupported':
      return 'Unsupported';
  }
}

const preflightSummary = computed(() => {
  const value = preflight.value;
  if (!value) return '尚未检查';
  if (value.blockerCount > 0) return `${value.blockerCount} 项阻断`;
  if (value.missingCount > 0) return `缺少 ${value.missingCount} 项 Always 授权`;
  if (value.runtimeCount > 0) return `已知权限就绪 · ${value.runtimeCount} 项运行时复核`;
  return '全部权限就绪';
});

async function refreshPreflight() {
  if (!workflowId.value) {
    preflight.value = null;
    preflightError.value = '';
    return;
  }

  preflightBusy.value = true;
  preflightError.value = '';
  try {
    preflight.value = await invoke<AutomationPermissionPreflight>(
      'automation_permission_preflight',
      { workflowId: workflowId.value },
    );
  } catch (value) {
    preflight.value = null;
    preflightError.value = errorText(value);
  } finally {
    preflightBusy.value = false;
  }
}

async function refreshAutomations(selectId?: string) {
  const items = await invoke<AutomationRecord[]>('automation_list');
  records.value = items;

  if (selectId && items.some((item) => item.id === selectId)) {
    selectedAutomationId.value = selectId;
    loadSelectedAutomation();
  } else if (
    selectedAutomationId.value &&
    !items.some((item) => item.id === selectedAutomationId.value)
  ) {
    selectedAutomationId.value = '';
  }
}

function loadSelectedAutomation() {
  const record = selectedRecord.value;
  if (!record) return;

  id.value = record.id;
  workflowId.value = record.workflowId;
  triggerType.value = record.trigger.type;
  if (record.trigger.type === 'interval') {
    everyMinutes.value = record.trigger.everyMinutes;
  } else {
    dailyTime.value = formatDailyTime(record.trigger.hour, record.trigger.minute);
  }
  retryEnabled.value = !!record.retryPolicy;
  maxRetries.value = record.retryPolicy?.maxRetries ?? 2;
  backoffMinutes.value = record.retryPolicy?.backoffMinutes ?? 1;
  enabled.value = record.enabled;
  error.value = '';
  status.value = `已加载 ${record.id}`;
}

function newAutomation() {
  selectedAutomationId.value = '';
  id.value = '';
  workflowId.value = props.workflows[0]?.id ?? '';
  triggerType.value = 'interval';
  everyMinutes.value = 15;
  dailyTime.value = '09:00';
  retryEnabled.value = false;
  maxRetries.value = 2;
  backoffMinutes.value = 1;
  enabled.value = true;
  status.value = '';
  error.value = '';
}

async function saveAutomation() {
  if (busy.value) return;

  status.value = '';
  error.value = '';
  try {
    const retryPolicy = retryEnabled.value
      ? buildRetryPolicy(Number(maxRetries.value), Number(backoffMinutes.value))
      : undefined;
    const automation = triggerType.value === 'daily'
      ? (() => {
          const { hour, minute } = parseDailyTime(dailyTime.value);
          return buildDailyAutomation(
            id.value,
            workflowId.value,
            hour,
            minute,
            enabled.value,
            retryPolicy,
          );
        })()
      : buildIntervalAutomation(
          id.value,
          workflowId.value,
          Number(everyMinutes.value),
          enabled.value,
          retryPolicy,
        );
    const previousId = selectedAutomationId.value;
    busy.value = true;
    const saved = await invoke<AutomationRecord>('automation_save', { automation });
    await refreshAutomations(saved.id);
    status.value =
      previousId && previousId !== saved.id
        ? `已另存为 ${saved.id}`
        : `已保存 ${saved.id}`;
  } catch (value) {
    error.value = errorText(value);
  } finally {
    busy.value = false;
  }
}

async function deleteAutomation() {
  const record = selectedRecord.value;
  if (busy.value || !record) return;
  if (!window.confirm(`删除 Automation「${record.id}」？`)) return;

  status.value = '';
  error.value = '';
  try {
    busy.value = true;
    await invoke('automation_delete', { automationId: record.id });
    newAutomation();
    await refreshAutomations();
    status.value = `已删除 ${record.id}`;
  } catch (value) {
    error.value = errorText(value);
  } finally {
    busy.value = false;
  }
}

async function manualRefresh() {
  if (busy.value) return;
  error.value = '';
  try {
    busy.value = true;
    await Promise.all([
      refreshAutomations(selectedAutomationId.value || undefined),
      refreshPreflight(),
    ]);
    status.value = '已刷新调度状态与权限预检';
  } catch (value) {
    error.value = errorText(value);
  } finally {
    busy.value = false;
  }
}

watch(
  () => props.workflows,
  (workflows) => {
    if (!workflowId.value && workflows.length) {
      workflowId.value = workflows[0].id;
    }
  },
  { immediate: true },
);

watch(
  () => [workflowId.value, props.permissionRevision] as const,
  () => {
    void refreshPreflight();
  },
  { immediate: true },
);


onMounted(() => {
  void refreshAutomations().catch((value) => {
    error.value = errorText(value);
  });
  void refreshNotifyStatus();
});
</script>

<template>
  <div class="automation-panel">
    <section class="automation-editor">
      <div class="panel-title">
        <div>
          <strong>Automation</strong>
          <span>Interval / Daily scheduler</span>
        </div>
        <button class="ghost-btn" :disabled="busy" @click="manualRefresh">刷新</button>
      </div>

      <section class="notify-perm-card">
        <div class="notify-perm-row">
          <span class="notify-perm-dot" :class="notifyStatus?.primed ? 'ok' : 'warn'" />
          <div class="notify-perm-copy">
            <strong>通知权限</strong>
            <span>{{ notifyStatusText }}</span>
            <span v-if="notifyMessage" class="notify-perm-msg">{{ notifyMessage }}</span>
          </div>
          <button class="ghost-btn" :disabled="notifyBusy" @click="requestNotifyPermission">
            发送测试通知
          </button>
          <button
            v-if="notifyStatus?.settingsUrl"
            class="ghost-btn"
            :disabled="notifyBusy"
            @click="openNotifySettings"
          >
            打开通知设置
          </button>
        </div>
      </section>

      <div class="automation-toolbar">
        <select
          v-model="selectedAutomationId"
          class="field-control grow"
          :disabled="busy"
          aria-label="已保存 Automation"
          @change="loadSelectedAutomation"
        >
          <option value="">新建 Automation</option>
          <option v-for="record in records" :key="record.id" :value="record.id">
            {{ record.id }}{{ record.enabled ? '' : ' · 已停用' }}
          </option>
        </select>
        <button class="ghost-btn" :disabled="busy" @click="newAutomation">新建</button>
        <button class="danger-btn" :disabled="busy || !selectedRecord" @click="deleteAutomation">
          删除
        </button>
      </div>

      <label class="field-block">
        <span>Automation ID</span>
        <input
          v-model="id"
          class="field-control"
          :disabled="busy"
          placeholder="例如 daily-summary"
          spellcheck="false"
        />
      </label>

      <label class="field-block">
        <span>Workflow</span>
        <select v-model="workflowId" class="field-control" :disabled="busy || !workflows.length">
          <option value="" disabled>请选择已保存 Workflow</option>
          <option v-for="workflow in workflows" :key="workflow.id" :value="workflow.id">
            {{ workflow.name }} · {{ workflow.id }}
          </option>
        </select>
      </label>

      <section class="preflight-card">
        <div class="preflight-heading">
          <div>
            <strong>后台权限预检</strong>
            <span>{{ preflightBusy ? '检查中…' : preflightSummary }}</span>
          </div>
          <button
            class="ghost-btn"
            :disabled="preflightBusy || !workflowId"
            @click="refreshPreflight"
          >
            检查
          </button>
        </div>

        <div
          v-if="preflight"
          class="preflight-summary"
          :class="{
            ready: preflight.fullyVerified,
            warning: preflight.ready && !preflight.fullyVerified,
            danger: !preflight.ready,
          }"
        >
          <span>{{ preflight.readyCount }} Ready</span>
          <span v-if="preflight.missingCount">{{ preflight.missingCount }} Missing</span>
          <span v-if="preflight.runtimeCount">{{ preflight.runtimeCount }} Runtime</span>
          <span v-if="preflight.blockerCount">{{ preflight.blockerCount }} Blocked</span>
        </div>

        <div v-if="preflight?.requirements.length" class="preflight-list">
          <div
            v-for="item in preflight.requirements"
            :key="item.stepId + ':' + item.toolName + ':' + item.capability + ':' + (item.resource ?? '')"
            class="preflight-item"
          >
            <span class="preflight-status" :class="'status-' + item.status">
              {{ preflightStatusLabel(item.status) }}
            </span>
            <div class="preflight-copy">
              <strong>{{ item.stepId }} · {{ item.capability }}</strong>
              <code v-if="item.resource">{{ item.resource }}</code>
              <span>{{ item.note || item.description }}</span>
            </div>
          </div>
        </div>

        <div v-else-if="preflight && !preflightBusy" class="preflight-empty">
          此 Workflow 没有需要持久授权的后台能力。
        </div>
        <div v-if="preflightError" class="error-message">{{ preflightError }}</div>
      </section>

      <label class="field-block">
        <span>调度方式</span>
        <select v-model="triggerType" class="field-control" :disabled="busy">
          <option value="interval">固定间隔</option>
          <option value="daily">每天固定时间</option>
        </select>
      </label>

      <label v-if="triggerType === 'interval'" class="field-block">
        <span>运行间隔</span>
        <div class="interval-row">
          <input
            v-model.number="everyMinutes"
            class="field-control"
            type="number"
            min="1"
            max="525600"
            step="1"
            :disabled="busy"
          />
          <span>分钟</span>
        </div>
      </label>

      <label v-else class="field-block">
        <span>每天运行时间</span>
        <input
          v-model="dailyTime"
          class="field-control daily-time"
          type="time"
          :disabled="busy"
        />
        <span class="field-hint">使用当前系统本地时区；nextRunAt 仍以 UTC 持久化。</span>
      </label>

      <div class="retry-block">
        <label class="enabled-row retry-toggle">
          <input v-model="retryEnabled" type="checkbox" :disabled="busy" />
          <div>
            <strong>失败后自动重试</strong>
            <span>仅对当前 occurrence 重试；不会跨过下一次正常调度时间。</span>
          </div>
        </label>

        <div v-if="retryEnabled" class="retry-config">
          <label class="field-block">
            <span>最大重试次数</span>
            <input
              v-model.number="maxRetries"
              class="field-control compact-number"
              type="number"
              min="1"
              max="10"
              step="1"
              :disabled="busy"
            />
          </label>
          <label class="field-block">
            <span>重试间隔</span>
            <div class="interval-row">
              <input
                v-model.number="backoffMinutes"
                class="field-control compact-number"
                type="number"
                min="1"
                max="1440"
                step="1"
                :disabled="busy"
              />
              <span>分钟</span>
            </div>
          </label>
        </div>
      </div>

      <label class="enabled-row">
        <input v-model="enabled" type="checkbox" :disabled="busy" />
        <div>
          <strong>启用后台运行</strong>
          <span>停用后 nextRunAt 会被清空；再次启用会从保存时刻重新计算下一次运行。</span>
        </div>
      </label>

      <button class="save-btn" :disabled="busy || !workflows.length" @click="saveAutomation">
        {{ busy ? '处理中…' : '保存 Automation' }}
      </button>

      <div v-if="status" class="status-message">{{ status }}</div>
      <div v-if="error" class="error-message">{{ error }}</div>
      <div v-if="!workflows.length" class="error-message">
        请先保存至少一个 Workflow，再创建 Automation。
      </div>
    </section>

    <section class="automation-overview">
      <div class="panel-title">
        <div>
          <strong>Scheduler 状态</strong>
          <span>{{ records.length }} 个 Automation</span>
        </div>
      </div>

      <article v-if="selectedRecord" class="scheduler-card">
        <div class="scheduler-heading">
          <span :class="['scheduler-dot', { enabled: selectedRecord.enabled }]"></span>
          <strong>{{ selectedRecord.id }}</strong>
          <span :class="['state-pill', { enabled: selectedRecord.enabled }]">
            {{ selectedRecord.enabled ? 'Enabled' : 'Disabled' }}
          </span>
        </div>
        <dl class="scheduler-meta">
          <div>
            <dt>Workflow</dt>
            <dd>{{ workflowName }}</dd>
          </div>
          <div>
            <dt>Cadence</dt>
            <dd>{{ automationTriggerLabel(selectedRecord) }}</dd>
          </div>
          <div>
            <dt>Next run</dt>
            <dd>{{ selectedRecord.enabled ? formatAutomationNextRun(selectedRecord.nextRunAt) : '已停用' }}</dd>
          </div>
          <div>
            <dt>Retry</dt>
            <dd>{{ automationRetryLabel(selectedRecord) }}</dd>
          </div>
        </dl>
      </article>

      <div v-else class="scheduler-empty">
        选择一个已保存 Automation，可以查看当前 nextRunAt。Scheduler 每 15 秒检查一次到期任务。
      </div>

      <div class="policy-card">
        <strong>后台执行规则</strong>
        <ul>
          <li>错过的 interval / daily 周期不补跑，只执行恢复后的一个到期周期。</li>
          <li>Daily 使用当前系统本地时区；遇到 DST 跳时会顺延到第一个有效分钟。</li>
          <li>Medium / High / Critical 能力必须提前授予 Workflow <code>Always</code> 权限。</li>
          <li>内置 Tool、MCP Tool、AI Step 可后台执行。</li>
          <li>Plugin Tool 可通过 Headless Runtime 后台执行；clipboard.read、非交互整屏 screen.capture，以及 fs.read/readBinary/exists/list 已支持，并要求对应 Workflow 的精确资源 Always 授权；写文件和其他未接入的高风险宿主 API 仍会明确拒绝。</li>
          <li>未配置 Retry 时失败不会重试；启用后按固定间隔重试，且不会跨过下一次正常调度。</li>
          <li>每次执行结果仍会写入 Workflow「最近运行」，便于审计失败与重试过程。</li>
        </ul>
      </div>

      <div v-if="records.length" class="automation-list">
        <button
          v-for="record in records"
          :key="record.id"
          class="automation-card"
          :class="{ active: record.id === selectedAutomationId }"
          @click="selectedAutomationId = record.id; loadSelectedAutomation()"
        >
          <div class="automation-card-heading">
            <strong>{{ record.id }}</strong>
            <span>{{ automationTriggerLabel(record) }}</span>
          </div>
          <div class="automation-card-meta">
            <span>{{ record.workflowId }}</span>
            <span>{{ record.enabled ? formatAutomationNextRun(record.nextRunAt) : '已停用' }}</span>
          </div>
        </button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.automation-panel {
  min-height: 0;
  flex: 1;
  display: grid;
  grid-template-columns: minmax(0, 0.92fr) minmax(0, 1.08fr);
}

.automation-editor,
.automation-overview {
  min-width: 0;
  min-height: 0;
  padding: 14px;
  overflow-y: auto;
}

.automation-editor {
  border-right: 1px solid var(--border-color);
}

.panel-title {
  min-height: 30px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 10px;
}

.panel-title > div {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.panel-title strong {
  font-size: 13px;
  font-weight: 650;
}

.panel-title span {
  color: var(--text-tertiary);
  font-size: 10px;
}

.automation-toolbar,
.interval-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.automation-toolbar {
  margin-bottom: 12px;
}

.grow {
  min-width: 0;
  flex: 1;
}

.field-block {
  display: block;
  margin-top: 10px;
}

.field-block > span {
  display: block;
  margin-bottom: 5px;
  color: var(--text-secondary);
  font-size: 10px;
  font-weight: 600;
}

.field-block > .field-hint {
  margin-top: 5px;
  margin-bottom: 0;
  color: var(--text-tertiary);
  font-weight: 400;
  line-height: 1.4;
}

.field-control {
  width: 100%;
  height: 32px;
  padding: 0 9px;
  border: 1px solid var(--border-color);
  border-radius: 7px;
  outline: none;
  color: var(--text-primary);
  background: var(--bg-secondary);
  font-size: 11px;
}

.field-control:focus {
  border-color: var(--accent-color);
}

.interval-row .field-control,
.daily-time {
  width: 110px;
}

.interval-row > span {
  color: var(--text-tertiary);
  font-size: 11px;
}

.retry-block {
  margin-top: 14px;
}

.retry-block .enabled-row {
  margin-top: 0;
}

.retry-config {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 8px;
  margin-top: 8px;
}

.retry-config .field-block {
  margin-top: 0;
}

.compact-number {
  width: 110px;
}

.enabled-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin-top: 14px;
  padding: 9px 10px;
  border: 1px solid var(--border-color);
  border-radius: 8px;
  background: var(--bg-secondary);
}

.enabled-row input {
  margin-top: 2px;
}

.enabled-row div {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.enabled-row strong {
  font-size: 11px;
  font-weight: 650;
}

.enabled-row span {
  color: var(--text-tertiary);
  font-size: 10px;
  line-height: 1.45;
}

.ghost-btn,
.danger-btn,
.save-btn {
  border-radius: 7px;
  font-size: 10px;
  font-weight: 600;
  cursor: pointer;
}

.ghost-btn,
.danger-btn {
  height: 30px;
  padding: 0 9px;
  border: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.ghost-btn {
  color: var(--text-primary);
}

.danger-btn {
  color: var(--danger-color);
}

.save-btn {
  width: 100%;
  height: 34px;
  margin-top: 14px;
  border: 0;
  color: white;
  background: var(--accent-color);
}

.ghost-btn:hover:not(:disabled),
.danger-btn:hover:not(:disabled) {
  background: var(--bg-hover);
}

.ghost-btn:disabled,
.danger-btn:disabled,
.save-btn:disabled,
.field-control:disabled {
  cursor: default;
  opacity: 0.55;
}

.status-message,
.error-message {
  margin-top: 8px;
  font-size: 10px;
  line-height: 1.45;
}

.status-message {
  color: var(--text-tertiary);
}

.error-message {
  color: var(--danger-color);
}

.scheduler-card,
.policy-card,
.automation-card,
.preflight-card {
  border: 1px solid var(--border-color);
  border-radius: 9px;
  background: var(--bg-secondary);
}

.scheduler-card,
.policy-card,
.preflight-card {
  padding: 11px 12px;
}

.preflight-card {
  margin-top: 10px;
}

.notify-perm-card {
  margin-top: 10px;
  padding: 9px 12px;
  border: 1px solid var(--border-color);
  border-radius: 9px;
  background: var(--bg-secondary);
}

.notify-perm-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.notify-perm-dot {
  flex: none;
  width: 7px;
  height: 7px;
  border-radius: 50%;
}

.notify-perm-dot.ok {
  background: var(--success-color, #34c759);
}

.notify-perm-dot.warn {
  background: var(--warning-color, #ff9f0a);
}

.notify-perm-copy {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
  flex: 1;
}

.notify-perm-copy strong {
  font-size: 11px;
  color: var(--text-primary);
}

.notify-perm-copy span {
  font-size: 10px;
  color: var(--text-secondary);
}

.notify-perm-msg {
  color: var(--text-primary);
}

.preflight-heading,
.preflight-heading > div {
  display: flex;
  align-items: center;
  gap: 8px;
}

.preflight-heading {
  justify-content: space-between;
}

.preflight-heading > div {
  min-width: 0;
}

.preflight-heading strong {
  font-size: 11px;
}

.preflight-heading span,
.preflight-empty {
  color: var(--text-tertiary);
  font-size: 9px;
}

.preflight-summary {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 8px;
  font-size: 9px;
}

.preflight-summary.ready {
  color: #34c759;
}

.preflight-summary.warning {
  color: #ff9f0a;
}

.preflight-summary.danger {
  color: var(--danger-color);
}

.preflight-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 8px;
}

.preflight-item {
  display: flex;
  align-items: flex-start;
  gap: 7px;
  padding-top: 6px;
  border-top: 1px solid var(--border-color);
}

.preflight-status {
  flex: 0 0 auto;
  padding: 2px 5px;
  border-radius: 999px;
  font-size: 8px;
  font-weight: 650;
  background: var(--bg-primary);
}

.status-ready {
  color: #34c759;
}

.status-missing,
.status-invalid,
.status-unsupported {
  color: var(--danger-color);
}

.status-runtime {
  color: #ff9f0a;
}

.preflight-copy {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.preflight-copy strong,
.preflight-copy code,
.preflight-copy span {
  font-size: 9px;
  line-height: 1.4;
}

.preflight-copy strong {
  color: var(--text-secondary);
}

.preflight-copy code {
  overflow-wrap: anywhere;
  color: var(--text-tertiary);
}

.preflight-copy span,
.preflight-empty {
  color: var(--text-tertiary);
}

.preflight-empty {
  margin-top: 8px;
}

.scheduler-heading {
  display: flex;
  align-items: center;
  gap: 8px;
}

.scheduler-heading strong {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.scheduler-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-tertiary);
}

.scheduler-dot.enabled {
  background: #34c759;
}

.state-pill {
  padding: 2px 6px;
  border-radius: 999px;
  color: var(--text-tertiary);
  background: var(--bg-primary);
  font-size: 9px;
}

.state-pill.enabled {
  color: #34c759;
}

.scheduler-meta {
  display: grid;
  gap: 7px;
  margin: 10px 0 0;
}

.scheduler-meta > div {
  display: grid;
  grid-template-columns: 72px minmax(0, 1fr);
  gap: 8px;
}

.scheduler-meta dt,
.scheduler-meta dd {
  margin: 0;
  font-size: 10px;
}

.scheduler-meta dt {
  color: var(--text-tertiary);
}

.scheduler-meta dd {
  overflow: hidden;
  color: var(--text-secondary);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.scheduler-empty {
  padding: 24px 16px;
  border: 1px dashed var(--border-color);
  border-radius: 9px;
  color: var(--text-tertiary);
  font-size: 10px;
  line-height: 1.6;
  text-align: center;
}

.policy-card {
  margin-top: 10px;
}

.policy-card strong {
  font-size: 11px;
}

.policy-card ul {
  margin: 8px 0 0;
  padding-left: 17px;
  color: var(--text-tertiary);
  font-size: 10px;
  line-height: 1.55;
}

.policy-card code {
  color: var(--text-secondary);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}

.automation-list {
  display: flex;
  flex-direction: column;
  gap: 7px;
  margin-top: 10px;
}

.automation-card {
  width: 100%;
  padding: 9px 10px;
  color: inherit;
  text-align: left;
  cursor: pointer;
}

.automation-card:hover,
.automation-card.active {
  border-color: color-mix(in srgb, var(--accent-color) 45%, var(--border-color));
}

.automation-card-heading,
.automation-card-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.automation-card-heading strong {
  overflow: hidden;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 10px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.automation-card-heading span,
.automation-card-meta {
  color: var(--text-tertiary);
  font-size: 9px;
}

.automation-card-meta {
  margin-top: 5px;
}

.automation-card-meta span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
