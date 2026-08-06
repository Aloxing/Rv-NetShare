<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { FolderDown } from '@lucide/vue';
import AppNavBar from './components/AppNavBar.vue';
import AppTitleBar from './components/AppTitleBar.vue';
import AppStatusBar from './components/AppStatusBar.vue';
import SharesPanel from './components/SharesPanel.vue';
import SitesPanel from './components/SitesPanel.vue';
import HistoryPanel from './components/HistoryPanel.vue';
import SettingsPanel from './components/SettingsPanel.vue';
import ToastStack from './components/ToastStack.vue';
import { disposeAppState, initAppState, useAppState } from './composables/useAppState';
import type { Tab } from './types';

const activeTab = ref<Tab>('shares');
const isDragging = ref(false);
const state = useAppState();

let dragVisualUnlisten: (() => void) | null = null;

onMounted(async () => {
  await initAppState();
  try {
    const { getCurrentWebview } = await import('@tauri-apps/api/webview');
    dragVisualUnlisten = await getCurrentWebview().onDragDropEvent((event) => {
      const payload = (event as { payload: { type: string } }).payload;
      isDragging.value = payload.type === 'enter' || payload.type === 'over';
    });
  } catch (e) { console.warn('visual drag listener failed', e); }
});

onBeforeUnmount(() => {
  disposeAppState();
  if (dragVisualUnlisten) dragVisualUnlisten();
});

const shareCount = computed(() => state.shares.length);
const siteCount = computed(() => state.sites.length);
const historyCount = computed(() => state.history.length);
</script>

<template>
  <div class="flex h-screen w-screen flex-col overflow-hidden rounded-2xl border border-[var(--color-border)] bg-[var(--color-bg)] text-[var(--color-text)]">
    <AppTitleBar />
    <AppNavBar :active="activeTab" @change="(t) => activeTab = t" />

    <main class="flex flex-1 flex-col overflow-hidden">
      <div class="flex-1 overflow-hidden">
        <SharesPanel v-if="activeTab === 'shares'" />
        <SitesPanel v-else-if="activeTab === 'sites'" />
        <HistoryPanel v-else-if="activeTab === 'history'" />
        <SettingsPanel v-else-if="activeTab === 'settings'" />
      </div>
    </main>

    <div
      v-show="isDragging"
      class="pointer-events-none fixed inset-3 z-50 flex items-center justify-center rounded-2xl border-2 border-dashed border-[var(--color-accent)] bg-[var(--color-bg)]/85 backdrop-blur-md"
    >
      <div class="flex flex-col items-center gap-3 text-center">
        <div class="flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--color-accent-soft)] text-[var(--color-accent)]">
          <FolderDown :size="26" />
        </div>
        <div class="text-[15px] font-semibold">松开以添加分享</div>
      </div>
    </div>

    <AppStatusBar :shares="shareCount" :sites="siteCount" :history="historyCount" />
    <ToastStack />
  </div>
</template>
