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
      sandbox="allow-scripts allow-same-origin"
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

const props = defineProps<{
  pluginId: string;
  featureId: string;
  query?: string;
}>();

const emit = defineEmits<{
  (e: 'ready'): void;
  (e: 'error', message: string): void;
  (e: 'exit'): void;
}>();

const pluginFrame = ref<HTMLIFrameElement>();
const pluginHtml = ref<string>('');
const loading = ref(true);
const error = ref<string>('');

// 加载插件
async function loadPlugin() {
  loading.value = true;
  error.value = '';

  try {
    // 获取插件 HTML 内容
    const html = await invoke<string>('get_plugin_html', {
      pluginId: props.pluginId,
    });
    pluginHtml.value = html;
  } catch (e) {
    error.value = String(e);
    emit('error', String(e));
  } finally {
    loading.value = false;
  }
}

// 插件加载完成
function onPluginLoad() {
  // 注入 API
  injectRubickAPI();

  // 触发 onPluginEnter
  sendMessage({
    type: 'onPluginEnter',
    data: {
      query: props.query || '',
      code: props.featureId,
    },
  });

  emit('ready');
}

// 注入 rubick API
function injectRubickAPI() {
  if (!pluginFrame.value?.contentWindow) return;

  const win = pluginFrame.value.contentWindow;

  // 创建 rubick API 对象
  const rubickAPI = {
    // 生命周期钩子
    onPluginEnter: null as ((data: any) => void) | null,
    onPluginReady: null as (() => void) | null,
    onPluginOut: null as (() => void) | null,

    // 窗口
    window: {
      hide: () => invoke('hide_main_window'),
      show: () => invoke('show_main_window'),
      setSize: (height: number) => invoke('set_window_height', { height }),
    },

    // 剪贴板
    clipboard: {
      readText: () => invoke('clipboard_read_text'),
      writeText: (text: string) => invoke('clipboard_write_text', { text }),
    },

    // 数据库
    db: {
      put: (id: string, data: any) => invoke('db_put', { id, data }),
      get: (id: string) => invoke('db_get', { id }),
      remove: (id: string) => invoke('db_remove', { id }),
      all: () => invoke('db_all'),
    },

    // 存储
    storage: {
      set: (key: string, value: any) => invoke('db_put', { id: key, data: value }),
      get: (key: string) => invoke('db_get', { id: key }).then((doc: any) => doc?.data ?? null),
      remove: (key: string) => invoke('db_remove', { id: key }),
    },

    // 通知
    notification: {
      show: (options: any) => {
        const opts = typeof options === 'string' ? { body: options } : options;
        return invoke('notification_show', { options: opts });
      },
    },

    // Shell
    shell: {
      open: (url: string) => invoke('shell_open', { url }),
      openPath: (path: string) => invoke('shell_open_path', { path }),
    },

    // 系统
    system: {
      platform: navigator.platform.toLowerCase().includes('mac') ? 'macos' :
                navigator.platform.toLowerCase().includes('win') ? 'windows' : 'linux',
      darkMode: window.matchMedia('(prefers-color-scheme: dark)').matches,
      version: '0.1.0',
    },
  };

  // 注入到 iframe
  (win as any).rubick = rubickAPI;
}

// 发送消息到插件
function sendMessage(message: any) {
  if (!pluginFrame.value?.contentWindow) return;

  pluginFrame.value.contentWindow.postMessage(message, '*');
}

// 处理来自插件的消息
function handleMessage(event: MessageEvent) {
  const { type, data } = event.data || {};

  switch (type) {
    case 'pluginExit':
      emit('exit');
      break;
    case 'setWindowSize':
      invoke('set_window_height', { height: data?.height || 400 });
      break;
  }
}

// 监听 query 变化
watch(() => props.query, (newQuery) => {
  if (newQuery !== undefined) {
    sendMessage({
      type: 'onQueryChange',
      data: { query: newQuery },
    });
  }
});

onMounted(() => {
  loadPlugin();
  window.addEventListener('message', handleMessage);
});

onUnmounted(() => {
  window.removeEventListener('message', handleMessage);
  // 触发 onPluginOut
  sendMessage({ type: 'onPluginOut' });
});

// 暴露方法
defineExpose({
  sendMessage,
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