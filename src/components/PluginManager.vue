/**
 * 插件管理组件
 */

<template>
  <div class="plugin-manager">
    <!-- 顶部导航 -->
    <div class="header">
      <button class="back-btn" @click="$emit('back')">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M19 12H5M12 19l-7-7 7-7"/>
        </svg>
      </button>
      <h2 class="title">插件管理</h2>
      <button class="install-btn" @click="showInstallDialog = true">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 5v14M5 12h14"/>
        </svg>
        安装插件
      </button>
    </div>

    <!-- 插件列表 -->
    <div class="plugin-list">
      <div v-if="loading" class="loading">
        <div class="spinner"></div>
        <span>加载中...</span>
      </div>

      <div v-else-if="plugins.length === 0" class="empty">
        <div class="empty-icon">🔌</div>
        <div class="empty-text">暂无已安装插件</div>
        <div class="empty-desc">点击右上角安装插件</div>
      </div>

      <div v-else class="list">
        <div
          v-for="plugin in plugins"
          :key="plugin.id"
          class="plugin-item"
          :class="{ disabled: !plugin.enabled }"
        >
          <!-- 插件图标 -->
          <div class="plugin-icon">
            <img v-if="plugin.icon" :src="plugin.icon" :alt="plugin.name" />
            <svg v-else width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/>
            </svg>
          </div>

          <!-- 插件信息 -->
          <div class="plugin-info">
            <div class="plugin-name">{{ plugin.name }}</div>
            <div class="plugin-meta">
              <span class="version">v{{ plugin.version }}</span>
              <span class="features">{{ plugin.features?.length || 0 }} 个功能</span>
            </div>
            <div v-if="plugin.description" class="plugin-desc">{{ plugin.description }}</div>
          </div>

          <!-- 插件操作 -->
          <div class="plugin-actions">
            <label class="toggle">
              <input
                type="checkbox"
                :checked="plugin.enabled"
                @change="togglePlugin(plugin)"
              />
              <span class="toggle-slider"></span>
            </label>
            <button class="action-btn" @click="openPlugin(plugin)">打开</button>
            <button class="action-btn danger" @click="uninstallPlugin(plugin)">卸载</button>
          </div>
        </div>
      </div>
    </div>

    <!-- 安装对话框 -->
    <div v-if="showInstallDialog" class="dialog-overlay" @click="showInstallDialog = false">
      <div class="dialog" @click.stop>
        <div class="dialog-header">
          <h3>安装插件</h3>
          <button class="close-btn" @click="showInstallDialog = false">×</button>
        </div>
        <div class="dialog-body">
          <p>选择插件目录进行安装</p>
          <button class="select-btn" @click="selectPluginDir">选择目录</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { withNativeDialog } from '../composables/nativeDialog';

const emit = defineEmits<{
  (e: 'back'): void;
  (e: 'open', pluginId: string, featureId: string): void;
}>();

interface Feature {
  id: string;
  name: string;
  keywords: string[];
  icon?: string;
  description?: string;
}

interface Plugin {
  id: string;
  name: string;
  version: string;
  description?: string;
  icon?: string;
  features: Feature[];
  permissions: string[];
  enabled: boolean;
}

const plugins = ref<Plugin[]>([]);
const loading = ref(true);
const showInstallDialog = ref(false);

// 加载插件列表
async function loadPlugins() {
  loading.value = true;
  try {
    const list = await invoke<Plugin[]>('list_plugins');
    // 添加 enabled 字段（默认启用）
    plugins.value = list.map(p => ({ ...p, enabled: true }));
  } catch (e) {
    console.error('Failed to load plugins:', e);
  } finally {
    loading.value = false;
  }
}

// 切换插件启用状态
async function togglePlugin(plugin: Plugin) {
  plugin.enabled = !plugin.enabled;
  // TODO: 调用后端 API 保存启用状态
}

// 打开插件
function openPlugin(plugin: Plugin) {
  if (plugin.features.length > 0) {
    emit('open', plugin.id, plugin.features[0].id);
  }
}

