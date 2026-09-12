import assert from 'node:assert/strict';
import test from 'node:test';

import {
  reduceTimelineEvent,
  replayEventToTimelineItem,
  replayEventsToTimeline,
} from '../src/agent/timeline.ts';

test('merges streaming message deltas without mutating the previous timeline', () => {
  const initial = [{ kind: 'user', text: 'hello' }];

  const first = reduceTimelineEvent(initial, {
    kind: 'message',
    delta: true,
    content: 'Hello',
  });
  const second = reduceTimelineEvent(first.timeline, {
    kind: 'message',
    delta: true,
    content: ' world',
  });

  assert.deepEqual(initial, [{ kind: 'user', text: 'hello' }]);
  assert.deepEqual(second.timeline, [
    { kind: 'user', text: 'hello' },
    { kind: 'message', text: 'Hello world', streaming: true },
  ]);
  assert.equal(second.finished, undefined);
});

test('a non-delta event closes the current streaming answer before adding a tool call', () => {
  const previous = [{ kind: 'message', text: 'Checking', streaming: true }];
  const transition = reduceTimelineEvent(previous, {
    kind: 'tool_call',
    name: 'filesystem.search',
    args: { query: 'volo' },
  });

  assert.equal(previous[0].streaming, true);
  assert.deepEqual(transition.timeline, [
    { kind: 'message', text: 'Checking', streaming: false },
    {
      kind: 'tool_call',
      text: 'filesystem.search',
      detail: '{"query":"volo"}',
    },
  ]);
});

test('tool results keep the full payload while truncating display text', () => {
  const payload = 'x'.repeat(220);
  const transition = reduceTimelineEvent([], {
    kind: 'tool_result',
    result: payload,
  });

  assert.equal(transition.timeline.length, 1);
  assert.equal(transition.timeline[0].kind, 'tool_result');
  assert.equal(transition.timeline[0].text, `${'x'.repeat(200)}…`);
  assert.equal(transition.timeline[0].fullText, payload);
});

test('error finalizes a streaming answer and finishes the session', () => {
  const transition = reduceTimelineEvent(
    [{ kind: 'message', text: 'partial', streaming: true }],
    { kind: 'error', content: 'network failed' },
  );

  assert.deepEqual(transition.timeline, [
    { kind: 'message', text: 'partial', streaming: false },
    { kind: 'error', text: 'network failed' },
  ]);
  assert.equal(transition.finished, true);
  assert.equal(transition.stopping, false);
});

test('done closes streaming state without creating a visible timeline item', () => {
  const transition = reduceTimelineEvent(
    [{ kind: 'message', text: 'complete', streaming: true }],
    { kind: 'done' },
  );

  assert.deepEqual(transition.timeline, [
    { kind: 'message', text: 'complete', streaming: false },
  ]);
  assert.equal(transition.finished, true);
  assert.equal(transition.stopping, false);
});

test('replay conversion preserves user image count and full tool result', () => {
  const items = replayEventsToTimeline([
    { kind: 'user', content: 'look', imageCount: 2 },
    { kind: 'message', content: 'answer' },
    { kind: 'tool_call', name: 'demo', args: { a: 1 } },
    { kind: 'tool_result', result: 'result' },
    { kind: 'error', content: 'failed' },
  ]);

  assert.deepEqual(items, [
    { kind: 'user', text: 'look', imageCount: 2 },
    { kind: 'message', text: 'answer' },
    { kind: 'tool_call', text: 'demo', detail: '{"a":1}' },
    { kind: 'tool_result', text: 'result', fullText: 'result' },
    { kind: 'error', text: 'failed' },
  ]);
});

test('empty replay message is ignored', () => {
  assert.equal(replayEventToTimelineItem({ kind: 'message', content: '' }), null);
});
