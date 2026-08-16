// 生成指定数量的 UUID v4，返回字符串数组。
// Tool 工具：无界面，入口通过 rubick.tool.onInvoke 注册，
// 由宿主在隐藏 iframe 中调用；返回值（或 Promise resolve 值）回传给 Agent。
// crypto.randomUUID 在 opaque origin iframe 中可用。

rubick.tool.onInvoke(function (input) {
  var count = input && typeof input.count === 'number' ? Math.floor(input.count) : 1;
  if (count < 1) count = 1;
  if (count > 20) count = 20;

  var uuids = [];
  for (var i = 0; i < count; i++) {
    uuids.push(crypto.randomUUID());
  }
  return { uuids: uuids };
});
