<script setup lang="ts">
import { onMounted, shallowRef } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import AutomationPanel from './AutomationPanel.vue';
import WorkflowPermissionPanel from './WorkflowPermissionPanel.vue';
import { toWorkflowOptions, type WorkflowDefinition, type WorkflowOption } from '../workflow/model';

defineEmits<{ back: [] }>();

const workflows = shallowRef<WorkflowOption[]>([]);

onMounted(() => {
  void invoke<WorkflowDefinition[]>('workflow_list')
    .then((items) => {
      workflows.value = toWorkflowOptions(items);
    })
    .catch((error) => {
      console.warn('读取 Workflow 列表失败', error);
    });
});
</script>

<template>
  <div class="automation-view">
    <header class="automation-header">
      <button class="back-btn" title="返回" @click="$emit('back')">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M19 12H5M12 19l-7-7 7-7" />
        </svg>
      </button>
      <div class="header-copy">
        <h2>Automation</h2>
        <span>Interval / Daily · Background · Skip missed runs</span>
      </div>
    </header>

    <WorkflowPermissionPanel :workflows="workflows" />
    <AutomationPanel :workflows="workflows" />
  </div>
</template>

<style scoped>
.automation-view {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  color: var(--text-primary);
  background: var(--bg-primary);
}

.automation-header {
  height: 58px;
  flex: 0 0 58px;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 16px;
  border-bottom: 1px solid var(--border-color);
}

.back-btn {
  width: 34px;
  height: 34px;
  display: grid;
  place-items: center;
  border: 0;
  border-radius: 8px;
  color: var(--text-secondary);
  background: transparent;
  cursor: pointer;
}

.back-btn:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.header-copy {
  min-width: 0;
  flex: 1;
  display: flex;
  align-items: baseline;
  gap: 10px;
}

.header-copy h2 {
  font-size: 17px;
  font-weight: 650;
}

.header-copy span {
  font-size: 12px;
  color: var(--text-tertiary);
}
</style>
