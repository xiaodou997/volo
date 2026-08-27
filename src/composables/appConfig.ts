/**
 * 应用级配置（跨视图共享的少量开关）
 *
 * App.vue 启动时加载一次；SettingsView 保存设置后同步对应 ref。
 * 当前只有 hideOnBlur（失焦隐藏），有新增共享开关再加字段。
 */

import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';

/** 失焦时是否隐藏主窗口（对应 config.hideOnBlur，默认 true） */
export const hideOnBlur = ref(true);

export async function loadAppConfig(): Promise<void> {
  try {
    const config = await invoke<{ hideOnBlur?: boolean }>('get_config');
    if (typeof config?.hideOnBlur === 'boolean') {
      hideOnBlur.value = config.hideOnBlur;
    }
  } catch (e) {
    console.error('Failed to load app config:', e);
  }
}
