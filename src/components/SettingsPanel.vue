<script setup lang="ts">
import { computed, ref } from 'vue';
import { Copy, FolderCog, HardDrive, Info, Laptop, Minus, Plus, RefreshCw, Type } from '@lucide/vue';
import { open } from '@tauri-apps/plugin-dialog';
import {
  pushToast,
  refreshLocalIp,
  setFontSize,
  setSaveDir,
  useAppState,
} from '../composables/useAppState';

const state = useAppState();
const saveDirInput = ref<string>(state.initial?.save_dir ?? '');
const submittingDir = ref(false);
const refreshingIp = ref(false);

const hostname = computed(() => state.initial?.hostname ?? '-');
const localIp = computed(() => state.initial?.local_ip ?? '-');
const port = computed(() => state.initial?.port ?? 0);
const rootUrl = computed(() =>
  state.initial ? 'http://' + state.initial.local_ip + ':' + state.initial.port + '/' : '-',
);
const rootPath = computed(() => state.initial?.save_dir ?? '-');

async function copy(text: string, label: string) {
  try {
    await navigator.clipboard.writeText(text);
    pushToast('success', '已复制 ' + label);
  } catch (e) {
    pushToast('error', '复制失败: ' + e);
  }
}

async function applySaveDir() {
  const v = saveDirInput.value.trim();
  if (!v) return;
  submittingDir.value = true;
  try {
    const resolved = await setSaveDir(v);
    saveDirInput.value = resolved;
    pushToast('success', '保存目录已更新');
  } catch (err) {
    pushToast('error', String(err));
  } finally {
    submittingDir.value = false;
  }
}

async function pickFolder() {
  try {
    const selected = await open({ directory: true });
    if (selected) saveDirInput.value = selected;
  } catch (err) {
    pushToast('error', String(err));
  }
}

function bumpFont(delta: number) { setFontSize(state.fontSize + delta); }

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
</script>

