/**
 * Volo API 封装
 * 提供给插件使用的 window.rubick API
 */

import { invoke } from '@tauri-apps/api/core';

// ============ 类型定义 ============

export interface Doc {
  _id: string;
  _rev?: string;
  data: any;
  updated_at?: number;
}

export interface NotificationOptions {
  title?: string;
  body: string;
  icon?: string;
}

export interface AppConfig {
  shortcut: string;
  theme: 'system' | 'light' | 'dark';
  hideOnBlur: boolean;
  language: string;
  showGuide: boolean;
}

export interface AppInfo {
  name: string;
  path: string;
  icon?: string;
  type: string;
  pinyin?: string;
  initials?: string;
}

export interface FeatureInfo {
  id: string;
  name: string;
  keywords: string[];
}

export interface CommandInfo {
  id: string;
  name: string;
  keywords: string[];
  description?: string;
  /** 命令模式：run（直接执行）或 list（列表模式） */
  mode: 'run' | 'list';
}

export interface PluginInfo {
  id: string;
  name: string;
  icon?: string;
  description?: string;
}

// AI 查询伪结果（本地追加，不进 Rust 搜索）；skill 为 @技能名 显式触发时解析出的技能名
export interface AiQueryResult {
  type: 'ai';
  query: string;
  skill?: string;
}

// @技能名 触发时的技能候选（选中后补全输入，不直接执行）
export interface SkillEntryResult {
  type: 'skill-entry';
  skill: SkillMeta;
}

// list 模式命令推送到启动器的列表项（本地注入，不进 Rust 搜索）
export interface ListCommandItem {
  id: string;
  title: string;
  description?: string;
  icon?: string;
  // 二级动作面板：选中项上按 Tab/→ 展开的可选动作；选中动作触发插件 onAction(itemId, actionId)
  actions?: ListCommandAction[];
}

// 列表项的二级动作
export interface ListCommandAction {
  id: string;
  title: string;
  description?: string;
  icon?: string;
}

export interface CommandItemResult {
  type: 'command-item';
  item: ListCommandItem;
}

export type SearchResult =
  | { type: 'app'; name: string; path: string; icon?: string; pinyin?: string; initials?: string }
  | { type: 'plugin'; plugin: PluginInfo; feature: FeatureInfo }
  | { type: 'command'; plugin: PluginInfo; command: CommandInfo }
  | { type: 'file'; path: string; name: string; file_type: string; extension?: string }
  | { type: 'ai-history' } // 空输入时的 AI 会话历史直达入口（本地追加，不进 Rust 搜索）
  | CommandItemResult
  | SkillEntryResult
  | AiQueryResult;

// ============ AI / Agent ============

// LLM 配置（空字符串 = 未配置）
export interface LlmConfig {
  baseUrl: string;
  model: string;
}

// MCP 服务器配置（stdio 本地子进程或 Streamable HTTP 远程服务，工具名形如 mcp__{server}__{tool}）
export interface McpServerConfig {
  command: string;
  args: string[];
  env: Record<string, string>;
  /** 远程 server URL（非空走 Streamable HTTP，忽略 command/args/env） */
  url?: string;
  enabled: boolean;
}

// agent-event 事件 payload
export interface AgentEvent {
  kind: 'message' | 'tool_call' | 'tool_result' | 'done' | 'error';
  content?: string;
  // delta: true 表示 content 是流式增量片段；不带 delta 的 message 是完整消息
  delta?: boolean;
  name?: string;
  args?: any;
  result?: string;
}

// 技能元数据（SKILL.md frontmatter；skill_list 返回）
export interface SkillMeta {
  name: string;
  description: string;
  version: string;
}

// agent_list_sessions 返回的会话元信息（按时间倒序）
export interface SessionMeta {
  id: string;
  startedAt: string;
  preview: string;
}

// agent_read_session 返回的历史事件（回放用）
export interface ReplayEvent {
  kind: 'user' | 'message' | 'tool_call' | 'tool_result' | 'error';
  content?: string;
  name?: string;
  args?: unknown;
  result?: string;
}

// 文件信息（来自索引）
export interface FileInfo {
  path: string;
  name: string;
  type: 'file' | 'directory';
  extension?: string;
  size: number;
  modified: number;
}

// 索引统计
export interface IndexStats {
  total_files: number;
  total_dirs: number;
  last_scan?: number;
  is_indexing: boolean;
}

// 文件选择选项
export interface PickOptions {
  multiple?: boolean;
  filters?: FileFilter[];
}

export interface FileFilter {
  name: string;
  extensions: string[];
}

// ============ 权限审批 ============

// 风险等级（与 Rust 端 RiskLevel 对应）
export type RiskLevel = 'Low' | 'Medium' | 'High' | 'Critical';

// 授权范围
export type PermissionScope = 'once' | 'session' | 'always';

// permission-request 事件 payload
export interface PermissionRequest {
  requestId: string;
  pluginId: string;
  capability: string;
  description: string;
  risk: RiskLevel;
  resource?: string;
}

// permission_list_grants 返回的授权记录
export interface PermissionGrant {
  pluginId: string;
  capability: string;
  scope: PermissionScope;
  risk: RiskLevel;
  description: string;
}

// ============ Rubick API ============

export interface RubickAPI {
  // 生命周期钩子
  hooks: {
    onPluginEnter?: (data: { query: string }) => void;
    onPluginReady?: () => void;
    onPluginOut?: () => void;
    onShow?: () => void;
    onHide?: () => void;
    onSubInputChange?: (text: string) => void;
  };

