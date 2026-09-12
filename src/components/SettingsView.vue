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

    <div class="settings-content">
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
            <span class="label-text">Dock 图标</span>
            <span class="label-desc">在 Dock 栏显示应用图标（macOS），关闭后只保留菜单栏托盘</span>
          </div>
          <div class="setting-control">
            <label class="toggle">
              <input type="checkbox" v-model="settings.showDockIcon" @change="onDockIconChange" />
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

      <AiSettingsSection
        :base-url="llmConfig.baseUrl"
        :model="llmConfig.model"
        :api-key-input="apiKeyInput"
        :has-api-key="hasApiKey"
        @update-base-url="llmConfig.baseUrl = $event"
        @update-model="llmConfig.model = $event"
        @update-api-key-input="apiKeyInput = $event"
        @save-config="saveLlmConfig"
        @save-api-key="saveApiKey"
      />

      <!-- MCP 服务器 -->
      <section class="settings-section">
        <h3 class="section-title">MCP 服务器</h3>

        <div class="mcp-hint">
          MCP 服务器可以是本地启动的工具进程（stdio，填命令+参数），也可以是远程服务（Streamable HTTP，填 URL）。
          配置即信任——请只添加你信任的 server。修改后下次问 AI 时生效。
        </div>

        <div v-if="mcpServerNames.length === 0" class="grants-empty">
          暂无 MCP 服务器
        </div>

        <div v-for="name in mcpServerNames" :key="name" class="setting-item">
          <div class="setting-label">
            <span class="label-text">{{ name }}</span>
            <span class="label-desc">{{ mcpSummary(mcpServers[name]) }}</span>
          </div>
          <div class="setting-control">
            <label class="toggle">
              <input
                type="checkbox"
                :checked="mcpServers[name].enabled"
                @change="toggleMcpServer(name)"
              />
              <span class="toggle-slider"></span>
            </label>
            <button class="danger-btn" @click="removeMcpServer(name)">删除</button>
          </div>
        </div>

        <div class="mcp-add-form">
          <input
            type="text"
            class="text-input"
            v-model="mcpForm.name"
            placeholder="名称（如 filesystem）"
          />
          <input
            type="text"
            class="text-input"
            v-model="mcpForm.command"
            placeholder="命令（如 npx；填了 URL 可留空）"
          />
          <input
            type="text"
            class="text-input mcp-args-input"
            v-model="mcpForm.args"
            placeholder="参数（空格分隔，可留空）"
          />
          <input
            type="text"
            class="text-input"
            v-model="mcpForm.url"
            placeholder="远程 URL（如 http://localhost:3000/mcp，可留空）"
          />
          <button class="action-btn" @click="addMcpServer">添加</button>
        </div>
        <div v-if="mcpFormError" class="mcp-form-error">{{ mcpFormError }}</div>
      </section>

      <section class="settings-section">
        <h3 class="section-title">技能</h3>

        <div class="mcp-hint">
          技能是包含 SKILL.md 的目录，向内置 Agent 提供可复用的任务指令。配置即信任——请只添加你信任的技能。增删后下次问 AI 时生效。
        </div>

        <div v-if="skills.length === 0" class="grants-empty">暂无已安装技能</div>

        <div v-for="skill in skills" :key="skill.name" class="setting-item">
          <div class="setting-label">
            <span class="label-text">
              {{ skill.name }}
              <span v-if="skill.version" class="skill-version">v{{ skill.version }}</span>
            </span>
            <span class="label-desc">{{ skill.description || '（无描述）' }}</span>
          </div>
          <div class="setting-control">
            <button class="danger-btn" @click="removeSkill(skill.name)">删除</button>
          </div>
        </div>

        <div class="mcp-add-form">
          <button class="action-btn" @click="addSkill">从目录安装…</button>
          <button class="action-btn" @click="openSkillsDir">打开技能目录</button>
        </div>
        <div v-if="skillError" class="mcp-form-error">{{ skillError }}</div>
      </section>

      <PermissionSettingsSection :grants="grants" @revoke="revokeGrant" />

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
              @click="installUpdate"
            >
              {{ updating ? updateProgress : '立即更新' }}
            </button>
            <button
              v-else
              class="action-btn"
              :disabled="checkingUpdate"
              @click="checkUpdate"
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
            <button class="action-btn" @click="openSessionsDir">打开</button>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { useSearchStore } from '../stores/search';
import { withNativeDialog } from '../composables/nativeDialog';
import { hideOnBlur } from '../composables/appConfig';
import type { LlmConfig, McpServerConfig, PermissionGrant, SkillMeta } from '../api/rubick';
import AiSettingsSection from './settings/AiSettingsSection.vue';
import PermissionSettingsSection from './settings/PermissionSettingsSection.vue';
import logoUrl from '../assets/logo.png';

const emit = defineEmits<{
  (e: 'back'): void;
}>();

const settings = ref({
  theme: 'system',
  opacity: 0.95,
  shortcut: 'Cmd+Space',
  autoLaunch: false,
  hideOnBlur: true,
  enableHistory: true,
  showDockIcon: true,
});