<template>
  <section class="flex h-full flex-col overflow-hidden">
    <div class="flex-1 overflow-auto p-4">
      <div class="mx-auto flex w-full flex-col gap-3">
        <div class="overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-elevated)] shadow-[var(--shadow-card)]">
          <div class="flex h-10 items-center justify-between border-b border-[var(--color-border-soft)] px-4">
            <div class="flex items-center gap-2.5">
              <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-[var(--color-icon-accent-soft)] text-[var(--color-icon-accent)]">
                <Laptop :size="14" />
              </div>
              <span class="text-[12.5px] font-semibold">身份</span>
            </div>
            <button
              class="inline-flex h-7 items-center gap-1.5 rounded-lg border border-[var(--color-border)] bg-transparent px-2.5 text-[11.5px] text-[var(--color-text-muted)] transition hover:bg-transparent hover:text-[var(--color-text)] disabled:opacity-60"
              title="刷新 IP"
              :disabled="refreshingIp"
              @click="onRefreshIp"
            >
              <RefreshCw :size="12" :class="refreshingIp ? 'animate-spin' : ''" />
              刷新 IP
            </button>
          </div>
          <div class="divide-y divide-[var(--color-border-soft)]">
            <div v-for="row in [
              { label: '主机名', value: hostname, copyLabel: '主机名' },
              { label: '局域网 IP', value: localIp, copyLabel: 'IP' },
              { label: '监听端口', value: port + '（被占用时自动 +1）', copyLabel: '' },
              { label: '根地址', value: rootUrl, copyLabel: '根地址' },
            ]" :key="row.label" class="flex items-center justify-between gap-3 px-4 py-2.5">
              <dt class="shrink-0 text-[11.5px] text-[var(--color-text-muted)]">{{ row.label }}</dt>
              <dd class="flex min-w-0 items-center gap-2">
                <span class="truncate font-mono text-[12px] text-[var(--color-text)]" :title="String(row.value)">{{ row.value }}</span>
                <button
                  v-if="row.copyLabel"
                  class="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-[var(--color-text-subtle)] transition hover:bg-transparent hover:text-[var(--color-text)]"
                  @click="copy(String(row.value), row.copyLabel)"
                  title="复制"
                >
                  <Copy :size="12" />
                </button>
              </dd>
            </div>
          </div>
        </div>

        <div class="overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-elevated)] shadow-[var(--shadow-card)]">
          <div class="flex h-10 items-center gap-2.5 border-b border-[var(--color-border-soft)] px-4">
            <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-[var(--color-icon-accent-soft)] text-[var(--color-icon-accent)]">
              <HardDrive :size="14" />
            </div>
            <span class="text-[12.5px] font-semibold">下载保存目录</span>
          </div>
          <div class="p-3.5">
            <div class="flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-panel)] px-3 py-2">
              <FolderCog :size="14" class="shrink-0 text-[var(--color-text-subtle)]" />
              <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--color-text-muted)]" :title="rootPath">{{ rootPath }}</span>
            </div>
            <form class="mt-2.5 flex gap-2" @submit.prevent="applySaveDir">
              <input
                v-model="saveDirInput"
                type="text"
                placeholder="D:\downloads"
                class="h-9 min-w-0 flex-1 rounded-lg border border-[var(--color-input)] bg-[var(--color-bg)] px-3 font-mono text-[11.5px] text-[var(--color-text)] outline-none transition placeholder:text-[var(--color-text-subtle)] focus:border-[var(--color-ring)] focus:ring-2 focus:ring-[var(--color-ring)]/20"
                :disabled="submittingDir"
              />
              <button type="button" class="inline-flex h-9 items-center rounded-lg border border-[var(--color-border)] bg-transparent px-3 text-[11.5px] text-[var(--color-text)] transition hover:bg-transparent" @click="pickFolder">浏览</button>
              <button type="submit" class="inline-flex h-9 items-center rounded-lg bg-transparent px-3.5 text-[11.5px] font-medium text-[var(--color-accent)] transition hover:bg-transparent hover:text-[var(--color-accent-hover)] disabled:opacity-50" :disabled="submittingDir || !saveDirInput.trim()">应用</button>
            </form>
          </div>
        </div>

        <div class="overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-elevated)] shadow-[var(--shadow-card)]">
          <div class="flex h-10 items-center gap-2.5 border-b border-[var(--color-border-soft)] px-4">
              <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-[var(--color-icon-accent-soft)] text-[var(--color-icon-accent)]">
              <Type :size="14" />
            </div>
            <span class="text-[12.5px] font-semibold">界面字体</span>
          </div>
          <div class="flex items-center gap-4 px-4 py-3.5">
            <button class="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--color-border)] bg-transparent text-[var(--color-text-muted)] transition hover:bg-transparent hover:text-[var(--color-text)]" @click="bumpFont(-1)" title="缩小">
              <Minus :size="14" />
            </button>
            <div class="flex-1 text-center">
              <div class="font-mono font-semibold" :style="{ fontSize: (state.fontSize + 6) + 'px' }">Aa</div>
              <div class="mt-1 text-[11px] tabular-nums text-[var(--color-text-muted)]">{{ state.fontSize }} px</div>
            </div>
            <button class="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--color-border)] bg-transparent text-[var(--color-text-muted)] transition hover:bg-transparent hover:text-[var(--color-text)]" @click="bumpFont(1)" title="放大">
              <Plus :size="14" />
            </button>
          </div>
        </div>

        <div class="overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-elevated)] shadow-[var(--shadow-card)]">
          <div class="flex h-10 items-center gap-2.5 border-b border-[var(--color-border-soft)] px-4">
            <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-[var(--color-icon-accent-soft)] text-[var(--color-icon-accent)]">
              <Info :size="14" />
            </div>
            <span class="text-[12.5px] font-semibold">关于</span>
          </div>
          <div class="flex flex-col gap-1.5 p-3.5">
            <div class="flex items-center justify-between rounded-lg bg-[var(--color-bg-panel)] px-3 py-2 text-[12px]">
              <span class="text-[var(--color-text-muted)]">版本</span>
              <span class="font-mono text-[var(--color-text)]">v0.1.0</span>
            </div>
            <div class="flex items-center justify-between rounded-lg bg-[var(--color-bg-panel)] px-3 py-2 text-[12px]">
              <span class="text-[var(--color-text-muted)]">本地数据</span>
              <span class="font-mono text-[var(--color-text)]">shares.json · history.json</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
</template>
