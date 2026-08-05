<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { FileUp, FolderPlus, FolderUp, Link2, Plus, RefreshCw, Trash2, Upload } from '@lucide/vue';
import { open } from '@tauri-apps/plugin-dialog';
import {
  buildShareUrl,
  clearShares,
  pushToast,
  refreshLocalIp,
  removeShare,
  resolveSharePaths,
  setDropHandler,
  useAppState,
} from '../composables/useAppState';
import ShareCard from './ShareCard.vue';
import type { ShareSession } from '../types';

const state = useAppState();
const pathInput = ref<string>('');
const submitting = ref(false);
const selectedId = ref<string | null>(null);
const refreshingIp = ref(false);

const urlFor = (share: ShareSession) =>
  buildShareUrl(
    state.initial?.local_ip ?? '127.0.0.1',
    state.initial?.port ?? 0,
    share.id,
  );

async function sharePaths(paths: string[]) {
  const cleaned = paths.map((p) => p.trim()).filter((p) => p.length > 0);
  if (!cleaned.length) return;

  let sessions: ShareSession[] = [];
  try {
    sessions = await resolveSharePaths(cleaned);
  } catch (err) {
    pushToast('error', String(err));
    return;
  }

  if (!sessions.length) {
    pushToast('error', '路径无效或不存在');
    return;
  }
  const bad = cleaned.length - sessions.length;
  if (bad > 0) {
    pushToast('info', '已创建 ' + sessions.length + ' 个分享，' + bad + ' 个路径无效');
  } else {
    pushToast('success', '已创建 ' + sessions.length + ' 个分享');
  }
}

async function pickFiles() {
  try {
    const selected = await open({ multiple: true, directory: false });
    if (selected) await sharePaths(Array.isArray(selected) ? selected : [selected]);
  } catch (err) {
    pushToast('error', String(err));
  }
}

async function pickFolder() {
  try {
    const selected = await open({ multiple: true, directory: true });
    if (selected) await sharePaths(Array.isArray(selected) ? selected : [selected]);
  } catch (err) {
    pushToast('error', String(err));
  }
}

async function submitPath() {
  const raw = pathInput.value;
  if (!raw.trim()) return;
  submitting.value = true;
  try {
    const lines = raw.split(/[\r\n]+/).map((l) => l.trim()).filter((l) => l.length > 0);
    await sharePaths(lines);
    pathInput.value = '';
  } finally {
    submitting.value = false;
  }
}

async function stopShare(id: string) {
  await removeShare(id);
  if (selectedId.value === id) selectedId.value = null;
}

async function stopAll() {
  if (!confirm('确定停止全部分享？')) return;
  await clearShares();
  selectedId.value = null;
}

async function onRefreshIp() {
  if (refreshingIp.value) return;
  refreshingIp.value = true;
  try {
    const ip = await refreshLocalIp();
    if (ip) {
      pushToast('success', 'IP 已刷新');
    } else {
      pushToast('error', '刷新失败');
    }
  } finally {
    refreshingIp.value = false;
  }
}

onMounted(() => {
  setDropHandler((paths) => { void sharePaths(paths); });
});
onBeforeUnmount(() => {
  setDropHandler(null);
});

const fileShares = computed(() => state.shares.filter((s) => s.kind === 'file'));
const folderShares = computed(() => state.shares.filter((s) => s.kind === 'folder'));
</script>