const recordingShortcut = ref(false);

async function loadSettings() {
  try {
    const config = await invoke<any>('get_config');
    if (config) settings.value = { ...settings.value, ...config };
  } catch (e) {
    console.error('Failed to load settings:', e);
  }
}

async function saveSettings() {
  try {
    await invoke('save_config', { newConfig: settings.value });
    hideOnBlur.value = settings.value.hideOnBlur;
  } catch (e) {
    console.error('Failed to save settings:', e);
  }
}

function onThemeChange() {
  applyTheme(settings.value.theme);
  saveSettings();
}

async function onDockIconChange() {
  try {
    await invoke('set_dock_icon_visible', { visible: settings.value.showDockIcon });
  } catch (e) {
    console.error('Failed to set dock icon visibility:', e);
  }
}

function applyTheme(theme: string) {
  const root = document.documentElement;
  if (theme === 'dark') {
    root.classList.add('dark');
  } else if (theme === 'light') {
    root.classList.remove('dark');
  } else {
    const isDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    if (isDark) root.classList.add('dark');
    else root.classList.remove('dark');
  }
}

function onOpacityChange() {
  document.documentElement.style.setProperty('--window-opacity', String(settings.value.opacity));
  saveSettings();
}

async function onAutoLaunchChange() {
  try {
    await saveSettings();
  } catch (e) {
    console.error('Failed to set auto launch:', e);
  }
}

function startRecordShortcut() {
  recordingShortcut.value = true;
  document.addEventListener('keydown', handleShortcutKeydown);
}

async function handleShortcutKeydown(e: KeyboardEvent) {
  if (!recordingShortcut.value) return;
  e.preventDefault();
  e.stopPropagation();
  if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return;

  const parts: string[] = [];
  if (e.metaKey) parts.push('Cmd');
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');

  let key = e.key.toUpperCase();
  if (key === ' ') key = 'Space';
  if (key === 'ESCAPE') {
    recordingShortcut.value = false;
    document.removeEventListener('keydown', handleShortcutKeydown);
    return;
  }
  parts.push(key);

  const shortcut = parts.join('+');
  settings.value.shortcut = shortcut;
  try {
    await invoke('register_shortcut', { shortcut });
    await saveSettings();
  } catch (err) {
    console.error('Failed to register shortcut:', err);
  }

  recordingShortcut.value = false;
  document.removeEventListener('keydown', handleShortcutKeydown);
}

async function clearHistory() {
  try {
    await invoke('clear_search_history');
  } catch (e) {
    console.error('Failed to clear history:', e);
  }
}

async function refreshCache() {
  try {
    await invoke('refresh_app_cache');
  } catch (e) {
    console.error('Failed to refresh cache:', e);
  }
}

const llmConfig = ref<LlmConfig>({ baseUrl: '', model: '' });
const apiKeyInput = ref('');
const hasApiKey = ref(false);

async function loadLlmConfig() {
  try {
    const config = await invoke<LlmConfig>('llm_get_config');
    llmConfig.value = { baseUrl: config.baseUrl ?? '', model: config.model ?? '' };
    hasApiKey.value = await invoke<boolean>('llm_has_api_key');
  } catch (e) {
    console.error('Failed to load LLM config:', e);
  }
}

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

const mcpServers = ref<Record<string, McpServerConfig>>({});
const mcpForm = ref({ name: '', command: '', args: '', url: '' });
const mcpFormError = ref('');
const mcpServerNames = computed(() => Object.keys(mcpServers.value));

async function loadMcpServers() {
  try {
    const config = await invoke<any>('get_config');
    mcpServers.value = config?.mcpServers ?? {};
  } catch (e) {
    console.error('Failed to load MCP servers:', e);
  }
}

function mcpSummary(server: McpServerConfig): string {
  if (server.url?.trim()) return `HTTP · ${server.url}`;
  return [server.command, ...(server.args ?? [])].join(' ');
}

async function saveMcpServers() {
  try {
    const config = await invoke<any>('get_config');
    await invoke('save_config', {
      newConfig: { ...config, mcpServers: mcpServers.value },
    });
  } catch (e) {
    console.error('Failed to save MCP servers:', e);
    alert(`MCP 服务器配置保存失败：${e}`);
  }
}

function addMcpServer() {
  const name = mcpForm.value.name.trim();
  const command = mcpForm.value.command.trim();
  const args = mcpForm.value.args.split(/\s+/).filter(Boolean);
  const url = mcpForm.value.url.trim();

  if (!name) {
    mcpFormError.value = '名称不能为空';
    return;
  }
  if (mcpServers.value[name]) {
    mcpFormError.value = `已存在同名服务器「${name}」`;
    return;
  }
  if (!command && !url) {
    mcpFormError.value = '命令和 URL 至少填一个（URL 非空走远程 HTTP）';
    return;
  }
  if (url && !/^https?:\/\//.test(url)) {
    mcpFormError.value = 'URL 需以 http:// 或 https:// 开头';
    return;
  }

  mcpFormError.value = '';
  mcpServers.value = {
    ...mcpServers.value,
    [name]: { command, args, env: {}, url, enabled: true },
  };
  mcpForm.value = { name: '', command: '', args: '', url: '' };
  saveMcpServers();
}

