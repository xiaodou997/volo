/**
 * 设置页面组件
 */

<template>
  <div class="settings-view">
    <!-- 顶部导航 -->
    <div class="settings-header">
      <button class="back-btn" @click="$emit('back')">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M19 12H5M12 19l-7-7 7-7"/>
        </svg>
      </button>
      <h2 class="title">设置</h2>
    </div>

    <!-- 设置内容 -->
    <div class="settings-content">
      <!-- 外观设置 -->
      <section class="settings-section">
        <h3 class="section-title">外观</h3>

        <div class="setting-item">
          <div class="setting-label">
            <span class="label-text">主题</span>
            <span class="label-desc">选择应用主题</span>
          </div>
          <div class="setting-control">
            <select v-model="settings.theme" @change="onThemeChange">
              <option value="system">跟随系统</option>
              <option value="light">亮色</option>
              <option value="dark">暗色</option>
            </select>
          </div>
        </div>

        <div class="setting-item">
          <div class="setting-label">
            <span class="label-text">窗口透明度</span>
            <span class="label-desc">调整窗口背景透明度</span>
          </div>
          <div class="setting-control">
            <input
              type="range"
              v-model.number="settings.opacity"
              min="0.7"
              max="1"
              step="0.05"
              @change="onOpacityChange"
            />
            <span class="range-value">{{ Math.round(settings.opacity * 100) }}%</span>
          </div>
        </div>
      </section>

      <!-- 快捷键设置 -->
      <section class="settings-section">
        <h3 class="section-title">快捷键</h3>

        <div class="setting-item">
          <div class="setting-label">
            <span class="label-text">呼出窗口</span>
            <span class="label-desc">全局快捷键</span>
          </div>
          <div class="setting-control">
            <button
              class="shortcut-btn"
              :class="{ recording: recordingShortcut }"
              @click="startRecordShortcut"
            >
              {{ recordingShortcut ? '按下快捷键...' : settings.shortcut }}
            </button>
          </div>
        </div>
      </section>

      <!-- 通用设置 -->
      <section class="settings-section">
        <h3 class="section-title">通用</h3>

        <div class="setting-item">
          <div class="setting-label">
            <span class="label-text">开机启动</span>
            <span class="label-desc">登录时自动启动</span>
          </div>
          <div class="setting-control">
            <label class="toggle">
              <input type="checkbox" v-model="settings.autoLaunch" @change="onAutoLaunchChange" />
              <span class="toggle-slider"></span>
            </label>
          </div>
        </div>

        <div class="setting-item">
          <div class="setting-label">
            <span class="label-text">失焦隐藏</span>
            <span class="label-desc">窗口失去焦点时自动隐藏</span>
          </div>
          <div class="setting-control">
            <label class="toggle">
              <input type="checkbox" v-model="settings.hideOnBlur" @change="saveSettings" />
              <span class="toggle-slider"></span>
            </label>
          </div>
        </div>

        <div class="setting-item">
          <div class="setting-label">
            <span class="label-text">搜索历史</span>
            <span class="label-desc">记录搜索历史</span>
          </div>
          <div class="setting-control">
            <label class="toggle">
              <input type="checkbox" v-model="settings.enableHistory" @change="saveSettings" />
              <span class="toggle-slider"></span>
            </label>
          </div>
        </div>
      </section>

      <!-- 数据管理 -->
      <section class="settings-section">
        <h3 class="section-title">数据</h3>

        <div class="setting-item">
          <div class="setting-label">
            <span class="label-text">清除搜索历史</span>
            <span class="label-desc">删除所有搜索记录</span>
          </div>
          <div class="setting-control">
            <button class="danger-btn" @click="clearHistory">清除</button>
          </div>
        </div>

        <div class="setting-item">
          <div class="setting-label">
            <span class="label-text">刷新应用缓存</span>
            <span class="label-desc">重新扫描本地应用</span>
          </div>
          <div class="setting-control">
            <button class="action-btn" @click="refreshCache">刷新</button>
          </div>
        </div>
      </section>

      <!-- AI 设置 -->
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
              v-model="llmConfig.baseUrl"
              placeholder="https://api.openai.com/v1"
              @blur="saveLlmConfig"
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
              v-model="llmConfig.model"
              placeholder="gpt-4o-mini / deepseek-chat"
              @blur="saveLlmConfig"
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
              v-model="apiKeyInput"
              placeholder="sk-..."
            />
            <button class="action-btn" @click="saveApiKey">保存</button>
          </div>
        </div>
      </section>

      <!-- 权限管理 -->
      <section class="settings-section">
        <h3 class="section-title">权限管理</h3>

        <div v-if="grants.length === 0" class="grants-empty">
          暂无已授权的插件权限
        </div>

        <div v-for="grant in grants" :key="grant.pluginId + ':' + grant.capability" class="setting-item">
          <div class="setting-label">
            <span class="label-text">
              {{ grant.pluginId }}
              <span class="grant-risk" :class="'grant-risk-' + grant.risk.toLowerCase()">{{ riskLabel(grant.risk) }}</span>
            </span>
            <span class="label-desc">{{ grant.description }} · {{ scopeLabel(grant.scope) }}</span>
          </div>
          <div class="setting-control">
            <button class="danger-btn" @click="revokeGrant(grant)">撤销</button>
          </div>
        </div>
      </section>

      <!-- 关于 -->
      <section class="settings-section">
        <h3 class="section-title">关于</h3>

        <div class="about-info">
          <div class="app-logo">V</div>
          <div class="app-name">Volo</div>
          <div class="app-version">版本 0.1.0</div>
          <div class="app-desc">桌面效率工具箱</div>
        </div>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useSearchStore } from '../stores/search';
