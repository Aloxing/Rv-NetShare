<script setup lang="ts">
import { computed } from 'vue';
import { Copy, ExternalLink, File, Folder, FolderOpen, Trash2 } from '@lucide/vue';
import { openPath, pushToast } from '../composables/useAppState';
import { formatBytes, formatTimestamp } from '../utils/format';
import type { ShareSession } from '../types';

const props = defineProps<{
  share: ShareSession;
  url: string;
  selected?: boolean;
}>();

const emit = defineEmits<{
  (e: 'select'): void;
  (e: 'remove'): void;
}>();

const isFolder = computed(() => props.share.kind === 'folder');
const displayPath = computed(() =>
  props.share.path
    .replace(/^\\\\\?\\UNC\\/, '\\\\')
    .replace(/^\\\\\?\\/, ''),
);

async function copyLink(url: string) {
  try {
    await navigator.clipboard.writeText(url);
    pushToast('success', '链接已复制');
  } catch (err) {
    pushToast('error', '复制失败：' + err);
  }
}

async function openInBrowser(url: string) {
  try {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url);
  } catch (err) {
    pushToast('error', '无法打开浏览器：' + err);
  }
}

async function showInFolder(path: string) {
  try {
    await openPath(path);
  } catch (err) {
    pushToast('error', '无法打开：' + err);
  }
}
</script>

<template>
  <div
    class="group flex cursor-pointer flex-col gap-2 rounded-xl border bg-[var(--color-bg-elevated)] p-3 transition"
    :class="selected
      ? 'border-[var(--color-accent)] bg-[var(--color-accent-soft)] ring-1 ring-[var(--color-accent)]/30'
      : 'border-[var(--color-border)] hover:border-[var(--color-border-strong)] hover:bg-[var(--color-bg-hover)]'"
    @click="emit('select')"
  >
    <div class="flex items-start gap-3">
      <div
        class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg"
        :class="isFolder ? 'bg-[var(--color-icon-warning-soft)] text-[var(--color-icon-warning)]' : 'bg-[var(--color-icon-accent-soft)] text-[var(--color-icon-accent)]'"
      >
        <Folder v-if="isFolder" :size="18" />
        <File v-else :size="18" />
      </div>
      <div class="min-w-0 flex-1">
        <div class="truncate text-[13.5px] font-semibold" :title="share.name">{{ share.name }}</div>
        <div class="mt-1 flex items-center gap-2">
          <span class="rounded-md bg-[var(--color-bg-panel)] px-1.5 py-0.5 font-mono text-[10.5px] tabular-nums text-[var(--color-text-subtle)]">{{ formatBytes(share.total_bytes || share.size) }}</span>
          <span class="text-[10.5px] text-[var(--color-text-subtle)]">{{ formatTimestamp(share.created_at) }}</span>
        </div>
      </div>
      <div class="flex items-center gap-0.5 opacity-70 transition group-hover:opacity-100">
        <button
          class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] hover:bg-transparent hover:text-[var(--color-text)]"
          title="复制链接"
          @click.stop="copyLink(url)"
        >
          <Copy :size="14" />
        </button>
        <button
          class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] hover:bg-transparent hover:text-[var(--color-text)]"
          title="打开预览"
          @click.stop="openInBrowser(url)"
        >
          <ExternalLink :size="14" />
        </button>
        <button
          class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] hover:bg-transparent hover:text-[var(--color-text)]"
          title="定位文件"
          @click.stop="showInFolder(share.path)"
        >
          <FolderOpen :size="14" />
        </button>
        <button
          class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] hover:bg-transparent hover:text-[var(--color-danger)]"
          title="停止分享"
          @click.stop="emit('remove')"
        >
          <Trash2 :size="14" />
        </button>
      </div>
    </div>
    <div class="flex items-center gap-2 rounded-lg bg-[var(--color-bg-panel)] px-3 py-1.5">
      <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--color-text-muted)]" :title="displayPath">{{ displayPath }}</span>
    </div>
  </div>
</template>