function toggleMcpServer(name: string) {
  const server = mcpServers.value[name];
  if (!server) return;
  mcpServers.value = {
    ...mcpServers.value,
    [name]: { ...server, enabled: !server.enabled },
  };
  saveMcpServers();
}

function removeMcpServer(name: string) {
  const next = { ...mcpServers.value };
  delete next[name];
  mcpServers.value = next;
  saveMcpServers();
}

const skills = ref<SkillMeta[]>([]);
const skillError = ref('');

async function loadSkills() {
  try {
    skills.value = await invoke<SkillMeta[]>('skill_list');
  } catch (e) {
    console.error('Failed to load skills:', e);
  }
}

async function addSkill() {
  skillError.value = '';
  try {
    const selected = await withNativeDialog(() => invoke<string | null>('fs_pick_folder'));
    if (!selected) return;
    await invoke('skill_install_from_dir', { sourceDir: selected });
    await loadSkills();
  } catch (e) {
    skillError.value = String(e);
  }
}

async function removeSkill(name: string) {
  skillError.value = '';
  try {
    await invoke('skill_remove', { name });
    await loadSkills();
  } catch (e) {
    skillError.value = String(e);
  }
}

async function openSkillsDir() {
  skillError.value = '';
  try {
    await invoke('open_skills_dir');
  } catch (e) {
    skillError.value = String(e);
  }
}

const grants = ref<PermissionGrant[]>([]);

async function loadGrants() {
  try {
    grants.value = await invoke<PermissionGrant[]>('permission_list_grants');
  } catch (e) {
    console.error('Failed to load permission grants:', e);
  }
}

async function revokeGrant(grant: PermissionGrant) {
  try {
    await invoke('permission_revoke', {
      pluginId: grant.pluginId,
      capability: grant.capability,
      resource: grant.resource ?? null,
    });
    await loadGrants();
  } catch (e) {
    console.error('Failed to revoke permission:', e);
  }
}

const appVersion = ref('');
const checkingUpdate = ref(false);
const updating = ref(false);
const updateStatus = ref('检查是否有新版本');
const updateProgress = ref('');
const pendingUpdate = ref<Update | null>(null);
const updateAvailable = ref(false);

async function loadAppVersion() {
  try {
    appVersion.value = await getVersion();
  } catch (e) {
    console.error('Failed to get app version:', e);
  }
}

async function checkUpdate() {
  checkingUpdate.value = true;
  updateStatus.value = '正在检查更新…';
  try {
    const update = await check();
    if (update) {
      pendingUpdate.value = update;
      updateAvailable.value = true;
      updateStatus.value = `发现新版本 ${update.version}（当前 ${update.currentVersion}）`;
    } else {
      updateAvailable.value = false;
      updateStatus.value = '已是最新';
    }
  } catch (e) {
    updateStatus.value = `检查更新失败：${e}`;
  } finally {
    checkingUpdate.value = false;
  }
}

async function installUpdate() {
  const update = pendingUpdate.value;
  if (!update || updating.value) return;
  updating.value = true;
  try {
    let downloaded = 0;
    let total = 0;
    await update.downloadAndInstall((event) => {
      if (event.event === 'Started') {
        total = event.data.contentLength ?? 0;
        updateProgress.value = '下载中…';
      } else if (event.event === 'Progress') {
        downloaded += event.data.chunkLength;
        updateProgress.value = total > 0
          ? `下载中 ${Math.round((downloaded / total) * 100)}%`
          : '下载中…';
      } else if (event.event === 'Finished') {
        updateProgress.value = '安装中…';
      }
    });
    await relaunch();
  } catch (e) {
    updateStatus.value = `更新失败：${e}`;
    updating.value = false;
  }
}

async function openSessionsDir() {
  try {
    await invoke('open_sessions_dir');
  } catch (e) {
    console.error('Failed to open sessions dir:', e);
    alert(`打开会话日志目录失败：${e}`);
  }
}

const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
function handleSystemThemeChange() {
  if (settings.value.theme === 'system') applyTheme('system');
}

onMounted(() => {
  loadSettings();
  loadGrants();
  loadLlmConfig();
  loadMcpServers();
  loadSkills();
  loadAppVersion();
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

.mcp-hint {
  padding: 4px 0 8px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-tertiary);
}

.mcp-add-form {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  padding: 12px 0;
}

.mcp-add-form .text-input {
  width: 140px;
}

.mcp-add-form .mcp-args-input {
  flex: 1;
  min-width: 200px;
}

.mcp-form-error {
  font-size: 12px;
  color: var(--danger-color);
}

.skill-version {
  margin-left: 6px;
  font-size: 11px;
  font-weight: 400;
  color: var(--text-tertiary);
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
</style>
