<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { PermissionGrant } from '../api/rubick';
import type { WorkflowOption } from '../workflow/model';

const props = defineProps<{
  workflows: WorkflowOption[];
}>();

const emit = defineEmits<{
  grantsChanged: [];
}>();

const workflowId = ref('');
const capability = ref('clipboard.read');
const resource = ref('');
const grants = ref<PermissionGrant[]>([]);
const busy = ref(false);
const status = ref('');
const error = ref('');

const principal = computed(() => workflowId.value ? `workflow:${workflowId.value}` : '');
const resourceRequired = computed(() => capability.value.trim() === 'fs.read');
const workflowGrants = computed(() =>
  grants.value.filter((grant) => grant.pluginId === principal.value && grant.scope === 'always'),
);

function errorText(value: unknown): string {
  if (value instanceof Error) return value.message;
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

async function refreshGrants() {
  grants.value = await invoke<PermissionGrant[]>('permission_list_grants');
}

async function requestAlwaysGrant() {
  if (busy.value || !workflowId.value || !capability.value.trim()) return;

  status.value = '';
  error.value = '';
  try {
    busy.value = true;
    await invoke('permission_request_workflow_always', {
      workflowId: workflowId.value,
      capability: capability.value.trim(),
      resource: resource.value.trim() || null,
    });
    await refreshGrants();
    emit('grantsChanged');
    status.value = '已授予 Workflow 后台 Always 权限';
  } catch (value) {
    error.value = errorText(value);
  } finally {
    busy.value = false;
  }
}

async function revokeGrant(grant: PermissionGrant) {
  if (busy.value) return;
  try {
    busy.value = true;
    await invoke('permission_revoke', {
      pluginId: grant.pluginId,
      capability: grant.capability,
      resource: grant.resource ?? null,
    });
    await refreshGrants();
    emit('grantsChanged');
    status.value = '已撤销授权';
    error.value = '';
  } catch (value) {
    error.value = errorText(value);
  } finally {
    busy.value = false;
  }
}

watch(
  () => props.workflows,
  (items) => {
    if (!workflowId.value && items.length) {
      workflowId.value = items[0].id;
    }
  },
  { immediate: true },
);

onMounted(() => {
  void refreshGrants().catch((value) => {
    error.value = errorText(value);
  });
});
</script>

<template>
  <details class="workflow-permission-panel">
    <summary>后台权限</summary>
    <div class="permission-body">
      <div class="permission-row">
        <select v-model="workflowId" class="permission-input" :disabled="busy || !workflows.length">
          <option value="" disabled>选择 Workflow</option>
          <option v-for="workflow in workflows" :key="workflow.id" :value="workflow.id">
            {{ workflow.name }} · {{ workflow.id }}
          </option>
        </select>

        <input
          v-model="capability"
          class="permission-input"
          list="workflow-capabilities"
          placeholder="Capability，例如 fs.read"
          :disabled="busy"
        />
        <datalist id="workflow-capabilities">
          <option value="clipboard.read" />
          <option value="screen.capture" />
          <option value="fs.read" />
          <option value="fs.write" />
          <option value="shell.open" />
          <option value="shell.execute" />
          <option value="mcp.call:mcp__server__tool" />
        </datalist>

        <input
          v-model="resource"
          class="permission-input"
          :placeholder="resourceRequired ? '文件 / 目录 / 目标完整路径（必填）' : 'Resource，可选'"
          :disabled="busy"
        />

        <button
          class="grant-btn"
          :disabled="busy || !workflowId || (resourceRequired && !resource.trim())"
          @click="requestAlwaysGrant"
        >
          {{ busy ? '处理中…' : '申请 Always 授权' }}
        </button>
      </div>

      <p class="permission-hint">
        Medium / High / Critical 能力仍会弹出标准审批框；只有选择“始终允许”后，后台 Scheduler 才会接受该授权。fs.read 必须填写精确路径；已存在文件/目录会解析为真实路径，exists 使用的未创建目标会按真实父目录规范化。目录授权不会自动覆盖子文件。
      </p>

      <div v-if="workflowGrants.length" class="grant-list">
        <div
          v-for="grant in workflowGrants"
          :key="grant.pluginId + ':' + grant.capability + ':' + (grant.resource ?? '')"
          class="grant-item"
        >
          <code>{{ grant.capability }}</code>
          <span v-if="grant.resource">{{ grant.resource }}</span>
          <button :disabled="busy" @click="revokeGrant(grant)">撤销</button>
        </div>
      </div>

      <div v-if="status" class="status">{{ status }}</div>
      <div v-if="error" class="error">{{ error }}</div>
    </div>
  </details>
</template>

<style scoped>
.workflow-permission-panel {
  flex: 0 0 auto;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.workflow-permission-panel summary {
  padding: 7px 16px;
  cursor: pointer;
  color: var(--text-secondary);
  font-size: 11px;
  font-weight: 600;
}

.permission-body {
  padding: 0 16px 10px;
}

.permission-row {
  display: grid;
  grid-template-columns: minmax(150px, 1.2fr) minmax(150px, 1fr) minmax(150px, 1fr) auto;
  gap: 7px;
}

.permission-input,
.grant-btn {
  height: 30px;
  border: 1px solid var(--border-color);
  border-radius: 7px;
  font-size: 10px;
}

.permission-input {
  min-width: 0;
  padding: 0 8px;
  color: var(--text-primary);
  background: var(--bg-primary);
}

.grant-btn {
  padding: 0 10px;
  color: white;
  background: var(--accent-color);
  cursor: pointer;
}

.permission-hint,
.status,
.error {
  margin: 7px 0 0;
  font-size: 10px;
  line-height: 1.45;
}

.permission-hint,
.status {
  color: var(--text-tertiary);
}

.error {
  color: var(--danger-color);
}

.grant-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 7px;
}

.grant-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 6px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-size: 9px;
}

.grant-item span {
  color: var(--text-tertiary);
}

.grant-item button {
  border: 0;
  color: var(--danger-color);
  background: transparent;
  cursor: pointer;
}

@media (max-width: 900px) {
  .permission-row {
    grid-template-columns: 1fr 1fr;
  }
}
</style>
