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
): Promise<void> {
  let source: string;
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
