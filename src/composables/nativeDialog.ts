/**
 * 原生对话框（文件/目录选择）状态
 *
 * 原生面板弹出会抢走主窗口焦点；这期间启动器的失焦隐藏不应生效，
 * 否则用户会看到"点按钮后窗口消失"。调用原生对话框的代码用
 * withNativeDialog 包一层即可。
 */

import { ref } from 'vue';

export const nativeDialogOpen = ref(false);

export async function withNativeDialog<T>(fn: () => Promise<T>): Promise<T> {
  nativeDialogOpen.value = true;
  try {
    return await fn();
  } finally {
    nativeDialogOpen.value = false;
  }
}
