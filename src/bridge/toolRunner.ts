/**
 * 插件工具执行器（Tool）
 *
 * Agent（Rust 侧）调用插件工具的完整生命周期：
 *   1. Rust emit 'plugin-tool-call' { requestId, pluginId, toolId, args }；
 *      initToolRunner() 监听到后，每个调用并发独立处理
 *   2. invoke get_plugin_tool_source 获取工具 JS 源码；失败直接回 ok:false
 *   3. 创建 sandbox 隐藏 iframe，srcdoc = shim + 插件源码 + bootstrap
 *      （bootstrap 调 rubick.tool.__invoke(JSON 字符串化的 args)）
 *   4. createPluginHost 接管消息（pluginId 自动附加，权限审批照常弹窗）
 *   5. 收到 tool-done 后 invoke plugin_tool_result 回传结果/错误并销毁；
 *      30s 超时按工具错误回传
 *
 * 与 commandRunner 的差别只在于有输入输出、结果回传给 Rust 而不是弹通知。
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { PLUGIN_CLIENT_SCRIPT } from './pluginClient';
import { createPluginHost } from './pluginHost';

const TOOL_TIMEOUT_MS = 30_000;

interface PluginToolCall {
  requestId: string;
  pluginId: string;
  toolId: string;
  args: unknown;
}

// 末尾脚本：此时 shim 与插件源码已按序执行完毕，直接触发已注册的 onInvoke 回调；
// 输入是 JSON 字符串，解析与返回值上报都在 shim 的 __invoke 内完成
function buildBootstrap(inputJson: string): string {
  return (
    '(function () {' +
    'var r = window.rubick;' +
    'if (r && r.tool) { r.tool.__invoke(' + JSON.stringify(inputJson) + '); }' +
    '})();'
  );
}

function buildSrcdoc(source: string, args: unknown): string {
  const endTag = '</scr' + 'ipt>';
  // '<' 转义为 <：防止 args 文本里出现 "</script>" 之类提前闭合 srcdoc 脚本
  const inputJson = JSON.stringify(args ?? {}).replace(/</g, '\\u003c');
  return (
    '<script>' + PLUGIN_CLIENT_SCRIPT + endTag +
    '<script>' + source + endTag +
    '<script>' + buildBootstrap(inputJson) + endTag
  );
}

async function runTool(call: PluginToolCall): Promise<void> {
  function reply(ok: boolean, result?: unknown, error?: string) {
    const payload: Record<string, unknown> = ok
      ? { requestId: call.requestId, ok, result: result ?? null }
      : { requestId: call.requestId, ok, error: error ?? '工具执行失败' };
    invoke('plugin_tool_result', payload).catch(() => {});
  }

  let source: string;
  try {
    source = await invoke<string>('get_plugin_tool_source', {
      pluginId: call.pluginId,
      toolId: call.toolId,
    });
  } catch (e) {
    reply(false, undefined, `加载工具源码失败：${String(e)}`);
    return;
  }

  const iframe = document.createElement('iframe');
  iframe.setAttribute('sandbox', 'allow-scripts');
  iframe.style.display = 'none';
  iframe.srcdoc = buildSrcdoc(source, call.args);
  document.body.appendChild(iframe);

  let finished = false;

  const host = createPluginHost(iframe, call.pluginId, {
    onExit: () => finish(undefined, '插件请求退出'),
    onResize: () => {},
    onSubInputShow: () => {},
    onSubInputHide: () => {},
    onSubInputSetValue: () => {},
    onToolDone: (ok, data, error) =>
      ok ? finish(data) : finish(undefined, error || '工具执行失败'),
  });

  function onMessage(event: MessageEvent) {
    host.handleMessage(event);
  }

  function finish(result?: unknown, error?: string) {
    if (finished) return;
    finished = true;
    window.clearTimeout(timer);
    window.removeEventListener('message', onMessage);
    host.dispose();
    iframe.remove();
    if (error !== undefined) {
      reply(false, undefined, error);
    } else {
      reply(true, result);
    }
  }

  window.addEventListener('message', onMessage);
  const timer = window.setTimeout(
    () => finish(undefined, `工具 ${call.toolId} 执行超时（30s）`),
    TOOL_TIMEOUT_MS,
  );
}

/** 监听 Rust 侧的插件工具调用事件；返回 unlisten（App 生命周期内常驻） */
export function initToolRunner(): Promise<UnlistenFn> {
  return listen<PluginToolCall>('plugin-tool-call', (event) => {
    void runTool(event.payload);
  });
}
