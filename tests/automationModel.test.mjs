import assert from 'node:assert/strict';
import test from 'node:test';

import {
  automationIntervalLabel,
  buildIntervalAutomation,
  formatAutomationNextRun,
} from '../src/automation/model.ts';

test('buildIntervalAutomation creates the backend JSON shape', () => {
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

test('automation draft validation rejects missing ids/workflows and invalid intervals', () => {
  assert.throws(() => buildIntervalAutomation('', 'workflow-1', 15, true), /id 不能为空/);
  assert.throws(() => buildIntervalAutomation(' job ', 'workflow-1', 15, true), /首尾空白/);
  assert.throws(() => buildIntervalAutomation('job', '', 15, true), /请选择/);
  assert.throws(() => buildIntervalAutomation('job', 'workflow-1', 0, true), /运行间隔/);
  assert.throws(() => buildIntervalAutomation('job', 'workflow-1', 1.5, true), /运行间隔/);
});

test('interval labels remain concise', () => {
  assert.equal(
    automationIntervalLabel({
      ...buildIntervalAutomation('a', 'w', 15, true),
      nextRunAt: undefined,
    }),
    '每 15 分钟',
  );
  assert.equal(
    automationIntervalLabel(buildIntervalAutomation('a', 'w', 120, true)),
    '每 2 小时',
  );
  assert.equal(
    automationIntervalLabel(buildIntervalAutomation('a', 'w', 2880, true)),
    '每 2 天',
  );
});

test('next-run formatting handles empty and invalid timestamps', () => {
  assert.equal(formatAutomationNextRun(), '—');
  assert.equal(formatAutomationNextRun('not-a-date'), 'not-a-date');
  assert.notEqual(formatAutomationNextRun('2026-09-17T12:00:00.000Z'), '2026-09-17T12:00:00.000Z');
});
