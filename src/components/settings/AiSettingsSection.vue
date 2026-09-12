<template>
  <section class="settings-section">
    <h3 class="section-title">AI 设置</h3>

    <div class="setting-item">
      <div class="setting-label">
        <span class="label-text">Base URL</span>
        <span class="label-desc">支持 OpenAI 兼容服务（如 DeepSeek）</span>
      </div>
      <div class="setting-control">
        <input
          type="text"
          class="text-input"
          :value="baseUrl"
          placeholder="https://api.openai.com/v1"
          @input="$emit('updateBaseUrl', ($event.target as HTMLInputElement).value)"
          @blur="$emit('saveConfig')"
        />
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-label">
        <span class="label-text">Model</span>
        <span class="label-desc">模型名称</span>
      </div>
      <div class="setting-control">
        <input
          type="text"
          class="text-input"
          :value="model"
          placeholder="gpt-4o-mini / deepseek-chat"
          @input="$emit('updateModel', ($event.target as HTMLInputElement).value)"
          @blur="$emit('saveConfig')"
        />
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-label">
        <span class="label-text">API Key</span>
        <span class="label-desc">明文保存在本地配置文件（config.json），请勿外泄该文件</span>
      </div>
      <div class="setting-control">
        <span v-if="hasApiKey" class="api-key-ok">已配置 ✓</span>
        <input
          type="password"
          class="text-input"
          :value="apiKeyInput"
          placeholder="sk-..."
          @input="$emit('updateApiKeyInput', ($event.target as HTMLInputElement).value)"
        />
        <button class="action-btn" @click="$emit('saveApiKey')">保存</button>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
defineProps<{
  baseUrl: string;
  model: string;
  apiKeyInput: string;
  hasApiKey: boolean;
}>();

defineEmits<{
  (e: 'updateBaseUrl', value: string): void;
  (e: 'updateModel', value: string): void;
  (e: 'updateApiKeyInput', value: string): void;
  (e: 'saveConfig'): void;
  (e: 'saveApiKey'): void;
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

.text-input {
  width: 200px;
  padding: 6px 12px;
  font-size: 13px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  outline: none;
}

.text-input:focus {
  border-color: var(--accent-color);
}

.api-key-ok {
  font-size: 12px;
  color: #34c759;
  white-space: nowrap;
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
</style>
