<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { Copy, Minus, Square, X } from '@lucide/vue';
import { getCurrentWindow } from '@tauri-apps/api/window';

const appWindow = getCurrentWindow();
const isMaximized = ref(false);
let resizeUnlisten: (() => void) | null = null;

async function refreshMaximized() {
  try {
    isMaximized.value = await appWindow.isMaximized();
  } catch (e) {
    console.warn('refresh maximized failed', e);
  }
}

onMounted(async () => {
  await refreshMaximized();
  try {
    resizeUnlisten = await appWindow.onResized(() => { void refreshMaximized(); });
  } catch (e) {
    console.warn('resize listener failed', e);
  }
});

onBeforeUnmount(() => {
  if (resizeUnlisten) resizeUnlisten();
});

function minimize() { void appWindow.minimize(); }
function toggleMaximize() { void appWindow.toggleMaximize(); }
function closeWindow() { void appWindow.close(); }
</script>

<template>
  <header
    class="relative flex h-14 shrink-0 select-none items-center justify-between border-b border-[var(--color-border-soft)] bg-[var(--color-bg-elevated)] pl-5 pr-0"
    @dblclick="toggleMaximize"
  >
    <div data-tauri-drag-region class="absolute inset-0 z-0"></div>

    <div class="relative z-10 flex items-center gap-4">
      <div class="flex items-center gap-3">
        <div class="flex h-9 w-9 items-center justify-center overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-bg)] shadow-[var(--shadow-card)]">
          <img src="/app-icon.png" alt="RV NetShare" class="h-full w-full object-cover" />
        </div>
        <div class="leading-tight">
          <span class="text-[14px] font-semibold tracking-tight">RV NetShare</span>
        </div>
      </div>
    </div>

    <div class="relative z-10 flex items-center">
      <div class="flex h-14 items-stretch">
        <button
          class="flex w-11 items-center justify-center text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
          title="最小化"
          @click.stop="minimize"
        >
          <Minus :size="15" />
        </button>
        <button
          class="flex w-11 items-center justify-center text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
          title="最大化 / 还原"
          @click.stop="toggleMaximize"
        >
          <component :is="isMaximized ? Copy : Square" :size="13" />
        </button>
        <button
          class="flex w-11 items-center justify-center text-[var(--color-text-muted)] transition hover:bg-[var(--color-danger)] hover:text-[var(--color-accent-fg)]"
          title="关闭到托盘"
          @click.stop="closeWindow"
        >
          <X :size="15" />
        </button>
      </div>
    </div>
  </header>
</template>
