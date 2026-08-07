<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import {
  Archive,
  Copy,
  ExternalLink,
  File,
  FolderOpen,
  Globe,
  Image,
  Inbox,
  KeyRound,
  Link2,
  Lock,
  Pencil,
  QrCode,
  Trash2,
  X,
} from '@lucide/vue';
import { toDataURL } from 'qrcode';
import { openReceiveDir, pushToast } from '../composables/useAppState';
import { formatBytes } from '../utils/format';
import type { ReceiveSession } from '../types';

const props = defineProps<{
  receiver: ReceiveSession;
  url: string;
  ngrokUrl?: string;
  tunnelBusy?: boolean;
}>();

const emit = defineEmits<{
  (e: 'remove'): void;
  (e: 'edit'): void;
  (e: 'toggle-tunnel'): void;
}>();

const allowedLabel = computed(() => {
  if (props.receiver.extensions.includes('*')) return '全部文件';
  return props.receiver.extensions.map((ext) => '.' + ext).join(' ');
});

const PHOTOS = ['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'heic', 'heif', 'svg', 'ico'];
const ARCHIVES = ['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'zst'];

const typeIcon = computed(() => {
  if (props.receiver.extensions.includes('*')) return Inbox;
  const exts = props.receiver.extensions;
  if (exts.every((ext) => PHOTOS.includes(ext))) return Image;
  if (exts.every((ext) => ARCHIVES.includes(ext))) return Archive;
  return File;
});

const encryptionLabel = computed(() => {
  if (props.receiver.encryption === 'none') return '不加密';
  if (props.receiver.encryption === 'common') return '通用加密';
  return '单独加密';
});

const encryptionIcon = computed(() => {
  if (props.receiver.encryption === 'none') return Lock;
  return KeyRound;
});

const qrOpen = ref(false);
const qrMode = ref<'local' | 'public'>('local');
const activeQrUrl = computed(() =>
  qrMode.value === 'public' && props.ngrokUrl ? props.ngrokUrl : props.url,
);
const qrUrl = ref(props.url);
const qrDataUrl = ref('');
const qrLoading = ref(false);
const qrError = ref('');
let qrKeydown: ((e: KeyboardEvent) => void) | null = null;

async function renderQr() {
  qrUrl.value = activeQrUrl.value;
  qrLoading.value = true;
  qrError.value = '';
  qrDataUrl.value = '';
  try {
    qrDataUrl.value = await toDataURL(activeQrUrl.value, {
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

async function openQrPreview() {
  qrMode.value = 'local';
  qrOpen.value = true;
  await renderQr();
}

async function selectQrMode(mode: 'local' | 'public') {
  if (qrMode.value === mode) return;
  qrMode.value = mode;
  await renderQr();
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

watch([() => props.url, () => props.ngrokUrl], () => {
  if (qrOpen.value) void renderQr();
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

async function openFolder() {
  try {
    await openReceiveDir(props.receiver.id);
  } catch (err) {
    pushToast('error', '无法打开接收目录：' + err);
  }
}
</script>

<template>
  <div
    class="group flex cursor-pointer flex-col gap-2 rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-elevated)] p-3 transition hover:border-[var(--color-border-strong)] hover:bg-[var(--color-bg-hover)]"
  >
    <div class="flex items-start gap-3">
      <div class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-[var(--color-icon-accent-soft)] text-[var(--color-icon-accent)]">
        <component :is="typeIcon" :size="18" />
      </div>
      <div class="min-w-0 flex-1">
        <div class="truncate text-[13.5px] font-semibold" :title="receiver.name">{{ receiver.name }}</div>
        <div class="mt-1 flex flex-wrap items-center gap-2">
          <span class="truncate rounded-md bg-[var(--color-bg-panel)] px-1.5 py-0.5 font-mono text-[10.5px] text-[var(--color-text-subtle)]" :title="allowedLabel">{{ allowedLabel }}</span>
          <span class="rounded-md bg-[var(--color-bg-panel)] px-1.5 py-0.5 text-[10.5px] text-[var(--color-text-subtle)]">
            {{ receiver.received_count }} 个 · {{ formatBytes(receiver.received_bytes) }}
          </span>
        </div>
      </div>
      <div class="flex items-center gap-0.5 opacity-70 transition group-hover:opacity-100">
        <button
          class="flex h-7 w-7 items-center justify-center rounded-md transition disabled:opacity-50"
          :class="ngrokUrl
            ? 'bg-[var(--color-accent-soft)] text-[var(--color-accent)] hover:bg-[var(--color-accent)] hover:text-[var(--color-accent-fg)]'
            : 'text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]'"
          :title="ngrokUrl ? '关闭公网访问' : '开启公网访问'"
          :disabled="tunnelBusy"
          @click.stop="emit('toggle-tunnel')"
        >
          <Globe :size="14" />
        </button>
        <button
          class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
          title="二维码"
          @click.stop="openQrPreview"
        >
          <QrCode :size="14" />
        </button>
        <button
          class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
          title="接收目录"
          @click.stop="openFolder"
        >
          <FolderOpen :size="14" />
        </button>
        <button
          class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
          title="编辑接收卡片"
          @click.stop="emit('edit')"
        >
          <Pencil :size="14" />
        </button>
        <button
          class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] transition hover:bg-[var(--color-danger-soft)] hover:text-[var(--color-danger)]"
          title="删除接收卡片"
          @click.stop="emit('remove')"
        >
          <Trash2 :size="14" />
        </button>
      </div>
    </div>

    <div class="flex items-center gap-2 rounded-lg bg-[var(--color-bg-panel)] px-3 py-1.5">
      <component :is="encryptionIcon" :size="12" class="shrink-0 text-[var(--color-text-subtle)]" />
      <span class="shrink-0 rounded bg-[var(--color-bg-hover)] px-1.5 py-0.5 text-[9.5px] font-semibold text-[var(--color-text-muted)]">{{ encryptionLabel }}</span>
      <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--color-text-muted)]" :title="url">{{ url }}</span>
      <button
        class="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-[var(--color-text-subtle)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
        title="复制本地链接"
        @click.stop="copyLink(url)"
      >
        <Copy :size="12" />
      </button>
      <button
        class="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-[var(--color-text-subtle)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
        title="打开本地链接"
        @click.stop="openInBrowser(url)"
      >
        <ExternalLink :size="12" />
      </button>
    </div>

    <div v-if="ngrokUrl" class="flex items-center gap-2 rounded-lg border border-[var(--color-accent)]/30 bg-[var(--color-accent-soft)] px-3 py-1.5">
      <Globe :size="12" class="shrink-0 text-[var(--color-accent)]" />
      <span class="shrink-0 rounded bg-[var(--color-bg-hover)] px-1.5 py-0.5 text-[9.5px] font-semibold text-[var(--color-accent)]">公网</span>
      <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--color-text)]" :title="ngrokUrl">{{ ngrokUrl }}</span>
      <button
        class="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-[var(--color-text-subtle)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
        title="复制公网链接"
        @click.stop="copyLink(ngrokUrl)"
      >
        <Copy :size="12" />
      </button>
      <button
        class="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-[var(--color-text-subtle)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
        title="打开公网链接"
        @click.stop="openInBrowser(ngrokUrl)"
      >
        <ExternalLink :size="12" />
      </button>
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
          <div v-if="ngrokUrl" class="mb-3 flex items-center gap-1 rounded-lg bg-[var(--color-bg-panel)] p-1">
            <button
              class="flex h-7 flex-1 items-center justify-center gap-1.5 rounded-md text-[11.5px] font-medium transition disabled:opacity-50"
              :class="qrMode === 'local'
                ? 'bg-[var(--color-bg-elevated)] text-[var(--color-text)] shadow-sm'
                : 'text-[var(--color-text-muted)] hover:text-[var(--color-text)]'"
              :disabled="qrLoading"
              @click="selectQrMode('local')"
            >
              <Link2 :size="12" />
              本地
            </button>
            <button
              class="flex h-7 flex-1 items-center justify-center gap-1.5 rounded-md text-[11.5px] font-medium transition disabled:opacity-50"
              :class="qrMode === 'public'
                ? 'bg-[var(--color-bg-elevated)] text-[var(--color-accent)] shadow-sm'
                : 'text-[var(--color-text-muted)] hover:text-[var(--color-text)]'"
              :disabled="qrLoading"
              @click="selectQrMode('public')"
            >
              <Globe :size="12" />
              公网
            </button>
          </div>
          <div class="flex items-center justify-center rounded-lg border border-[var(--color-border)] bg-white p-4">
            <img v-if="qrDataUrl" :src="qrDataUrl" alt="接收二维码" class="h-56 w-56" />
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
