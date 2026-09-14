export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export interface WorkflowToolStep {
  type: 'tool';
  id: string;
  name: string;
  args?: JsonValue;
}

export interface WorkflowAiStep {
  type: 'ai';
  id: string;
  prompt: string;
}

export type WorkflowStep = WorkflowToolStep | WorkflowAiStep;

export interface WorkflowDefinition {
  id: string;
  name: string;
  steps: WorkflowStep[];
}

export type WorkflowExecutionStatus = 'completed' | 'failed';
export type WorkflowStepStatus = 'completed' | 'failed';

export interface WorkflowStepExecution {
  stepId: string;
  status: WorkflowStepStatus;
  output?: JsonValue;
  error?: string;
}

export interface WorkflowExecution {
  workflowId: string;
  status: WorkflowExecutionStatus;
  steps: WorkflowStepExecution[];
  output?: JsonValue;
  error?: string;
}

export const DEFAULT_WORKFLOW: WorkflowDefinition = {
  id: 'clipboard-notify',
  name: 'Clipboard Notify',
  steps: [
    {
      type: 'tool',
      id: 'read',
      name: 'clipboard_read',
      args: {},
    },
    {
      type: 'tool',
      id: 'notify',
      name: 'notification_show',
      args: {
        title: 'Volo Workflow',
        body: '${steps.read}',
      },
    },
  ],
};

export const DEFAULT_WORKFLOW_TEXT = JSON.stringify(DEFAULT_WORKFLOW, null, 2);
export const DEFAULT_WORKFLOW_INPUT = '{}';

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function parseWorkflowDefinition(text: string): WorkflowDefinition {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (error) {
    throw new Error(`Workflow JSON 解析失败: ${error instanceof Error ? error.message : String(error)}`);
  }

  if (!isRecord(value) || typeof value.id !== 'string' || typeof value.name !== 'string' || !Array.isArray(value.steps)) {
    throw new Error('Workflow 必须包含字符串 id、name 和 steps 数组');
  }

  return value as unknown as WorkflowDefinition;
}

export function parseWorkflowInput(text: string): JsonValue {
  if (!text.trim()) return null;
  try {
    return JSON.parse(text) as JsonValue;
  } catch (error) {
    throw new Error(`Input JSON 解析失败: ${error instanceof Error ? error.message : String(error)}`);
  }
}

export function formatWorkflowValue(value: JsonValue | undefined, maxChars = 1200): string {
  if (value === undefined) return '';
  const rendered = typeof value === 'string' ? value : JSON.stringify(value, null, 2);
  if (rendered.length <= maxChars) return rendered;
  return `${rendered.slice(0, maxChars)}…`;
}

export function workflowStepLabel(workflow: WorkflowDefinition | null, stepId: string): string {
  const step = workflow?.steps.find((item) => item.id === stepId);
  if (!step) return stepId;
  return step.type === 'tool' ? `${stepId} · ${step.name}` : `${stepId} · AI`;
}
