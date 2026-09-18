export interface IntervalAutomationTrigger {
  type: 'interval';
  everyMinutes: number;
}

export interface WorkflowAutomation {
  id: string;
  workflowId: string;
  enabled: boolean;
  trigger: IntervalAutomationTrigger;
}

export interface AutomationRecord extends WorkflowAutomation {
  nextRunAt?: string;
}

export function buildIntervalAutomation(
  id: string,
  workflowId: string,
  everyMinutes: number,
  enabled: boolean,
): WorkflowAutomation {
  if (!id.trim()) {
    throw new Error('Automation id 不能为空');
  }
  if (id.trim() !== id) {
    throw new Error('Automation id 不能包含首尾空白');
  }
  if (!workflowId.trim()) {
    throw new Error('请选择一个已保存的 Workflow');
  }
  if (!Number.isInteger(everyMinutes) || everyMinutes < 1 || everyMinutes > 525_600) {
    throw new Error('运行间隔必须是 1 到 525600 分钟之间的整数');
  }

  return {
    id,
    workflowId,
    enabled,
    trigger: {
      type: 'interval',
      everyMinutes,
    },
  };
}

export function formatAutomationNextRun(value?: string): string {
  if (!value) return '—';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  });
}

export function automationIntervalLabel(record: AutomationRecord): string {
  const minutes = record.trigger.everyMinutes;
  if (minutes < 60) return `每 ${minutes} 分钟`;
  if (minutes % 1440 === 0) return `每 ${minutes / 1440} 天`;
  if (minutes % 60 === 0) return `每 ${minutes / 60} 小时`;
  return `每 ${minutes} 分钟`;
}
