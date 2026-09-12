export const MAX_IMAGE_SIZE = 10 * 1024 * 1024;
export const MAX_TEXT_SIZE = 256 * 1024;

const TEXT_FILE_EXT =
  /\.(txt|md|markdown|log|json|csv|yaml|yml|xml|html?|js|ts|jsx|tsx|py|rs|sh|c|h|cpp|java|go|css|sql|toml|ini)$/i;

export interface AttachmentMeta {
  name: string;
  type: string;
  size: number;
}

export interface TextAttachment {
  name: string;
  text: string;
}

export type AttachmentDecision =
  | { kind: 'image' }
  | { kind: 'text' }
  | { kind: 'reject'; message: string };

/**
 * 根据浏览器 File 的稳定元数据决定附件处理方式。
 * 不读取文件内容，也不依赖 DOM/FileReader，因此可以独立回归测试。
 */
export function classifyAttachment(file: AttachmentMeta): AttachmentDecision {
  if (file.type.startsWith('image/')) {
    if (file.size > MAX_IMAGE_SIZE) {
      return {
        kind: 'reject',
        message: `图片超过 ${Math.round(MAX_IMAGE_SIZE / 1024 / 1024)}MB，未添加`,
      };
    }
    return { kind: 'image' };
  }

  if (file.type.startsWith('text/') || TEXT_FILE_EXT.test(file.name)) {
    if (file.size > MAX_TEXT_SIZE) {
      return {
        kind: 'reject',
        message: `文本文件超过 ${Math.round(MAX_TEXT_SIZE / 1024)}KB，未添加`,
      };
    }
    return { kind: 'text' };
  }

  return {
    kind: 'reject',
    message: `不支持的文件类型：${file.name || file.type || '未知'}（支持图片和文本类文件）`,
  };
}

/** 把文本附件按现有 Agent 协议拼进用户正文。 */
export function buildQueryWithTextAttachments(
  query: string,
  files: readonly TextAttachment[],
): string {
  let fullQuery = query;
  for (const file of files) {
    fullQuery += `\n\n[附件 ${file.name}]\n\`\`\`\n${file.text}\n\`\`\``;
  }
  return fullQuery;
}
