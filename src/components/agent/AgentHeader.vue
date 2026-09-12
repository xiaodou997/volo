<template>
  <div class="agent-header">
    <button class="back-btn" @click="$emit('back')">
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M19 12H5M12 19l-7-7 7-7"/>
      </svg>
    </button>
    <h2 class="title">{{ title }}</h2>
    <span v-if="chatMode && !finished" class="running-dot"></span>
    <div class="header-actions">
      <button
        v-if="chatMode && !finished"
        class="header-btn stop-btn"
        :disabled="stopping"
        @click="$emit('stop')"
      >{{ stopping ? '停止中…' : '停止' }}</button>
      <button v-if="chatMode" class="header-btn" @click="$emit('history')">历史</button>
      <button class="header-btn" @click="$emit('newSession')">新对话</button>
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  title: string;
  chatMode: boolean;
  finished: boolean;
  stopping: boolean;
}>();

defineEmits<{
  (e: 'back'): void;
  (e: 'stop'): void;
  (e: 'history'): void;
  (e: 'newSession'): void;
}>();
</script>

<style scoped>
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

.stop-btn:hover {
  color: var(--danger-color);
}
</style>