// 卸载插件
async function uninstallPlugin(plugin: Plugin) {
  if (!confirm(`确定要卸载插件 "${plugin.name}" 吗？`)) {
    return;
  }
  
  try {
    await invoke('uninstall_plugin', { id: plugin.id });
    await loadPlugins();
  } catch (e) {
    console.error('Failed to uninstall plugin:', e);
    alert(`卸载失败：${e}`);
  }
}

// 选择插件目录
async function selectPluginDir() {
  try {
    // 原生面板期间抑制失焦隐藏（withNativeDialog）
    const selected = await withNativeDialog(() => invoke<string | null>('fs_pick_folder'));
    if (selected) {
      await invoke('install_plugin_from_dir', { sourceDir: selected });
      await loadPlugins();
      showInstallDialog.value = false;
    }
  } catch (e) {
    console.error('Failed to install plugin:', e);
    alert(`安装失败：${e}`);
  }
}

onMounted(() => {
  loadPlugins();
});
</script>

<style scoped>
.plugin-manager {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
}

.header {
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

.install-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  font-size: 13px;
  font-weight: 500;
  border: none;
  border-radius: 6px;
  background: var(--accent-color);
  color: white;
  cursor: pointer;
  transition: opacity 0.2s;
}

.install-btn:hover {
  opacity: 0.9;
}

.plugin-list {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
}

.loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 200px;
  color: var(--text-secondary);
  gap: 12px;
}

.spinner {
  width: 24px;
  height: 24px;
  border: 2px solid var(--border-color);
  border-top-color: var(--accent-color);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 200px;
  color: var(--text-tertiary);
}

.empty-icon {
  font-size: 48px;
  margin-bottom: 12px;
}

.empty-text {
  font-size: 15px;
  font-weight: 500;
  color: var(--text-secondary);
  margin-bottom: 4px;
}

.empty-desc {
  font-size: 13px;
}

.list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.plugin-item {
  display: flex;
  align-items: center;
  padding: 16px;
  border: 1px solid var(--border-color);
  border-radius: 10px;
  background: var(--bg-primary);
  transition: all 0.2s;
}

.plugin-item:hover {
  border-color: var(--accent-color);
  background: var(--bg-secondary);
}

.plugin-item.disabled {
  opacity: 0.6;
}

.plugin-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 48px;
  height: 48px;
  margin-right: 16px;
  border-radius: 10px;
  background: var(--bg-secondary);
  color: var(--text-secondary);
  overflow: hidden;
}

.plugin-icon img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.plugin-info {
  flex: 1;
  min-width: 0;
}

.plugin-name {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 4px;
}

.plugin-meta {
  display: flex;
  gap: 12px;
  font-size: 12px;
  color: var(--text-tertiary);
  margin-bottom: 4px;
}

.version {
  color: var(--accent-color);
}

.plugin-desc {
  font-size: 13px;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.plugin-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-left: 16px;
}

.toggle {
  position: relative;
  display: inline-block;
  width: 44px;
  height: 24px;
  margin-right: 8px;
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

.action-btn {
  padding: 6px 12px;
  font-size: 13px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-primary);
  color: var(--text-primary);
  cursor: pointer;
  transition: all 0.2s;
}

.action-btn:hover {
  background: var(--hover-bg);
}

.action-btn.danger {
  color: var(--danger-color);
  border-color: var(--danger-color);
}

.action-btn.danger:hover {
  background: var(--danger-color);
  color: white;
}

/* 对话框 */
.dialog-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}

.dialog {
  width: 400px;
  background: var(--bg-primary);
  border-radius: 12px;
  box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.1);
}

.dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-color);
}

.dialog-header h3 {
  font-size: 16px;
  font-weight: 600;
}

.close-btn {
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  font-size: 20px;
  color: var(--text-tertiary);
  cursor: pointer;
}

.dialog-body {
  padding: 20px;
  text-align: center;
}

.dialog-body p {
  color: var(--text-secondary);
  margin-bottom: 16px;
}

.select-btn {
  padding: 10px 24px;
  font-size: 14px;
  font-weight: 500;
  border: none;
  border-radius: 8px;
  background: var(--accent-color);
  color: white;
  cursor: pointer;
}
</style>