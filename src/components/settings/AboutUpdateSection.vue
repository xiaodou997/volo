<template>
  <section class="settings-section">
    <h3 class="section-title">关于与更新</h3>

    <div class="about-info">
      <img class="app-logo" :src="logoUrl" alt="Volo logo" />
      <div class="app-name">Volo</div>
      <div class="app-version">版本 {{ appVersion }}</div>
      <div class="app-desc">桌面效率工具箱</div>
    </div>

    <div class="setting-item">
      <div class="setting-label">
        <span class="label-text">检查更新</span>
        <span class="label-desc">{{ updateStatus }}</span>
      </div>
      <div class="setting-control">
        <button
          v-if="updateAvailable"
          class="action-btn"
          :disabled="updating"
          @click="$emit('installUpdate')"
        >
          {{ updating ? updateProgress : '立即更新' }}
        </button>
        <button
          v-else
          class="action-btn"
          :disabled="checkingUpdate"
          @click="$emit('checkUpdate')"
        >
          {{ checkingUpdate ? '检查中…' : '检查更新' }}
        </button>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-label">
        <span class="label-text">会话日志</span>
        <span class="label-desc">打开 AI 会话事件日志目录</span>
      </div>
      <div class="setting-control">
        <button class="action-btn" @click="$emit('openSessions')">打开</button>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import logoUrl from '../../assets/logo.png';

defineProps<{
  appVersion: string;
  updateStatus: string;
  updateAvailable: boolean;
  checkingUpdate: boolean;
  updating: boolean;
  updateProgress: string;
}>();

defineEmits<{
  (e: 'checkUpdate'): void;
  (e: 'installUpdate'): void;
  (e: 'openSessions'): void;
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

.about-info {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 24px 0;
  text-align: center;
}

.app-logo {
  width: 64px;
  height: 64px;
  border-radius: 16px;
  margin-bottom: 12px;
}

.app-name {
  font-size: 18px;
  font-weight: 600;
  color: var(--text-primary);
}

.app-version {
  font-size: 13px;
  color: var(--text-secondary);
  margin-top: 4px;
}

.app-desc {
  font-size: 13px;
  color: var(--text-tertiary);
  margin-top: 4px;
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

.action-btn {
  padding: 6px 12px;
  font-size: 13px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  cursor: pointer;
  transition: all 0.2s;
}

.action-btn:hover {
  background: var(--hover-bg);
}

.action-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
</style>
