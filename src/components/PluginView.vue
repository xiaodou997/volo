/**
 * 插件视图组件
 * 在主窗口内加载和显示插件内容
 */

<template>
  <div class="plugin-view">
    <div v-if="loading" class="plugin-loading">
      <div class="loading-spinner"></div>
      <span>加载中...</span>
    </div>
    <iframe
      v-else-if="pluginHtml"
      ref="pluginFrame"
      :srcdoc="pluginHtml"
      class="plugin-frame"
      sandbox="allow-scripts"
      @load="onPluginLoad"
    ></iframe>
    <div v-else-if="error" class="plugin-error">
      <span>{{ error }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { PLUGIN_CLIENT_SCRIPT } from '../bridge/pluginClient';
import { createPluginHost, type PluginHost } from '../bridge/pluginHost';

const props = defineProps<{
  pluginId: string;
  featureId: string;
  query?: string;
}>();

const emit = defineEmits<{
  (e: 'ready'): void;
  (e: 'error', message: string): void;
  (e: 'exit'): void;
  (e: 'subInputShow', data: { placeholder?: string }): void;
  (e: 'subInputHide'): void;
  (e: 'subInputSetValue', text: string): void;
}>();

const pluginFrame = ref<HTMLIFrameElement>();
const pluginHtml = ref<string>('');
const loading = ref(true);
const error = ref<string>('');

// 宿主侧 postMessage 桥（iframe 渲染后惰性创建）
let host: PluginHost | null = null;

function ensureHost(): PluginHost | null {
  if (host || !pluginFrame.value) return host;
  host = createPluginHost(pluginFrame.value, props.pluginId, {
    onExit: () => emit('exit'),
    onResize: (height) => {
      invoke('set_window_height', { height });
    },
    onSubInputShow: (data) => emit('subInputShow', data),
    onSubInputHide: () => emit('subInputHide'),
    onSubInputSetValue: (text) => emit('subInputSetValue', text),
  });
  return host;
}

// 把客户端 shim 注入插件 HTML 的 <head> 最前（脚本为纯 ASCII；
// 若 head 内有 charset meta 则插到其后，避免破坏编码嗅探）
function injectClientScript(html: string): string {
  const script = '<script>' + PLUGIN_CLIENT_SCRIPT + '</scr' + 'ipt>';
  const headMatch = /<head[^>]*>/i.exec(html);
  if (headMatch) {
    const headEnd = headMatch.index + headMatch[0].length;
    const charsetMatch = /<meta[^>]*charset[^>]*>/i.exec(
      html.slice(headEnd, headEnd + 1024),
    );
    const insertAt = charsetMatch
      ? headEnd + charsetMatch.index + charsetMatch[0].length
      : headEnd;
    return html.slice(0, insertAt) + script + html.slice(insertAt);
  }
  return script + html;
}

// 加载插件
async function loadPlugin() {
  loading.value = true;
  error.value = '';

  try {
    // 获取插件 HTML 内容并注入桥接脚本
    const html = await invoke<string>('get_plugin_html', {
      pluginId: props.pluginId,
    });
    pluginHtml.value = injectClientScript(html);
  } catch (e) {
    error.value = String(e);
    emit('error', String(e));
  } finally {
    loading.value = false;
  }
}

// 插件加载完成
function onPluginLoad() {
  // 触发 onPluginEnter
  ensureHost()?.sendEvent('onPluginEnter', {
    query: props.query || '',
    code: props.featureId,
  });

  emit('ready');
}

// 处理来自插件的消息
function handleMessage(event: MessageEvent) {
  ensureHost()?.handleMessage(event);
}

// 处理来自父组件的子输入框变化
function handleSubInputChange(text: string) {
  ensureHost()?.sendEvent('subInputChange', { text });
}

// 监听 query 变化
watch(() => props.query, (newQuery) => {
  if (newQuery !== undefined) {
    ensureHost()?.sendEvent('onQueryChange', { query: newQuery });
  }
});

onMounted(() => {
  loadPlugin();
  window.addEventListener('message', handleMessage);
});

onUnmounted(() => {
  window.removeEventListener('message', handleMessage);
  // 触发 onPluginOut
  host?.sendEvent('onPluginOut');
  host?.dispose();
  host = null;
});

// 暴露方法
defineExpose({
  handleSubInputChange,
});
</script>

<style scoped>
.plugin-view {
  width: 100%;
  height: 100%;
  background: var(--bg-primary);
}

.plugin-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 200px;
  color: var(--text-secondary);
  gap: 12px;
}

.loading-spinner {
  width: 24px;
  height: 24px;
  border: 2px solid var(--border-color);
  border-top-color: var(--accent-color);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.plugin-frame {
  width: 100%;
  height: 100%;
  border: none;
  background: transparent;
}

.plugin-error {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 200px;
  color: var(--text-tertiary);
}
</style>