<template>
  <section class="settings-section">
    <h3 class="section-title">权限管理</h3>

    <div v-if="grants.length === 0" class="grants-empty">
      暂无已授权的插件权限
    </div>

    <div
      v-for="grant in grants"
      :key="grant.pluginId + ':' + grant.capability + ':' + (grant.resource ?? '')"
      class="setting-item"
    >
      <div class="setting-label">
        <span class="label-text">
          {{ grant.pluginId }}
          <span class="grant-risk" :class="'grant-risk-' + grant.risk.toLowerCase()">
            {{ riskLabel(grant.risk) }}
          </span>
        </span>
        <span class="label-desc">{{ grant.description }} · {{ scopeLabel(grant.scope) }}</span>
        <span v-if="grant.resource" class="label-desc grant-resource">{{ grant.resource }}</span>
      </div>
      <div class="setting-control">
        <button class="danger-btn" @click="$emit('revoke', grant)">撤销</button>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import type {
  PermissionGrant,
  PermissionScope,
  RiskLevel,
} from '../../api/rubick';

defineProps<{
  grants: PermissionGrant[];
}>();

defineEmits<{
  (e: 'revoke', grant: PermissionGrant): void;
}>();

const SCOPE_LABELS: Record<PermissionScope, string> = {
  once: '仅一次',
  session: '本次会话',
  always: '始终允许',
};

const RISK_LABELS: Record<RiskLevel, string> = {
  Low: '低风险',
  Medium: '中风险',
  High: '高风险',
  Critical: '严重风险',
};

function scopeLabel(scope: PermissionScope): string {
  return SCOPE_LABELS[scope] ?? scope;
}

function riskLabel(risk: RiskLevel): string {
  return RISK_LABELS[risk] ?? risk;
}
</script>

<style scoped>
.settings-section {
  margin-bottom: 24px;
}

.section-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin-bottom: 12px;
}

.setting-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 0;
  border-bottom: 1px solid var(--border-color);
}

.setting-item:last-child {
  border-bottom: none;
}

.setting-label {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.label-text {
  font-size: 14px;
  color: var(--text-primary);
}

.label-desc {
  font-size: 12px;
  color: var(--text-tertiary);
}

.setting-control {
  display: flex;
  align-items: center;
  gap: 8px;
}

.grants-empty {
  padding: 12px 0;
  font-size: 13px;
  color: var(--text-tertiary);
}

.grant-resource {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  word-break: break-all;
}

.grant-risk {
  margin-left: 6px;
  padding: 1px 6px;
  font-size: 11px;
  font-weight: 600;
  border-radius: 8px;
}

.grant-risk-low {
  background: rgba(52, 199, 89, 0.15);
  color: #34c759;
}

.grant-risk-medium {
  background: rgba(255, 159, 10, 0.15);
  color: #ff9f0a;
}

.grant-risk-high,
.grant-risk-critical {
  background: rgba(255, 59, 48, 0.15);
  color: #ff3b30;
}

.danger-btn {
  padding: 6px 12px;
  font-size: 13px;
  border: 1px solid var(--danger-color);
  border-radius: 6px;
  background: transparent;
  color: var(--danger-color);
  cursor: pointer;
  transition: all 0.2s;
}

.danger-btn:hover {
  background: var(--danger-color);
  color: white;
}
</style>
