/**
 * 权限审批弹窗
 * 监听后端 permission-request 事件，队列化处理插件权限申请
 */

<template>
  <Teleport to="body">
    <div v-if="current" class="approval-overlay">
      <div class="approval-dialog">
        <div class="dialog-header">
          <span class="dialog-icon">🔐</span>
          <h3 class="dialog-title">权限申请</h3>
          <span class="risk-badge" :class="riskClass">{{ riskLabel }}</span>
        </div>

        <div class="dialog-body">
          <div class="request-info">
            <div class="info-row">
              <span class="info-label">插件</span>
              <span class="info-value">{{ current.pluginId }}</span>
            </div>
            <div class="info-row">
              <span class="info-label">权限</span>
              <span class="info-value">{{ current.description }}</span>
            </div>
            <div v-if="current.resource" class="info-row">
              <span class="info-label">资源</span>
              <span class="info-value resource-value">{{ current.resource }}</span>
            </div>
          </div>

          <div v-if="isHighRisk" class="risk-warning">
            该权限风险较高，插件获得授权后可能造成数据泄露或系统改动，请确认你信任此插件。
          </div>
        </div>

        <div class="dialog-footer">
          <button class="btn btn-deny" :disabled="responding" @click="respond(false)">
            拒绝
          </button>
          <div class="allow-group">
            <button class="btn" :disabled="responding" @click="respond(true, 'always')">
              始终允许
            </button>
            <button class="btn" :disabled="responding" @click="respond(true, 'session')">
              本次会话
            </button>
            <button class="btn btn-primary" :disabled="responding" @click="respond(true, 'once')">
              允许一次
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import type { PermissionRequest, PermissionScope, RiskLevel } from '../api/rubick';

// 待处理请求队列（先进先出）
const queue = ref<PermissionRequest[]>([]);
const responding = ref(false);

const current = computed<PermissionRequest | null>(() => queue.value[0] ?? null);

const RISK_LABELS: Record<RiskLevel, string> = {
  Low: '低风险',
  Medium: '中风险',
  High: '高风险',
  Critical: '严重风险',
};

const riskLabel = computed(() =>
  current.value ? RISK_LABELS[current.value.risk] ?? current.value.risk : '',
);

const riskClass = computed(() => {
  switch (current.value?.risk) {
    case 'Critical':
      return 'risk-critical';
    case 'High':
      return 'risk-high';
    case 'Medium':
      return 'risk-medium';
    default:
      return 'risk-low';
  }
});

const isHighRisk = computed(
  () => current.value?.risk === 'High' || current.value?.risk === 'Critical',
);

async function respond(allow: boolean, scope?: PermissionScope) {
  const req = current.value;
  if (!req || responding.value) return;

  responding.value = true;
  try {
    await invoke('permission_respond', {
      requestId: req.requestId,
      allow,
      ...(allow && scope ? { scope } : {}),
    });
  } catch (e) {
    console.error('Failed to respond permission request:', e);
  } finally {
    responding.value = false;
    queue.value.shift();
  }
}

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  unlisten = await listen<PermissionRequest>('permission-request', (event) => {
    queue.value.push(event.payload);
  });
});

onUnmounted(() => {
  unlisten?.();
});
</script>

<style scoped>
.approval-overlay {
  position: fixed;
  inset: 0;
  z-index: 9999;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.4);
}

.approval-dialog {
  width: 380px;
  max-width: calc(100vw - 48px);
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 12px;
  box-shadow: 0 20px 40px rgba(0, 0, 0, 0.25);
  overflow: hidden;
}

.dialog-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 14px 16px;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.dialog-icon {
  font-size: 18px;
}

.dialog-title {
  flex: 1;
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
}

.risk-badge {
  padding: 2px 8px;
  font-size: 11px;
  font-weight: 600;
  border-radius: 10px;
}

.risk-low {
  background: rgba(52, 199, 89, 0.15);
  color: #34c759;
}

.risk-medium {
  background: rgba(255, 159, 10, 0.15);
  color: #ff9f0a;
}

.risk-high,
.risk-critical {
  background: rgba(255, 59, 48, 0.15);
  color: #ff3b30;
}

.risk-critical {
  border: 1px solid #ff3b30;
}

.dialog-body {
  padding: 16px;
}

.request-info {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.info-row {
  display: flex;
  gap: 12px;
  font-size: 13px;
}

.info-label {
  flex-shrink: 0;
  width: 36px;
  color: var(--text-tertiary);
}

.info-value {
  color: var(--text-primary);
  word-break: break-all;
}

.resource-value {
  font-family: monospace;
  font-size: 12px;
  color: var(--text-secondary);
}

.risk-warning {
  margin-top: 12px;
  padding: 10px 12px;
  font-size: 12px;
  line-height: 1.5;
  color: #ff3b30;
  background: rgba(255, 59, 48, 0.08);
  border: 1px solid rgba(255, 59, 48, 0.3);
  border-radius: 8px;
}

.dialog-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 12px 16px;
  border-top: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.allow-group {
  display: flex;
  gap: 8px;
}

.btn {
  padding: 6px 12px;
  font-size: 13px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-primary);
  color: var(--text-primary);
  cursor: pointer;
  transition: all 0.2s;
  white-space: nowrap;
}

.btn:hover:not(:disabled) {
  background: var(--hover-bg);
}

.btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.btn-primary {
  background: var(--accent-color);
  border-color: var(--accent-color);
  color: white;
}

.btn-primary:hover:not(:disabled) {
  background: var(--accent-color);
  opacity: 0.9;
}

.btn-deny {
  background: transparent;
  border-color: var(--danger-color);
  color: var(--danger-color);
}

.btn-deny:hover:not(:disabled) {
  background: var(--danger-color);
  color: white;
}
</style>
