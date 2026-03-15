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

  // 子输入框 API
  const subInputAPI = {
    show: (placeholder?: string) => {
      sendMessage({ type: 'subInputShow', data: { placeholder } });
    },
    hide: () => {
      sendMessage({ type: 'subInputHide' });
    },
    setValue: (text: string) => {
      sendMessage({ type: 'subInputSetValue', data: { text } });
    },
    onChange: (callback: (text: string) => void) => {
      // 存储回调
      (win as any).__subInputCallback = callback;
    },
  };

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
      readImage: () => invoke<string | null>('clipboard_read_image'),
      writeImage: (base64: string) => invoke('clipboard_write_image', { base64 }),
      readFiles: () => invoke<string[]>('clipboard_read_files'),
    },

    // 数据库
    db: {
      put: (id: string, data: any) => invoke('db_put', { pluginId: props.pluginId, id, data }),
      get: (id: string) => invoke('db_get', { pluginId: props.pluginId, id }),
      remove: (id: string) => invoke('db_remove', { pluginId: props.pluginId, id }),
      all: () => invoke('db_all', { pluginId: props.pluginId }),
    },

    // 存储
    storage: {
      set: (key: string, value: any) => invoke('db_put', { pluginId: props.pluginId, id: key, data: value }),
      get: (key: string) => invoke('db_get', { pluginId: props.pluginId, id: key }).then((doc: any) => doc?.data ?? null),
      remove: (key: string) => invoke('db_remove', { pluginId: props.pluginId, id: key }),
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

    // 子输入框
    subInput: subInputAPI,

    // 截图
    screenCapture: async () => {
      const result = await invoke<string>('screen_capture');
      return result;
    },

    // 截取选定区域
    screenCaptureArea: async () => {
      const result = await invoke<string>('screen_capture_area');
      return result;
    },

    // 文件系统
    fs: {
      read: (path: string) => invoke<string>('fs_read', { path }),
      readBinary: (path: string) => invoke<string>('fs_read_binary', { path }),
      write: (path: string, content: string) => invoke('fs_write', { path, content }),
      writeBinary: (path: string, content: string) => invoke('fs_write_binary', { path, content }),
      exists: (path: string) => invoke<boolean>('fs_exists', { path }),
      mkdir: (path: string) => invoke('fs_mkdir', { path }),
      remove: (path: string) => invoke('fs_remove', { path }),
      list: (path: string) => invoke('fs_list', { path }),
      pickFile: (options?: { multiple?: boolean; filters?: { name: string; extensions: string[] }[] }) =>
        invoke<string | null>('fs_pick_file', { options }),
      pickFiles: (options?: { filters?: { name: string; extensions: string[] }[] }) =>
        invoke<string[]>('fs_pick_files', { options }),
      pickFolder: () => invoke<string | null>('fs_pick_folder'),
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
    case 'subInputShow':
      emit('subInputShow' as any, data);
      break;
    case 'subInputHide':
      emit('subInputHide' as any);
      break;
    case 'subInputSetValue':
      emit('subInputSetValue' as any, data?.text);
      break;
  }
}

// 处理来自父组件的子输入框变化
function handleSubInputChange(text: string) {
  if (pluginFrame.value?.contentWindow) {
    const win = pluginFrame.value.contentWindow as any;
    if (win.__subInputCallback) {
      win.__subInputCallback(text);
    }
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