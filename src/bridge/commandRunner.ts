/**
 * Command（no-view）命令执行器
 *
 * 在隐藏 iframe 中执行插件声明的无界面命令：
 *   1. invoke get_plugin_command_source 获取命令 JS 源码
 *   2. 创建 sandbox 隐藏 iframe，srcdoc = shim + 插件源码 + bootstrap
 *   3. createPluginHost 接管消息（pluginId 自动附加，权限审批照常弹窗）
 *   4. 收到 command-done / 10s 超时后销毁：dispose host、移除 iframe、清理监听
 *
 * 出错（源码加载失败、命令抛错、超时）通过系统通知告知用户。
 *
 * 另提供 runListCommand：list 模式命令（iframe 常驻，setList 推送结果、
 * command-select 触发选中动作、done 结束，无超时），见文件下方。
 */

import { invoke } from '@tauri-apps/api/core';
import { PLUGIN_CLIENT_SCRIPT } from './pluginClient';
import { createPluginHost } from './pluginHost';

const COMMAND_TIMEOUT_MS = 10_000;

/** 主窗口自用的错误通知（不附加 pluginId） */
function notifyError(body: string) {
  invoke('notification_show', {
    options: { title: '命令执行失败', body },
  }).catch(() => {});
}

// 末尾脚本：此时 shim 与插件源码已按序执行完毕，直接触发已注册的 onRun 回调；
// 同步异常与 Promise rejection 都上报为 command-done（带 error）
function buildBootstrap(query: string): string {
  return (
    '(function () {' +
    'try {' +
    'var r = window.rubick;' +
    'var p = r.command.__trigger(' + JSON.stringify(query) + ');' +
    'if (p && typeof p.catch === "function") {' +
    'p.catch(function (e) { r.command.done(e); });' +
    '}' +
    '} catch (e) {' +
    'if (window.rubick && window.rubick.command) { window.rubick.command.done(e); }' +
    '}' +
    '})();'
  );
}

function buildSrcdoc(source: string, query: string): string {
  const endTag = '</scr' + 'ipt>';
  return (
    '<script>' + PLUGIN_CLIENT_SCRIPT + endTag +
    '<script>' + source + endTag +
    '<script>' + buildBootstrap(query) + endTag
  );
}

export async function runCommand(
  pluginId: string,
  commandId: string,
  query: string,
): Promise<void> {  let source: string;
  try {
    source = await invoke<string>('get_plugin_command_source', { pluginId, commandId });
  } catch (e) {
    notifyError(`加载命令源码失败：${String(e)}`);
    return;
  }

  const iframe = document.createElement('iframe');
  iframe.setAttribute('sandbox', 'allow-scripts');
  iframe.style.display = 'none';
  iframe.srcdoc = buildSrcdoc(source, query);
  document.body.appendChild(iframe);

  let finished = false;

  const host = createPluginHost(iframe, pluginId, {
    onExit: () => finish(),
    onResize: () => {},
    onSubInputShow: () => {},
    onSubInputHide: () => {},
    onSubInputSetValue: () => {},
    onCommandDone: (error) => finish(error),
  });

  function onMessage(event: MessageEvent) {
    host.handleMessage(event);
  }

  function finish(error?: string) {
    if (finished) return;
    finished = true;
    window.clearTimeout(timer);
    window.removeEventListener('message', onMessage);
    host.dispose();
    iframe.remove();
    if (error) {
      notifyError(`命令 ${commandId} 出错：${error}`);
    }
  }

  window.addEventListener('message', onMessage);
  const timer = window.setTimeout(
    () => finish(`命令 ${commandId} 执行超时（10s）`),
    COMMAND_TIMEOUT_MS,
  );
}

/** list 模式命令推送到启动器的列表项 */
export interface ListCommandItem {
  id: string;
  title: string;
  description?: string;
  icon?: string;
}

export interface ListCommandCallbacks {
  /** 命令调 rubick.command.setList 时触发 */
  onList: (items: ListCommandItem[]) => void;
  /** 命令调 rubick.command.done 时触发；error 存在表示命令执行出错 */
  onDone: (error?: string) => void;
  /** 运行单元启动失败（源码加载失败等）时触发 */
  onError?: (error: string) => void;
}

export interface ListCommandHandle {
  /** 过滤词变化：重触发 onRun(query) */
  setQuery: (query: string) => void;
  /** 回车选中某项：触发命令的 onSelect(id) */
  select: (id: string) => void;
  /** 销毁运行单元（幂等） */
  destroy: () => void;
}

/**
 * List 模式命令执行器
 *
 * 与 runCommand 的差异：
 *   - 不自动 done、不适用 10s 超时：iframe 常驻，等待 setList / select / done
 *   - bootstrap 触发 onRun('')；之后通过 setQuery 发 'run' 事件重触发 onRun(query)
 *   - 回车选中通过 select 发 'command-select' 事件触发 onSelect(id)
 *   - 收到 command-done 后销毁并回调 onDone
 *
 * 源码加载失败时通知用户、回调 onError 并返回 null。
 */
export async function runListCommand(
  pluginId: string,
  commandId: string,
  callbacks: ListCommandCallbacks,
): Promise<ListCommandHandle | null> {
  let source: string;
  try {
    source = await invoke<string>('get_plugin_command_source', { pluginId, commandId });
  } catch (e) {
    notifyError(`加载命令源码失败：${String(e)}`);
    callbacks.onError?.(String(e));
    return null;
  }

  const iframe = document.createElement('iframe');
  iframe.setAttribute('sandbox', 'allow-scripts');
  iframe.style.display = 'none';
  iframe.srcdoc = buildSrcdoc(source, '');
  document.body.appendChild(iframe);

  let finished = false;

  const host = createPluginHost(iframe, pluginId, {
    onExit: () => finish(),
    onResize: () => {},
    onSubInputShow: () => {},
    onSubInputHide: () => {},
    onSubInputSetValue: () => {},
    onCommandDone: (error) => finish(error),
    onCommandSetList: (items) => {
      if (!finished) callbacks.onList(items as ListCommandItem[]);
    },
  });

  function onMessage(event: MessageEvent) {
    host.handleMessage(event);
  }

  function destroy() {
    if (finished) return;
    finished = true;
    window.removeEventListener('message', onMessage);
    host.dispose();
    iframe.remove();
  }

  function finish(error?: string) {
    if (finished) return;
    destroy();
    if (error) {
      notifyError(`命令 ${commandId} 出错：${error}`);
    }
    callbacks.onDone(error);
  }

  window.addEventListener('message', onMessage);

  return {
    setQuery: (query) => host.sendEvent('run', { query }),
    select: (id) => host.sendEvent('command-select', { id }),
    destroy,
  };
}
