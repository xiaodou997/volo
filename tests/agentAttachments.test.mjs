import assert from 'node:assert/strict';
import test from 'node:test';

import {
  MAX_IMAGE_SIZE,
  MAX_TEXT_SIZE,
  buildQueryWithTextAttachments,
  classifyAttachment,
} from '../src/agent/attachments.ts';

test('accepts images up to the configured size limit', () => {
  assert.deepEqual(
    classifyAttachment({ name: 'shot.png', type: 'image/png', size: MAX_IMAGE_SIZE }),
    { kind: 'image' },
  );
});

test('rejects oversized images with the existing user-facing message', () => {
  assert.deepEqual(
    classifyAttachment({ name: 'large.png', type: 'image/png', size: MAX_IMAGE_SIZE + 1 }),
    { kind: 'reject', message: '图片超过 10MB，未添加' },
  );
});

test('recognizes text attachments by MIME type or known extension', () => {
  assert.deepEqual(
    classifyAttachment({ name: 'notes', type: 'text/plain', size: 10 }),
    { kind: 'text' },
  );
  assert.deepEqual(
    classifyAttachment({ name: 'config.json', type: 'application/json', size: 10 }),
    { kind: 'text' },
  );
});

test('rejects oversized text attachments', () => {
  assert.deepEqual(
    classifyAttachment({ name: 'huge.md', type: 'text/markdown', size: MAX_TEXT_SIZE + 1 }),
    { kind: 'reject', message: '文本文件超过 256KB，未添加' },
  );
});

test('rejects unsupported binary attachments without changing the error wording', () => {
  assert.deepEqual(
    classifyAttachment({ name: 'archive.zip', type: 'application/zip', size: 100 }),
    {
      kind: 'reject',
      message: '不支持的文件类型：archive.zip（支持图片和文本类文件）',
    },
  );
});

test('builds the follow-up query with text attachments using the current prompt format', () => {
  assert.equal(
    buildQueryWithTextAttachments('请总结', [
      { name: 'a.txt', text: 'alpha' },
      { name: 'b.md', text: '# beta' },
    ]),
    '请总结\n\n[附件 a.txt]\n```\nalpha\n```\n\n[附件 b.md]\n```\n# beta\n```',
  );
});
