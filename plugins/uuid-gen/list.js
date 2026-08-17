// List 模式命令示范：选择 UUID 格式后生成并复制到剪贴板。
// onRun(query) 按过滤词筛选固定选项后 setList 推送到启动器结果列表；
// 宿主重触发 onRun（过滤词变化）与触发 onSelect（回车选中）都走 postMessage 事件。
// 格式项带二级动作面板（Tab/→ 展开）：onAction(itemId, actionId) 处理面板动作。
// 选中动作完成后必须调 rubick.command.done() 结束运行单元。

const FORMAT_ACTIONS = [
  { id: 'copy', title: '复制到剪贴板', description: '生成 1 个该格式 UUID 并复制' },
  { id: 'copy5', title: '复制 5 个', description: '生成 5 个该格式 UUID，每行一个' },
];

const OPTIONS = [
  {
    id: 'lower',
    title: '小写 UUID',
    description: 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx',
    actions: FORMAT_ACTIONS,
  },
  {
    id: 'upper',
    title: '大写 UUID',
    description: 'XXXXXXXX-XXXX-4XXX-YXXX-XXXXXXXXXXXX',
    actions: FORMAT_ACTIONS,
  },
  {
    id: 'no-dashes',
    title: '无横线 UUID',
    description: 'xxxxxxxxxxxx4xxxyxxxxxxxxxxxxxxx',
    actions: FORMAT_ACTIONS,
  },
  {
    id: 'batch5',
    title: '批量 5 个',
    description: '生成 5 个小写 UUID，每行一个',
  },
];

function generateOne(id) {
  const uuid = crypto.randomUUID();
  if (id === 'upper') return uuid.toUpperCase();
  if (id === 'no-dashes') return uuid.replace(/-/g, '');
  return uuid;
}

function generate(id) {
  if (id === 'batch5') {
    return Array.from({ length: 5 }, () => crypto.randomUUID()).join('\n');
  }
  return generateOne(id);
}

async function copyAndNotify(title, text) {
  await rubick.clipboard.writeText(text);
  await rubick.notification.show({
    title: `已复制：${title}`,
    body: text,
  });
  rubick.command.done();
}

rubick.command.onRun((query) => {
  const q = (query || '').trim().toLowerCase();
  const items = q
    ? OPTIONS.filter(
        (o) =>
          o.title.toLowerCase().includes(q) ||
          o.id.toLowerCase().includes(q) ||
          o.description.toLowerCase().includes(q),
      )
    : OPTIONS;
  rubick.command.setList(items);
});

rubick.command.onSelect(async (id) => {
  const option = OPTIONS.find((o) => o.id === id);
  if (!option) {
    rubick.command.done(new Error(`未知选项：${id}`));
    return;
  }
  await copyAndNotify(option.title, generate(id));
});

// 二级动作面板：copy = 单个，copy5 = 同格式 5 个
rubick.command.onAction(async (id, actionId) => {
  const option = OPTIONS.find((o) => o.id === id);
  if (!option) {
    rubick.command.done(new Error(`未知选项：${id}`));
    return;
  }
  if (actionId === 'copy') {
    await copyAndNotify(option.title, generateOne(id));
    return;
  }
  if (actionId === 'copy5') {
    const text = Array.from({ length: 5 }, () => generateOne(id)).join('\n');
    await copyAndNotify(`${option.title} × 5`, text);
    return;
  }
  rubick.command.done(new Error(`未知动作：${actionId}`));
});