  // 窗口
  window: {
    hide: () => Promise<void>;
    show: () => Promise<void>;
    setSize: (height: number | { height: number; width?: number }) => Promise<void>;
  };

  // 剪贴板
  clipboard: {
    readText: () => Promise<string>;
    writeText: (text: string) => Promise<void>;
    readImage: () => Promise<string | null>;
    writeImage: (base64: string) => Promise<void>;
    readFiles: () => Promise<string[]>;
  };

  // 数据库
  db: {
    put: (id: string, data: any) => Promise<Doc>;
    get: (id: string) => Promise<Doc | null>;
    remove: (id: string) => Promise<void>;
    all: () => Promise<Doc[]>;
  };

  // 简化存储
  storage: {
    set: (key: string, value: any) => Promise<void>;
    get: (key: string) => Promise<any>;
    remove: (key: string) => Promise<void>;
  };

  // 通知
  notification: {
    show: (options: NotificationOptions | string) => Promise<void>;
  };

  // Shell
  shell: {
    open: (url: string) => Promise<void>;
    openPath: (path: string) => Promise<void>;
  };

  // 系统
  system: {
    platform: 'macos' | 'windows' | 'linux';
    darkMode: boolean;
    version: string;
  };

  // 子输入框
  subInput: {
    show: (placeholder?: string) => void;
    hide: () => void;
    setValue: (text: string) => void;
    onChange: (callback: (text: string) => void) => void;
  };

  // 截图
  screenCapture: () => Promise<string>;
  screenCaptureArea: () => Promise<string>;

  // 文件系统
  fs: {
    read: (path: string) => Promise<string>;
    readBinary: (path: string) => Promise<string>;
    write: (path: string, content: string) => Promise<void>;
    writeBinary: (path: string, content: string) => Promise<void>;
    exists: (path: string) => Promise<boolean>;
    mkdir: (path: string) => Promise<void>;
    remove: (path: string) => Promise<void>;
    list: (path: string) => Promise<FileInfo[]>;
    pickFile: (options?: PickOptions) => Promise<string | null>;
    pickFiles: (options?: PickOptions) => Promise<string[]>;
    pickFolder: () => Promise<string | null>;
  };
}

// ============ API 实现 ============

const rubick: RubickAPI = {
  hooks: {},

  window: {
    hide: () => invoke('hide_main_window'),
    show: () => invoke('show_main_window'),
    setSize: async (height) => {
      const h = typeof height === 'number' ? height : height.height;
      return invoke('set_window_height', { height: h });
    },
  },

  clipboard: {
    readText: () => invoke('clipboard_read_text'),
    writeText: (text) => invoke('clipboard_write_text', { text }),
    readImage: () => invoke<string | null>('clipboard_read_image'),
    writeImage: (base64) => invoke('clipboard_write_image', { base64 }),
    readFiles: () => invoke<string[]>('clipboard_read_files'),
  },

  db: {
    put: (id, data) => invoke('db_put', { id, data }),
    get: (id) => invoke('db_get', { id }),
    remove: (id) => invoke('db_remove', { id }),
    all: () => invoke('db_all'),
  },

  storage: {
    set: async (key, value) => {
      await invoke('db_put', { id: key, data: value });
    },
    get: async (key) => {
      const doc = await invoke<Doc | null>('db_get', { id: key });
      return doc?.data ?? null;
    },
    remove: (key) => invoke('db_remove', { id: key }),
  },

  notification: {
    show: (options) => {
      const opts = typeof options === 'string' 
        ? { body: options } 
        : options;
      return invoke('notification_show', { options: opts });
    },
  },

  shell: {
    open: (url) => invoke('shell_open', { url }),
    openPath: (path) => invoke('shell_open_path', { path }),
  },

  system: {
    get platform() {
      const p = navigator.platform.toLowerCase();
      if (p.includes('mac')) return 'macos';
      if (p.includes('win')) return 'windows';
      return 'linux';
    },
    get darkMode() {
      return window.matchMedia('(prefers-color-scheme: dark)').matches;
    },
    get version() {
      return '0.1.0';
    },
  },

  subInput: {
    show: (placeholder?: string) => {
      // 通过 postMessage 通知父组件
      window.parent.postMessage({ type: 'subInputShow', data: { placeholder } }, '*');
    },
    hide: () => {
      window.parent.postMessage({ type: 'subInputHide' }, '*');
    },
    setValue: (text: string) => {
      window.parent.postMessage({ type: 'subInputSetValue', data: { text } }, '*');
    },
    onChange: (_callback: (text: string) => void) => {
      // 回调在 PluginView 中处理
    },
  },

  screenCapture: () => invoke<string>('screen_capture'),
  screenCaptureArea: () => invoke<string>('screen_capture_area'),

  fs: {
    read: (path) => invoke<string>('fs_read', { path }),
    readBinary: (path) => invoke<string>('fs_read_binary', { path }),
    write: (path, content) => invoke('fs_write', { path, content }),
    writeBinary: (path, content) => invoke('fs_write_binary', { path, content }),
    exists: (path) => invoke<boolean>('fs_exists', { path }),
    mkdir: (path) => invoke('fs_mkdir', { path }),
    remove: (path) => invoke('fs_remove', { path }),
    list: (path) => invoke<FileInfo[]>('fs_list', { path }),
    pickFile: (options) => invoke<string | null>('fs_pick_file', { options }),
    pickFiles: (options) => invoke<string[]>('fs_pick_files', { options }),
    pickFolder: () => invoke<string | null>('fs_pick_folder'),
  },
};

// ============ 挂载到全局 ============

declare global {
  interface Window {
    rubick: RubickAPI;
  }
}

window.rubick = rubick;

export default rubick;