import type { LlmConfig, PermissionGrant, PermissionScope, RiskLevel } from '../api/rubick';

const emit = defineEmits<{
  (e: 'back'): void;
}>();

// 设置数据
const settings = ref({
  theme: 'system',
  opacity: 0.95,
  shortcut: 'Cmd+Space',
  autoLaunch: false,
  hideOnBlur: true,
  enableHistory: true,
});

// 快捷键录制状态
const recordingShortcut = ref(false);

// 加载设置
async function loadSettings() {
  try {
    const config = await invoke<any>('get_config');
    if (config) {
      settings.value = { ...settings.value, ...config };
    }
  } catch (e) {
    console.error('Failed to load settings:', e);
  }
}

// 保存设置
async function saveSettings() {
  try {
    await invoke('save_config', { config: settings.value });
  } catch (e) {
    console.error('Failed to save settings:', e);
  }
}

// 主题变化
function onThemeChange() {
  applyTheme(settings.value.theme);
  saveSettings();
}

// 应用主题
function applyTheme(theme: string) {
  const root = document.documentElement;

  if (theme === 'dark') {
    root.classList.add('dark');
  } else if (theme === 'light') {
    root.classList.remove('dark');
  } else {
    // 跟随系统
    const isDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    if (isDark) {
      root.classList.add('dark');
    } else {
      root.classList.remove('dark');
    }
  }
}

// 透明度变化
function onOpacityChange() {
  document.documentElement.style.setProperty('--window-opacity', String(settings.value.opacity));
  saveSettings();
}

// 开机启动变化
async function onAutoLaunchChange() {
  try {
    // TODO: 调用 Tauri API 设置开机启动
    await saveSettings();
  } catch (e) {
    console.error('Failed to set auto launch:', e);
  }
}

// 开始录制快捷键
function startRecordShortcut() {
  recordingShortcut.value = true;
  document.addEventListener('keydown', handleShortcutKeydown);
}

// 处理快捷键按下
async function handleShortcutKeydown(e: KeyboardEvent) {
  if (!recordingShortcut.value) return;

  e.preventDefault();
  e.stopPropagation();

  // 忽略单独的修饰键
  if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) {
    return;
  }

  // 构建快捷键字符串
  const parts: string[] = [];
  if (e.metaKey) parts.push('Cmd');
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');

  // 添加主键
  let key = e.key.toUpperCase();
  if (key === ' ') key = 'Space';
  if (key === 'ESCAPE') {
    // ESC 取消录制
    recordingShortcut.value = false;
    document.removeEventListener('keydown', handleShortcutKeydown);
    return;
  }
  parts.push(key);

  const shortcut = parts.join('+');
  settings.value.shortcut = shortcut;

  // 注册新快捷键
  try {
    await invoke('register_shortcut', { shortcut });
    await saveSettings();
  } catch (err) {
    console.error('Failed to register shortcut:', err);
  }

  recordingShortcut.value = false;
  document.removeEventListener('keydown', handleShortcutKeydown);
}

// 清除搜索历史
async function clearHistory() {
  try {
    await invoke('clear_search_history');
  } catch (e) {
    console.error('Failed to clear history:', e);
  }
}

// 刷新应用缓存
async function refreshCache() {
  try {
    await invoke('refresh_app_cache');
  } catch (e) {
    console.error('Failed to refresh cache:', e);
  }
}

// ============ AI 设置 ============

const llmConfig = ref<LlmConfig>({ baseUrl: '', model: '' });
const apiKeyInput = ref('');
const hasApiKey = ref(false);

