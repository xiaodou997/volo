// 生成 UUID v4，复制到剪贴板并弹出通知。
// Command（no-view）命令：无界面，入口通过 rubick.command.onRun 注册，
// 由宿主在隐藏 iframe 中触发；完成后调用 rubick.command.done() 结束运行单元
// （可选，宿主有超时兜底）。crypto.randomUUID 在 opaque origin iframe 中可用。

rubick.command.onRun(async () => {
  const id = crypto.randomUUID();

  await rubick.clipboard.writeText(id);

  await rubick.notification.show({
    title: 'UUID 已生成',
    body: id,
  });

  rubick.command.done();
});
