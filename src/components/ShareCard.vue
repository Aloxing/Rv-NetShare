<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { Copy, ExternalLink, File, Folder, FolderOpen, QrCode, Trash2, X } from '@lucide/vue';
import { toDataURL } from 'qrcode';
import { openPath, pushToast } from '../composables/useAppState';
import { formatBytes, formatTimestamp } from '../utils/format';
import type { ShareSession } from '../types';

const props = defineProps<{
  share: ShareSession;
  url: string;
  selected?: boolean;
  removeLabel?: string;
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

const qrOpen = ref(false);
const qrUrl = ref(props.url);
const qrDataUrl = ref('');
const qrLoading = ref(false);
const qrError = ref('');
let qrKeydown: ((e: KeyboardEvent) => void) | null = null;

async function openQrPreview() {
  qrUrl.value = props.url;
  qrOpen.value = true;
  qrLoading.value = true;
  qrError.value = '';
  qrDataUrl.value = '';
  try {
    qrDataUrl.value = await toDataURL(props.url, {
      width: 280,
      margin: 2,
      errorCorrectionLevel: 'M',
      color: { dark: '#000000', light: '#ffffff' },
    });
  } catch (err) {
    qrError.value = '二维码生成失败：' + String(err);
  } finally {
    qrLoading.value = false;
  }
}

function closeQrPreview() {
  qrOpen.value = false;
  qrDataUrl.value = '';
  qrError.value = '';
}

watch(qrOpen, (open) => {
  if (open) {
    qrKeydown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') closeQrPreview();
    };
    window.addEventListener('keydown', qrKeydown);
  } else if (qrKeydown) {
    window.removeEventListener('keydown', qrKeydown);
    qrKeydown = null;
  }
});

watch(() => props.url, () => {
  if (qrOpen.value) void openQrPreview();
});

onBeforeUnmount(() => {
  if (qrKeydown) {
    window.removeEventListener('keydown', qrKeydown);
    qrKeydown = null;
  }
});

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
          class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
          title="二维码"
          @click.stop="openQrPreview"
        >
          <QrCode :size="14" />
        </button>
        <button
          class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
          title="复制链接"
          @click.stop="copyLink(url)"
        >
          <Copy :size="14" />
        </button>
        <button
          class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
          title="打开预览"
          @click.stop="openInBrowser(url)"
        >
          <ExternalLink :size="14" />
        </button>
        <button
          class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
          title="定位文件"
          @click.stop="showInFolder(share.path)"
        >
          <FolderOpen :size="14" />
        </button>
        <button
          class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] transition hover:bg-[var(--color-danger-soft)] hover:text-[var(--color-danger)]"
          :title="props.removeLabel || '停止分享'"
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

  <Teleport to="body">
    <div
      v-if="qrOpen"
      class="fixed inset-0 z-[70] flex items-center justify-center bg-black/45 p-4 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label="二维码"
      @click.self="closeQrPreview"
    >
      <div class="anim-rise w-[340px] max-w-full overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-elevated)] shadow-[var(--shadow-popup)]">
        <div class="flex h-12 items-center justify-between border-b border-[var(--color-border-soft)] px-4">
          <div class="flex items-center gap-2 text-[13px] font-semibold">
            <QrCode :size="15" class="text-[var(--color-icon-accent)]" />
            二维码
          </div>
          <button
            class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
            title="关闭"
            @click="closeQrPreview"
          >
            <X :size="15" />
          </button>
        </div>
        <div class="p-4">
          <div class="flex items-center justify-center rounded-lg border border-[var(--color-border)] bg-white p-4">
            <img v-if="qrDataUrl" :src="qrDataUrl" alt="分享二维码" class="h-56 w-56" />
            <div v-else-if="qrLoading" class="flex h-56 w-56 items-center justify-center text-[12px] text-[var(--color-text-muted)]">生成中</div>
            <div v-else-if="qrError" class="flex h-56 w-56 items-center justify-center px-4 text-center text-[12px] text-[var(--color-danger)]">{{ qrError }}</div>
          </div>
          <div class="mt-3 flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-panel)] px-3 py-2">
            <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--color-text-muted)]" :title="qrUrl">{{ qrUrl }}</span>
            <button
              class="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-[var(--color-text-subtle)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
              title="复制链接"
              @click="copyLink(qrUrl)"
            >
              <Copy :size="12" />
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
