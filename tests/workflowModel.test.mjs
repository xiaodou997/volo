import assert from 'node:assert/strict';
import test from 'node:test';

import {
  DEFAULT_WORKFLOW_TEXT,
  formatWorkflowValue,
  parseWorkflowDefinition,
  parseWorkflowInput,
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

test('formatting preserves strings and truncates large structured values', () => {
  assert.equal(formatWorkflowValue('hello'), 'hello');
  const rendered = formatWorkflowValue({ text: 'x'.repeat(100) }, 30);
  assert.equal(rendered.endsWith('…'), true);
  assert.equal(rendered.length, 31);
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
