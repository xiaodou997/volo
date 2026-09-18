import assert from 'node:assert/strict';
import test from 'node:test';

import {
  automationRetryLabel,
  automationTriggerLabel,
  buildDailyAutomation,
  buildIntervalAutomation,
  buildRetryPolicy,
  formatAutomationNextRun,
  formatDailyTime,
  parseDailyTime,
} from '../src/automation/model.ts';

test('interval automation creates the backend JSON shape', () => {
  assert.deepEqual(buildIntervalAutomation('job-1', 'workflow-1', 15, true), {
    id: 'job-1',
    workflowId: 'workflow-1',
    enabled: true,
    trigger: {
      type: 'interval',
      everyMinutes: 15,
    },
  });
});

test('daily automation creates the backend JSON shape', () => {
  assert.deepEqual(buildDailyAutomation('daily-1', 'workflow-1', 9, 30, true), {
    id: 'daily-1',
    workflowId: 'workflow-1',
    enabled: true,
    trigger: {
      type: 'daily',
      hour: 9,
      minute: 30,
    },
  });
});

test('automation draft validation rejects invalid definitions', () => {
  assert.throws(() => buildIntervalAutomation('', 'workflow-1', 15, true), /id 不能为空/);
  assert.throws(() => buildIntervalAutomation(' job ', 'workflow-1', 15, true), /首尾空白/);
  assert.throws(() => buildIntervalAutomation('job', '', 15, true), /请选择/);
  assert.throws(() => buildIntervalAutomation('job', 'workflow-1', 0, true), /运行间隔/);
  assert.throws(() => buildIntervalAutomation('job', 'workflow-1', 1.5, true), /运行间隔/);
  assert.throws(() => buildDailyAutomation('job', 'workflow-1', 24, 0, true), /每日运行时间/);
  assert.throws(() => buildDailyAutomation('job', 'workflow-1', 23, 60, true), /每日运行时间/);
});

test('retry policy is optional, validated, and included only when enabled', () => {
  const retryPolicy = buildRetryPolicy(2, 3);
  assert.deepEqual(retryPolicy, { maxRetries: 2, backoffMinutes: 3 });

  assert.deepEqual(
    buildIntervalAutomation('job', 'workflow-1', 15, true, retryPolicy).retryPolicy,
    retryPolicy,
  );
  assert.equal(
    Object.hasOwn(buildIntervalAutomation('job', 'workflow-1', 15, true), 'retryPolicy'),
    false,
  );

  assert.throws(() => buildRetryPolicy(0, 1), /最大重试次数/);
  assert.throws(() => buildRetryPolicy(11, 1), /最大重试次数/);
  assert.throws(() => buildRetryPolicy(2, 0), /重试间隔/);
  assert.throws(() => buildRetryPolicy(2, 1441), /重试间隔/);
});

test('retry label shows pending runtime state before static policy', () => {
  const record = {
    ...buildIntervalAutomation('job', 'workflow-1', 15, true, buildRetryPolicy(2, 3)),
    nextRunAt: '2026-09-17T12:30:00.000Z',
    retryState: {
      attempt: 1,
      retryAt: '2026-09-17T12:18:00.000Z',
    },
  };

  assert.match(automationRetryLabel(record), /第 1 次重试/);
  const withoutRuntime = { ...record, retryState: undefined };
  assert.match(automationRetryLabel(withoutRuntime), /最多重试 2 次/);
  assert.equal(
    automationRetryLabel(buildIntervalAutomation('no-retry', 'workflow-1', 15, true)),
    '不重试',
  );
});

test('daily time parser and formatter keep HH:mm stable', () => {
  assert.deepEqual(parseDailyTime('09:05'), { hour: 9, minute: 5 });
  assert.equal(formatDailyTime(9, 5), '09:05');
  assert.throws(() => parseDailyTime('9:05'), /HH:mm/);
  assert.throws(() => parseDailyTime('24:00'), /HH:mm/);
});

test('trigger labels remain concise for interval and daily schedules', () => {
  assert.equal(
    automationTriggerLabel({
      ...buildIntervalAutomation('a', 'w', 15, true),
      nextRunAt: undefined,
    }),
    '每 15 分钟',
  );
  assert.equal(
    automationTriggerLabel(buildIntervalAutomation('a', 'w', 120, true)),
    '每 2 小时',
  );
  assert.equal(
    automationTriggerLabel(buildIntervalAutomation('a', 'w', 2880, true)),
    '每 2 天',
  );
  assert.equal(
    automationTriggerLabel(buildDailyAutomation('a', 'w', 9, 30, true)),
    '每天 09:30',
  );
});

test('next-run formatting handles empty and invalid timestamps', () => {
  assert.equal(formatAutomationNextRun(), '—');
  assert.equal(formatAutomationNextRun('not-a-date'), 'not-a-date');
  assert.notEqual(formatAutomationNextRun('2026-09-17T12:00:00.000Z'), '2026-09-17T12:00:00.000Z');
});