<template>
  <section class="flex h-full flex-col overflow-hidden">
    <div class="flex h-14 shrink-0 items-center gap-2.5 border-b border-[var(--color-border-soft)] bg-[var(--color-bg-elevated)] px-5">
      <button
        class="inline-flex h-9 items-center gap-2 rounded-lg bg-transparent px-3.5 text-[12.5px] font-medium text-[var(--color-accent)] transition hover:bg-transparent hover:text-[var(--color-accent-hover)] active:text-[var(--color-accent-pressed)] disabled:opacity-50"
        @click="pickFiles"
      >
        <Upload :size="15" />
        选择文件
      </button>
      <button
        class="inline-flex h-9 items-center gap-2 rounded-lg border border-[var(--color-border)] bg-transparent px-3.5 text-[12.5px] font-medium text-[var(--color-text)] transition hover:bg-transparent active:bg-transparent"
        @click="pickFolder"
      >
        <FolderPlus :size="15" />
        选择文件夹
      </button>
      <div class="mx-1 h-5 w-px bg-[var(--color-border)]"></div>
      <form class="flex flex-1 items-center gap-2" @submit.prevent="submitPath">
        <div class="flex h-9 flex-1 items-center gap-2 rounded-lg border border-[var(--color-input)] bg-[var(--color-bg)] px-3 transition focus-within:border-[var(--color-ring)] focus-within:ring-2 focus-within:ring-[var(--color-ring)]/20">
          <Link2 :size="14" class="shrink-0 text-[var(--color-text-subtle)]" />
          <input
            v-model="pathInput"
            type="text"
            placeholder="粘贴文件或文件夹路径"
            class="flex-1 bg-transparent text-[12.5px] text-[var(--color-text)] outline-none placeholder:text-[var(--color-text-subtle)]"
            :disabled="submitting"
          />
        </div>
        <button
          type="submit"
          class="inline-flex h-9 items-center gap-2 rounded-lg border border-[var(--color-border)] bg-transparent px-3.5 text-[12.5px] font-medium text-[var(--color-text)] transition hover:bg-transparent active:bg-transparent disabled:opacity-50"
          :disabled="submitting || !pathInput.trim()"
        >
          <Plus :size="14" />
          添加
        </button>
      </form>
      <button
        class="inline-flex h-9 items-center gap-2 rounded-lg px-3 text-[12.5px] text-[var(--color-text-muted)] transition hover:bg-transparent hover:text-[var(--color-danger)] disabled:opacity-50"
        :disabled="!state.shares.length"
        @click="stopAll"
      >
        <Trash2 :size="14" />
        清空
      </button>
      <div class="mx-1 h-5 w-px bg-[var(--color-border)]"></div>
      <button
        class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-[var(--color-border)] bg-transparent text-[var(--color-text-muted)] transition hover:bg-transparent hover:text-[var(--color-text)] disabled:opacity-60"
        title="刷新 IP"
        :disabled="refreshingIp"
        @click="onRefreshIp"
      >
        <RefreshCw :size="14" :class="refreshingIp ? 'animate-spin' : ''" />
      </button>
    </div>

    <div class="flex-1 overflow-auto">
      <div class="w-full px-3 py-3">
        <div v-if="folderShares.length" class="mb-3">
          <div class="mb-2 flex items-center gap-2 px-1">
            <FolderUp :size="14" class="text-[var(--color-icon-warning)]" />
            <span class="text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--color-text-subtle)]">文件夹</span>
            <span class="rounded-full bg-[var(--color-bg-hover)] px-2 py-0.5 text-[10.5px] font-semibold tabular-nums text-[var(--color-text-muted)]">{{ folderShares.length }}</span>
          </div>
          <div class="grid grid-cols-1 gap-2">
            <ShareCard
              v-for="share in folderShares"
              :key="share.id"
              :share="share"
              :url="urlFor(share)"
              :selected="selectedId === share.id"
              @select="selectedId = share.id"
              @remove="stopShare(share.id)"
            />
          </div>
        </div>

        <div v-if="fileShares.length">
          <div class="mb-2 flex items-center gap-2 px-1">
            <FileUp :size="14" class="text-[var(--color-icon-accent)]" />
            <span class="text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--color-text-subtle)]">文件</span>
            <span class="rounded-full bg-[var(--color-bg-hover)] px-2 py-0.5 text-[10.5px] font-semibold tabular-nums text-[var(--color-text-muted)]">{{ fileShares.length }}</span>
          </div>
          <div class="grid grid-cols-1 gap-2">
            <ShareCard
              v-for="share in fileShares"
              :key="share.id"
              :share="share"
              :url="urlFor(share)"
              :selected="selectedId === share.id"
              @select="selectedId = share.id"
              @remove="stopShare(share.id)"
            />
          </div>
        </div>

        <div v-if="!state.shares.length" class="flex flex-col items-center justify-center py-24 text-center">
          <div class="flex h-16 w-16 items-center justify-center rounded-2xl border border-dashed border-[var(--color-border-strong)] bg-[var(--color-bg-elevated)] text-[var(--color-text-subtle)]">
            <Upload :size="26" />
          </div>
          <div class="mt-4 text-[14px] font-semibold">暂无分享</div>
        </div>
      </div>
    </div>
  </section>
</template>
