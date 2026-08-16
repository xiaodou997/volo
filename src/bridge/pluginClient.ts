/**
 * 插件端 postMessage 客户端（iframe 内运行的 shim）
 *
 * PLUGIN_CLIENT_SCRIPT 会被 PluginView 原文注入插件 HTML 的 <script> 标签中，
 * 因此必须是纯内联 JS：不能 import 任何东西，且必须是纯 ASCII
 * （注入位置可能先于 <meta charset>，非 ASCII 字符会破坏编码嗅探）。
 *
 * 协议：
 *   客户端 -> 宿主: { source: 'volo-plugin', kind: 'api', reqId, method, args }
 *                   { source: 'volo-plugin', kind: 'subInput', action, data }
 *                   { source: 'volo-plugin', kind: 'exit' }
 *   宿主 -> 客户端: { source: 'volo-host', kind: 'api-result', reqId, ok, data, error }
 *                   { source: 'volo-host', kind: 'event', type, data }
 */
export const PLUGIN_CLIENT_SCRIPT = `(function () {
  if (window.rubick && window.rubick.__isVoloBridge) return;

  var reqSeq = 0;
  var pending = {};
  var subInputCallback = null;

  function call(method, args) {
    return new Promise(function (resolve, reject) {
      var reqId = 'req-' + (++reqSeq) + '-' + Date.now();
      pending[reqId] = { resolve: resolve, reject: reject };
      window.parent.postMessage({
        source: 'volo-plugin',
        kind: 'api',
        reqId: reqId,
        method: method,
        args: args || {}
      }, '*');
    });
  }

  window.addEventListener('message', function (event) {
    var msg = event.data;
    if (!msg || msg.source !== 'volo-host') return;

    if (msg.kind === 'api-result') {
      var p = pending[msg.reqId];
      if (!p) return;
      delete pending[msg.reqId];
      if (msg.ok) {
        p.resolve(msg.data);
      } else {
        p.reject(new Error(typeof msg.error === 'string' ? msg.error : String(msg.error)));
      }
      return;
    }

    if (msg.kind === 'event') {
      var r = window.rubick;
      if (!r) return;
      if (msg.type === 'subInputChange') {
        if (typeof subInputCallback === 'function') {
          subInputCallback(msg.data && msg.data.text);
        }
        return;
      }
      var hook = r[msg.type];
      if (typeof hook === 'function') {
        hook(msg.data);
      }
    }
  });

  window.rubick = {
    __isVoloBridge: true,

    // lifecycle hooks, assigned by the plugin, triggered by host events
    onPluginEnter: null,
    onPluginReady: null,
    onPluginOut: null,
    onQueryChange: null,

    // window
    window: {
      hide: function () { return call('window.hide'); },
      show: function () { return call('window.show'); },
      setSize: function (height) {
        var h = typeof height === 'number' ? height : (height && height.height);
        return call('window.setSize', { height: h });
      }
    },

    // clipboard
    clipboard: {
      readText: function () { return call('clipboard.readText'); },
      writeText: function (text) { return call('clipboard.writeText', { text: text }); },
      readImage: function () { return call('clipboard.readImage'); },
      writeImage: function (base64) { return call('clipboard.writeImage', { base64: base64 }); },
      readFiles: function () { return call('clipboard.readFiles'); }
    },

    // database (pluginId is attached by the host, never sent by the client)
    db: {
      put: function (id, data) { return call('db.put', { id: id, data: data }); },
      get: function (id) { return call('db.get', { id: id }); },
      remove: function (id) { return call('db.remove', { id: id }); },
      all: function () { return call('db.all'); }
    },

    // simplified storage
    storage: {
      set: function (key, value) { return call('db.put', { id: key, data: value }); },
      get: function (key) {
        return call('db.get', { id: key }).then(function (doc) {
          return doc && doc.data !== undefined ? doc.data : null;
        });
      },
      remove: function (key) { return call('db.remove', { id: key }); }
    },

    // notification
    notification: {
      show: function (options) {
        var opts = typeof options === 'string' ? { body: options } : options;
        return call('notification.show', { options: opts });
      }
    },

    // Shell
    shell: {
      open: function (url) { return call('shell.open', { url: url }); },
      openPath: function (path) { return call('shell.openPath', { path: path }); }
    },

    // system info (computed locally in the iframe, no bridge needed)
    system: {
      get platform() {
        var p = (navigator.platform || '').toLowerCase();
        if (p.indexOf('mac') !== -1) return 'macos';
        if (p.indexOf('win') !== -1) return 'windows';
        return 'linux';
      },
      get darkMode() {
        return window.matchMedia('(prefers-color-scheme: dark)').matches;
      },
      get version() {
        return '0.1.0';
      }
    },

    // sub input
    subInput: {
      show: function (placeholder) {
        window.parent.postMessage({
          source: 'volo-plugin',
          kind: 'subInput',
          action: 'show',
          data: { placeholder: placeholder }
        }, '*');
      },
      hide: function () {
        window.parent.postMessage({
          source: 'volo-plugin',
          kind: 'subInput',
          action: 'hide'
        }, '*');
      },
      setValue: function (text) {
        window.parent.postMessage({
          source: 'volo-plugin',
          kind: 'subInput',
          action: 'setValue',
          data: { text: text }
        }, '*');
      },
      onChange: function (callback) {
        subInputCallback = callback;
      }
    },

    // screen capture
    screenCapture: function () { return call('screenCapture'); },
    screenCaptureArea: function () { return call('screenCaptureArea'); },

    // file system
    fs: {
      read: function (path) { return call('fs.read', { path: path }); },
      readBinary: function (path) { return call('fs.readBinary', { path: path }); },
      write: function (path, content) { return call('fs.write', { path: path, content: content }); },
      writeBinary: function (path, content) { return call('fs.writeBinary', { path: path, content: content }); },
      exists: function (path) { return call('fs.exists', { path: path }); },
      mkdir: function (path) { return call('fs.mkdir', { path: path }); },
      remove: function (path) { return call('fs.remove', { path: path }); },
      list: function (path) { return call('fs.list', { path: path }); },
      pickFile: function (options) { return call('fs.pickFile', { options: options }); },
      pickFiles: function (options) { return call('fs.pickFiles', { options: options }); },
      pickFolder: function () { return call('fs.pickFolder'); }
    }
  };
})();
`;
