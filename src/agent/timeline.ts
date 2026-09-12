import type { AgentEvent, ReplayEvent } from '../api/rubick';

export interface TimelineItem {
  kind: 'user' | 'message' | 'tool_call' | 'tool_result' | 'error';
  text: string;
  detail?: string;
  /** 工具结果的完整内容；text 只用于截断显示。 */
  fullText?: string;
  /** 流式回答尚未结束。 */
  streaming?: boolean;
  /** 实时用户消息中的图片 data URL。 */
  images?: string[];
  /** 实时用户消息中的文本附件文件名。 */
  files?: string[];
  /** 回放日志只保存图片数量时用于徽标展示。 */
  imageCount?: number;
}

export interface TimelineTransition {
  timeline: TimelineItem[];
  /** 仅当本事件改变会话完成状态时返回。 */
  finished?: boolean;
  /** 仅当本事件应改变停止按钮状态时返回。 */
  stopping?: boolean;
}

export function truncateTimelineText(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max)}…` : text;
}

export function replayEventToTimelineItem(event: ReplayEvent): TimelineItem | null {
  switch (event.kind) {
    case 'user':
      return {
        kind: 'user',
        text: event.content ?? '',
        imageCount: event.imageCount,
      };
    case 'message':
      return event.content ? { kind: 'message', text: event.content } : null;
    case 'tool_call':
      return {
        kind: 'tool_call',
        text: event.name ?? 'unknown',
        detail: truncateTimelineText(JSON.stringify(event.args ?? {}), 100),
      };
    case 'tool_result': {
      const result = event.result ?? '';
      return {
        kind: 'tool_result',
        text: truncateTimelineText(result, 200),
        fullText: result,
      };
    }
    case 'error':
      return { kind: 'error', text: event.content ?? '未知错误' };
  }
}

export function replayEventsToTimeline(events: ReplayEvent[]): TimelineItem[] {
  return events
    .map(replayEventToTimelineItem)
    .filter((item): item is TimelineItem => item !== null);
}

/**
 * 把一个 AgentEvent 归并到时间线。
 *
 * 该函数不依赖 Vue / DOM / Tauri，并且不会修改传入的 timeline，便于对流式、
 * Tool、Error、Done 等状态转换做稳定单元测试。
 */
export function reduceTimelineEvent(
  timeline: TimelineItem[],
  event: AgentEvent,
): TimelineTransition {
  const next = timeline.slice();
  const lastIndex = next.length - 1;
  const last = lastIndex >= 0 ? next[lastIndex] : undefined;

  if (event.kind === 'message' && event.delta) {
    const delta = event.content ?? '';
    if (last?.kind === 'message' && last.streaming) {
      next[lastIndex] = { ...last, text: last.text + delta };
    } else {
      next.push({ kind: 'message', text: delta, streaming: true });
    }
    return { timeline: next };
  }

  // 任意非 delta 事件都意味着上一段流式回答结束。
  if (last?.streaming) {
    next[lastIndex] = { ...last, streaming: false };
  }

  switch (event.kind) {
    case 'message':
      if (event.content) {
        next.push({ kind: 'message', text: event.content });
      }
      return { timeline: next };

    case 'tool_call':
      next.push({
        kind: 'tool_call',
        text: event.name ?? 'unknown',
        detail: truncateTimelineText(JSON.stringify(event.args ?? {}), 100),
      });
      return { timeline: next };

    case 'tool_result': {
      const result = event.result ?? '';
      next.push({
        kind: 'tool_result',
        text: truncateTimelineText(result, 200),
        fullText: result,
      });
      return { timeline: next };
    }

    case 'error':
      next.push({ kind: 'error', text: event.content ?? '未知错误' });
      return { timeline: next, finished: true, stopping: false };

    case 'done':
      return { timeline: next, finished: true, stopping: false };
  }
}
