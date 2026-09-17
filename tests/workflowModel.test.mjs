import assert from 'node:assert/strict';
import test from 'node:test';

import {
  DEFAULT_WORKFLOW_TEXT,
  formatWorkflowRunDuration,
  formatWorkflowValue,
  parseWorkflowDefinition,
  parseWorkflowInput,
  toWorkflowOptions,
  workflowRunErrorText,
  workflowRunFailedStep,
  workflowStepLabel,
} from '../src/workflow/model.ts';

test('default workflow parses and demonstrates a typed step binding', () => {
  const workflow = parseWorkflowDefinition(DEFAULT_WORKFLOW_TEXT);
  assert.equal(workflow.id, 'clipboard-notify');
  assert.equal(workflow.steps.length, 2);
  assert.equal(workflow.steps[1].type, 'tool');
  assert.equal(workflow.steps[1].args.body, '${steps.read}');
});

test('workflow input keeps JSON types and blank input becomes null', () => {
  assert.deepEqual(parseWorkflowInput('{"count":2,"enabled":true}'), {
    count: 2,
    enabled: true,
  });
  assert.equal(parseWorkflowInput('   '), null);
});

test('invalid workflow JSON produces a user-facing parse error', () => {
  assert.throws(() => parseWorkflowDefinition('{bad'), /Workflow JSON 解析失败/);
  assert.throws(
    () => parseWorkflowDefinition('{"id":"x","name":"X"}'),
    /steps 数组/,
  );
});

test('saved workflow options stay flat and do not expose recursive steps', () => {
  const workflow = parseWorkflowDefinition(DEFAULT_WORKFLOW_TEXT);
  assert.deepEqual(toWorkflowOptions([workflow]), [
    { id: 'clipboard-notify', name: 'Clipboard Notify' },
  ]);
});

test('formatting preserves strings and truncates large structured values', () => {
  assert.equal(formatWorkflowValue('hello'), 'hello');
  const rendered = formatWorkflowValue({ text: 'x'.repeat(100) }, 30);
  assert.equal(rendered.endsWith('…'), true);
  assert.equal(rendered.length, 31);
});

test('run duration formatting stays compact across millisecond, second and minute ranges', () => {
  assert.equal(formatWorkflowRunDuration(420), '420 ms');
  assert.equal(formatWorkflowRunDuration(1500), '1.5 s');
  assert.equal(formatWorkflowRunDuration(12_040), '12 s');
  assert.equal(formatWorkflowRunDuration(60_000), '1m');
  assert.equal(formatWorkflowRunDuration(62_000), '1m 2s');
});

test('run failure helpers prefer the failed step and do not require persisted outputs', () => {
  const run = {
    id: 'run-1',
    workflowId: 'demo',
    workflowName: 'Demo',
    startedAt: '2026-09-17T00:00:00.000Z',
    finishedAt: '2026-09-17T00:00:00.500Z',
    durationMs: 500,
    status: 'failed',
    steps: [
      { stepId: 'read', status: 'completed' },
      { stepId: 'notify', status: 'failed', error: 'permission denied' },
    ],
    error: 'workflow failed',
  };

  assert.equal(workflowRunFailedStep(run)?.stepId, 'notify');
  assert.equal(workflowRunErrorText(run), 'permission denied');
  assert.equal('output' in run, false);
});

test('step labels include tool names while AI steps remain concise', () => {
  const workflow = parseWorkflowDefinition(`{
    "id":"demo",
    "name":"Demo",
    "steps":[
      {"type":"tool","id":"read","name":"clipboard_read","args":{}},
      {"type":"ai","id":"summary","prompt":"summarize"}
    ]
  }`);
  assert.equal(workflowStepLabel(workflow, 'read'), 'read · clipboard_read');
  assert.equal(workflowStepLabel(workflow, 'summary'), 'summary · AI');
  assert.equal(workflowStepLabel(workflow, 'missing'), 'missing');
});
