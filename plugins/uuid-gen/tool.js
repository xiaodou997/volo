// 生成指定数量的 UUID v4，返回字符串数组。
// Tool 工具：无界面，入口通过 rubick.tool.onInvoke 注册，
// 前台仍可由 renderer iframe 调用；后台 Automation 会在受限 QuickJS runtime 中调用。
// 两种 runtime 都提供 crypto.randomUUID。

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
