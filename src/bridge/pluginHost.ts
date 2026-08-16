/**
 * 宿主端 postMessage 桥（父窗口 / 可信侧）
 *
 * 校验 event.source === iframe.contentWindow，将插件的 API 调用映射为
 * Tauri invoke，并自动附加 pluginId（插件自身无法伪造身份）。
 * 回传统一使用 postMessage '*'：iframe 为 opaque origin，无法预知 origin，
 * 安全性由 source 校验和结构化消息字段保证。
 */

import { invoke } from '@tauri-apps/api/core';

export interface PluginHostHandlers {
  onExit: () => void;
  onResize: (height: number) => void;
  onSubInputShow: (data: { placeholder?: string }) => void;
  onSubInputHide: () => void;
  onSubInputSetValue: (text: string) => void;
  /** Command（no-view）命令完成；error 存在表示命令执行出错 */
  onCommandDone?: (error?: string) => void;
  /** Tool 工具调用完成；ok 为 true 时 data 为返回值，否则 error 为错误文本 */
  onToolDone?: (ok: boolean, data?: any, error?: string) => void;
}

export interface PluginHost {
  handleMessage: (event: MessageEvent) => boolean;
  sendEvent: (type: string, data?: any) => void;
  dispose: () => void;
}

interface MethodEntry {
  command: string;
  /** 是否需要自动附加 pluginId（插件面命令） */
  attachPluginId?: boolean;
}

// method -> Tauri 命令映射表
const METHOD_MAP: Record<string, MethodEntry> = {
  // 窗口（宿主自身命令，不属于插件能力面，不附加 pluginId）
  'window.hide': { command: 'hide_main_window' },
  'window.show': { command: 'show_main_window' },
  'window.setSize': { command: 'set_window_height' },

  // 剪贴板
  'clipboard.readText': { command: 'clipboard_read_text', attachPluginId: true },
  'clipboard.writeText': { command: 'clipboard_write_text', attachPluginId: true },
  'clipboard.readImage': { command: 'clipboard_read_image', attachPluginId: true },
  'clipboard.writeImage': { command: 'clipboard_write_image', attachPluginId: true },
  'clipboard.readFiles': { command: 'clipboard_read_files', attachPluginId: true },

  // 数据库
  'db.put': { command: 'db_put', attachPluginId: true },
  'db.get': { command: 'db_get', attachPluginId: true },
  'db.remove': { command: 'db_remove', attachPluginId: true },
  'db.all': { command: 'db_all', attachPluginId: true },

  // 通知
  'notification.show': { command: 'notification_show', attachPluginId: true },

  // Shell
  'shell.open': { command: 'shell_open', attachPluginId: true },
  'shell.openPath': { command: 'shell_open_path', attachPluginId: true },

  // 截图
  screenCapture: { command: 'screen_capture', attachPluginId: true },
  screenCaptureArea: { command: 'screen_capture_area', attachPluginId: true },

  // 文件系统
  'fs.read': { command: 'fs_read', attachPluginId: true },
  'fs.readBinary': { command: 'fs_read_binary', attachPluginId: true },
  'fs.write': { command: 'fs_write', attachPluginId: true },
  'fs.writeBinary': { command: 'fs_write_binary', attachPluginId: true },
  'fs.exists': { command: 'fs_exists', attachPluginId: true },
  'fs.mkdir': { command: 'fs_mkdir', attachPluginId: true },
  'fs.remove': { command: 'fs_remove', attachPluginId: true },
  'fs.list': { command: 'fs_list', attachPluginId: true },
  'fs.pickFile': { command: 'fs_pick_file', attachPluginId: true },
  'fs.pickFiles': { command: 'fs_pick_files', attachPluginId: true },
  'fs.pickFolder': { command: 'fs_pick_folder', attachPluginId: true },
};

export function createPluginHost(
  iframe: HTMLIFrameElement,
  pluginId: string,
  handlers: PluginHostHandlers,
): PluginHost {
  let disposed = false;

  function post(message: any) {
    iframe.contentWindow?.postMessage(message, '*');
  }

  async function handleApi(msg: any) {
    const base = { source: 'volo-host', kind: 'api-result', reqId: msg.reqId };
    const entry = METHOD_MAP[msg.method];

    if (!entry) {
      post({ ...base, ok: false, error: `Unknown method: ${String(msg.method)}` });
      return;
    }

    try {
      const args = entry.attachPluginId
        ? { ...(msg.args || {}), pluginId }
        : msg.args || {};
      const data = await invoke(entry.command, args);
      post({ ...base, ok: true, data });
    } catch (e) {
      post({ ...base, ok: false, error: String(e) });
    }
  }

  function handleSubInput(action: string, data: any) {
    switch (action) {
      case 'show':
        handlers.onSubInputShow(data || {});
        break;
      case 'hide':
        handlers.onSubInputHide();
        break;
      case 'setValue':
        handlers.onSubInputSetValue(data?.text ?? '');
        break;
    }
  }

  function handleMessage(event: MessageEvent): boolean {
    if (disposed) return false;
    // 只接受来自本插件 iframe 的消息（opaque origin 下 origin 恒为 'null'，靠 source 校验）
    if (event.source !== iframe.contentWindow) return false;

    const msg = event.data;
    if (!msg || typeof msg !== 'object') return true;

    // 新协议消息
    if (msg.source === 'volo-plugin') {
      switch (msg.kind) {
        case 'api':
          void handleApi(msg);
          break;
        case 'subInput':
          handleSubInput(msg.action, msg.data);
          break;
        case 'exit':
          handlers.onExit();
          break;
        case 'command-done':
          handlers.onCommandDone?.(
            typeof msg.data?.error === 'string' ? msg.data.error : undefined,
          );
          break;
        case 'tool-done':
          handlers.onToolDone?.(
            !!msg.ok,
            msg.data,
            typeof msg.error === 'string' ? msg.error : undefined,
          );
          break;
      }
      return true;
    }

    // 旧协议消息（存量插件直接 postMessage 的裸 { type, data }）
    switch (msg.type) {
      case 'pluginExit':
        handlers.onExit();
        break;
      case 'setWindowSize':
        handlers.onResize(msg.data?.height || 400);
        break;
      case 'subInputShow':
        handlers.onSubInputShow(msg.data || {});
        break;
      case 'subInputHide':
        handlers.onSubInputHide();
        break;
      case 'subInputSetValue':
        handlers.onSubInputSetValue(msg.data?.text ?? '');
        break;
    }
    return true;
  }

  function sendEvent(type: string, data?: any) {
    if (disposed) return;
    post({ source: 'volo-host', kind: 'event', type, data });
  }

  function dispose() {
    disposed = true;
  }

  return { handleMessage, sendEvent, dispose };
}
