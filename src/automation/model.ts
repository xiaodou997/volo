export interface IntervalAutomationTrigger {
  type: 'interval';
  everyMinutes: number;
}

export interface DailyAutomationTrigger {
  type: 'daily';
  hour: number;
  minute: number;
}

export type AutomationTrigger = IntervalAutomationTrigger | DailyAutomationTrigger;

export interface WorkflowAutomation {
  id: string;
  workflowId: string;
  enabled: boolean;
  trigger: AutomationTrigger;
}

export interface AutomationRecord extends WorkflowAutomation {
  nextRunAt?: string;
}

function validateAutomationIdentity(id: string, workflowId: string) {
  if (!id.trim()) {
    throw new Error('Automation id 不能为空');
  }
  if (id.trim() !== id) {
    throw new Error('Automation id 不能包含首尾空白');
  }
  if (!workflowId.trim()) {
    throw new Error('请选择一个已保存的 Workflow');
  }
}

export function buildIntervalAutomation(
  id: string,
  workflowId: string,
  everyMinutes: number,
  enabled: boolean,
): WorkflowAutomation {
  validateAutomationIdentity(id, workflowId);
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

export function buildDailyAutomation(
  id: string,
  workflowId: string,
  hour: number,
  minute: number,
  enabled: boolean,
): WorkflowAutomation {
  validateAutomationIdentity(id, workflowId);
  if (
    !Number.isInteger(hour) ||
    !Number.isInteger(minute) ||
    hour < 0 ||
    hour > 23 ||
    minute < 0 ||
    minute > 59
  ) {
    throw new Error('每日运行时间必须是有效的 HH:mm');
  }

  return {
    id,
    workflowId,
    enabled,
    trigger: {
      type: 'daily',
      hour,
      minute,
    },
  };
}

export function parseDailyTime(value: string): { hour: number; minute: number } {
  const match = /^(\d{2}):(\d{2})$/.exec(value);
  if (!match) {
    throw new Error('每日运行时间必须是有效的 HH:mm');
  }
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (hour > 23 || minute > 59) {
    throw new Error('每日运行时间必须是有效的 HH:mm');
  }
  return { hour, minute };
}

export function formatDailyTime(hour: number, minute: number): string {
  return `${String(hour).padStart(2, '0')}:${String(minute).padStart(2, '0')}`;
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

export function automationTriggerLabel(record: WorkflowAutomation): string {
  if (record.trigger.type === 'daily') {
    return `每天 ${formatDailyTime(record.trigger.hour, record.trigger.minute)}`;
  }

  const minutes = record.trigger.everyMinutes;
  if (minutes < 60) return `每 ${minutes} 分钟`;
  if (minutes % 1440 === 0) return `每 ${minutes / 1440} 天`;
  if (minutes % 60 === 0) return `每 ${minutes / 60} 小时`;
  return `每 ${minutes} 分钟`;
}
