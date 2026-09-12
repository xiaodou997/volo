<template>
  <section class="settings-section">
    <h3 class="section-title">技能</h3>

    <div class="section-hint">
      技能是包含 SKILL.md 的目录，向内置 Agent 提供可复用的任务指令。配置即信任——请只添加你信任的技能。增删后下次问 AI 时生效。
    </div>

    <div v-if="skills.length === 0" class="empty-hint">暂无已安装技能</div>

    <div v-for="skill in skills" :key="skill.name" class="setting-item">
      <div class="setting-label">
        <span class="label-text">
          {{ skill.name }}
          <span v-if="skill.version" class="skill-version">v{{ skill.version }}</span>
        </span>
        <span class="label-desc">{{ skill.description || '（无描述）' }}</span>
      </div>
      <div class="setting-control">
        <button class="danger-btn" @click="$emit('remove', skill.name)">删除</button>
      </div>
    </div>

    <div class="section-actions">
      <button class="action-btn" @click="$emit('install')">从目录安装…</button>
      <button class="action-btn" @click="$emit('openDir')">打开技能目录</button>
    </div>
    <div v-if="error" class="section-error">{{ error }}</div>
  </section>
</template>

<script setup lang="ts">
import type { SkillMeta } from '../../api/rubick';

defineProps<{
  skills: SkillMeta[];
  error: string;
}>();

defineEmits<{
  (e: 'install'): void;
  (e: 'remove', name: string): void;
  (e: 'openDir'): void;
}>();
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

.section-hint {
  padding: 4px 0 8px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-tertiary);
}

.empty-hint {
  padding: 12px 0;
  font-size: 13px;
  color: var(--text-tertiary);
}

.setting-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 0;
  border-bottom: 1px solid var(--border-color);
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

.skill-version {
  margin-left: 6px;
  font-size: 11px;
  font-weight: 400;
  color: var(--text-tertiary);
}

.section-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  padding: 12px 0;
}

.action-btn,
.danger-btn {
  padding: 6px 12px;
  font-size: 13px;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.2s;
}

.action-btn {
  border: 1px solid var(--border-color);
  background: var(--bg-secondary);
  color: var(--text-primary);
}

.action-btn:hover {
  background: var(--hover-bg);
}

.danger-btn {
  border: 1px solid var(--danger-color);
  background: transparent;
  color: var(--danger-color);
}

.danger-btn:hover {
  background: var(--danger-color);
  color: white;
}

.section-error {
  font-size: 12px;
  color: var(--danger-color);
}
</style>
