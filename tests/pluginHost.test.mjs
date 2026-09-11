import assert from 'node:assert/strict';
import test from 'node:test';

import { createPluginHost } from '../src/bridge/pluginHost.ts';

const flushAsync = () => new Promise((resolve) => setImmediate(resolve));

function createFixture() {
  const posted = [];
  const calls = [];
  const events = [];

  const contentWindow = {
    postMessage(message, targetOrigin) {
      posted.push({ message, targetOrigin });
    },
  };

  const iframe = { contentWindow };
  const invoke = async (command, args) => {
    calls.push({ command, args });
    return { command, ok: true };
  };

  const handlers = {
    onExit: () => events.push(['exit']),
    onResize: (height) => events.push(['resize', height]),
    onSubInputShow: (data) => events.push(['subInputShow', data]),
    onSubInputHide: () => events.push(['subInputHide']),
    onSubInputSetValue: (text) => events.push(['subInputSetValue', text]),
  };

  const host = createPluginHost(iframe, 'trusted-plugin', handlers, invoke);
  return { host, contentWindow, posted, calls, events };
}

function message(source, data) {
  return { source, data };
}

test('rejects messages that do not originate from the plugin iframe', async () => {
  const { host, calls, posted } = createFixture();

  const handled = host.handleMessage(
    message({}, {
      source: 'volo-plugin',
      kind: 'api',
      reqId: 'req-1',
      method: 'fs.read',
      args: { path: '/tmp/demo.txt' },
    }),
  );

  await flushAsync();
  assert.equal(handled, false);
  assert.deepEqual(calls, []);
  assert.deepEqual(posted, []);
});

test('overwrites a spoofed pluginId with the host-owned identity', async () => {
  const { host, contentWindow, calls, posted } = createFixture();

  assert.equal(
    host.handleMessage(
      message(contentWindow, {
        source: 'volo-plugin',
        kind: 'api',
        reqId: 'req-2',
        method: 'fs.read',
        args: {
          path: '/tmp/demo.txt',
          pluginId: 'spoofed-plugin',
        },
      }),
    ),
    true,
  );

  await flushAsync();
  assert.deepEqual(calls, [
    {
      command: 'fs_read',
      args: {
        path: '/tmp/demo.txt',
        pluginId: 'trusted-plugin',
      },
    },
  ]);
  assert.equal(posted.length, 1);
  assert.equal(posted[0].targetOrigin, '*');
  assert.equal(posted[0].message.source, 'volo-host');
  assert.equal(posted[0].message.kind, 'api-result');
  assert.equal(posted[0].message.reqId, 'req-2');
  assert.equal(posted[0].message.ok, true);
});

test('rejects unknown API methods without invoking Tauri', async () => {
  const { host, contentWindow, calls, posted } = createFixture();

  host.handleMessage(
    message(contentWindow, {
      source: 'volo-plugin',
      kind: 'api',
      reqId: 'req-3',
      method: 'shell.exec-arbitrary',
      args: { command: 'rm -rf /' },
    }),
  );

  await flushAsync();
  assert.deepEqual(calls, []);
  assert.equal(posted.length, 1);
  assert.equal(posted[0].message.ok, false);
  assert.match(posted[0].message.error, /Unknown method/);
});

test('host-only window commands do not receive plugin identity arguments', async () => {
  const { host, contentWindow, calls } = createFixture();

  host.handleMessage(
    message(contentWindow, {
      source: 'volo-plugin',
      kind: 'api',
      reqId: 'req-4',
      method: 'window.hide',
      args: {},
    }),
  );

  await flushAsync();
  assert.deepEqual(calls, [{ command: 'hide_main_window', args: {} }]);
});

test('disposed hosts stop accepting messages and emitting events', async () => {
  const { host, contentWindow, calls, posted } = createFixture();
  host.dispose();

  assert.equal(
    host.handleMessage(
      message(contentWindow, {
        source: 'volo-plugin',
        kind: 'api',
        reqId: 'req-5',
        method: 'clipboard.readText',
        args: {},
      }),
    ),
    false,
  );
  host.sendEvent('onPluginEnter', { query: 'ignored' });

  await flushAsync();
  assert.deepEqual(calls, []);
  assert.deepEqual(posted, []);
});