// 加载 LLM 配置与 API key 状态
async function loadLlmConfig() {
  try {
    const config = await invoke<LlmConfig>('llm_get_config');
    llmConfig.value = { baseUrl: config.baseUrl ?? '', model: config.model ?? '' };
    hasApiKey.value = await invoke<boolean>('llm_has_api_key');
  } catch (e) {
    console.error('Failed to load LLM config:', e);
  }
}

// 保存 Base URL / Model（失焦时触发）
async function saveLlmConfig() {
  try {
    await invoke('llm_set_config', {
      baseUrl: llmConfig.value.baseUrl,
      model: llmConfig.value.model,
    });
    useSearchStore().refreshLlmStatus();
  } catch (e) {
    console.error('Failed to save LLM config:', e);
  }
}

// 保存 API Key（只写不回显）
async function saveApiKey() {
  const key = apiKeyInput.value.trim();
  if (!key) return;
  try {
    await invoke('llm_set_api_key', { key });
    apiKeyInput.value = '';
    hasApiKey.value = true;
    useSearchStore().refreshLlmStatus();
  } catch (e) {
    console.error('Failed to save API key:', e);
    alert(`API Key 保存失败：${e}`);
  }
}

// ============ 权限管理 ============

// 已授权的插件权限
const grants = ref<PermissionGrant[]>([]);

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

// 加载授权列表
async function loadGrants() {
  try {
    grants.value = await invoke<PermissionGrant[]>('permission_list_grants');
  } catch (e) {
    console.error('Failed to load permission grants:', e);
  }
}

// 撤销授权
async function revokeGrant(grant: PermissionGrant) {
  try {
    await invoke('permission_revoke', {
      pluginId: grant.pluginId,
      capability: grant.capability,
    });
    await loadGrants();
  } catch (e) {
    console.error('Failed to revoke permission:', e);
  }
}

// 监听系统主题变化
const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
function handleSystemThemeChange() {
  if (settings.value.theme === 'system') {
    applyTheme('system');
  }
}

onMounted(() => {
  loadSettings();
  loadGrants();
  loadLlmConfig();
  mediaQuery.addEventListener('change', handleSystemThemeChange);
});

onUnmounted(() => {
  document.removeEventListener('keydown', handleShortcutKeydown);
  mediaQuery.removeEventListener('change', handleSystemThemeChange);
});
</script>

<style scoped>
.settings-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
}

.settings-header {
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
  margin-left: 12px;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.settings-content {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
}

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

/* 下拉选择 */
select {
  padding: 6px 12px;
  font-size: 14px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  cursor: pointer;
  outline: none;
}

select:focus {
  border-color: var(--accent-color);
}

/* 滑块 */
input[type="range"] {
  width: 100px;
  height: 4px;
  background: var(--border-color);
  border-radius: 2px;
  outline: none;
  cursor: pointer;
}

input[type="range"]::-webkit-slider-thumb {
  -webkit-appearance: none;
  width: 14px;
  height: 14px;
  background: var(--accent-color);
  border-radius: 50%;
  cursor: pointer;
}

.range-value {
  font-size: 12px;
  color: var(--text-secondary);
  min-width: 36px;
}

/* 开关 */
.toggle {
  position: relative;
  display: inline-block;
  width: 44px;
  height: 24px;
}

.toggle input {
  opacity: 0;
  width: 0;
  height: 0;
}

.toggle-slider {
  position: absolute;
  cursor: pointer;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background-color: var(--border-color);
  transition: 0.3s;
  border-radius: 24px;
}

.toggle-slider::before {
  position: absolute;
  content: "";
  height: 18px;
  width: 18px;
  left: 3px;
  bottom: 3px;
  background-color: white;
  transition: 0.3s;
  border-radius: 50%;
}

.toggle input:checked + .toggle-slider {
  background-color: var(--accent-color);
}

.toggle input:checked + .toggle-slider::before {
  transform: translateX(20px);
}

/* 按钮 */
.shortcut-btn {
  padding: 6px 12px;
  font-size: 13px;
  font-family: monospace;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  cursor: pointer;
  transition: all 0.2s;
}

.shortcut-btn:hover {
  border-color: var(--accent-color);
}

.shortcut-btn.recording {
  border-color: var(--accent-color);
  animation: pulse 1s infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.6; }
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

/* 文本输入框（AI 设置） */
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

/* 权限管理 */
.grants-empty {
  padding: 12px 0;
  font-size: 13px;
  color: var(--text-tertiary);
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

/* 关于 */
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
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 32px;
  font-weight: bold;
  color: white;
  background: linear-gradient(135deg, var(--accent-color), #6366f1);
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
</style>