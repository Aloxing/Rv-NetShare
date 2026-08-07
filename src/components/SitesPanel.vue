<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { FolderPlus, Globe, Link2, Plus, Trash2, Upload } from '@lucide/vue';
import { open } from '@tauri-apps/plugin-dialog';
import {
  buildSiteUrl,
  clearSites,
  createSite,
  pushToast,
  removeSite,
  setDropHandler,
  startSiteTunnel,
  stopSiteTunnel,
  useAppState,
} from '../composables/useAppState';
import ShareCard from './ShareCard.vue';
import type { SiteSession } from '../types';

const state = useAppState();
const pathInput = ref<string>('');
const submitting = ref(false);
const selectedId = ref<string | null>(null);
const tunnelBusyId = ref<string | null>(null);

const urlFor = (site: SiteSession) =>
  buildSiteUrl(
    state.initial?.local_ip ?? '127.0.0.1',
    site.port,
  );

const ngrokUrlFor = (site: SiteSession) => state.ngrokUrls['site:' + site.id];

async function toggleSiteTunnel(site: SiteSession) {
  if (tunnelBusyId.value === site.id) return;
  if (!state.initial?.ngrok_authtoken && !state.ngrokUrls['site:' + site.id]) {
    pushToast('error', '请先在设置中配置 ngrok Authtoken');
    return;
  }
  tunnelBusyId.value = site.id;
  try {
    if (state.ngrokUrls['site:' + site.id]) {
      await stopSiteTunnel(site.id);
      pushToast('success', '已关闭公网访问');
    } else {
      await startSiteTunnel(site.id);
      pushToast('success', '公网地址已生成');
    }
  } catch (err) {
    pushToast('error', String(err));
  } finally {
    tunnelBusyId.value = null;
  }
}

async function addSitePath(raw: string) {
  const path = raw.trim();
  if (!path) return;
  try {
    const session = await createSite(path);
    pushToast('success', '站点已添加：' + session.name);
  } catch (err) {
    pushToast('error', String(err));
  }
}

async function addSitePaths(paths: string[]) {
  const cleaned = paths.map((p) => p.trim()).filter((p) => p.length > 0);
  for (const path of cleaned) {
    await addSitePath(path);
  }
}

async function pickFolders() {
  try {
    const selected = await open({ multiple: true, directory: true });
    if (selected) await addSitePaths(Array.isArray(selected) ? selected : [selected]);
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
    await addSitePaths(lines);
    pathInput.value = '';
  } finally {
    submitting.value = false;
  }
}

async function stopSite(id: string) {
  await removeSite(id);
  if (selectedId.value === id) selectedId.value = null;
}

async function stopAll() {
  if (!confirm('确定移除全部站点？')) return;
  await clearSites();
  selectedId.value = null;
}

onMounted(() => {
  setDropHandler((paths) => { void addSitePaths(paths); });
});
onBeforeUnmount(() => {
  setDropHandler(null);
});
</script>

<template>
  <section class="flex h-full flex-col overflow-hidden">
    <div class="flex h-14 shrink-0 items-center gap-2.5 border-b border-[var(--color-border-soft)] bg-[var(--color-bg-elevated)] px-5">
      <button
        class="inline-flex h-9 items-center gap-2 rounded-lg bg-transparent px-3.5 text-[12.5px] font-medium text-[var(--color-accent)] transition hover:bg-[var(--color-accent-soft)] hover:text-[var(--color-accent-hover)] active:text-[var(--color-accent-pressed)]"
        @click="pickFolders"
      >
        <FolderPlus :size="15" />
        选择文件夹
      </button>
      <div class="mx-1 h-5 w-px bg-[var(--color-border)]"></div>
      <form class="flex flex-1 items-center gap-2" @submit.prevent="submitPath">
        <div class="rv-input-wrap min-w-0 flex-1">
          <Link2 :size="14" class="shrink-0 text-[var(--color-text-subtle)]" />
          <input
            v-model="pathInput"
            type="text"
            placeholder="粘贴包含 index.html 的文件夹路径"
            class="rv-input text-[12.5px]"
            :disabled="submitting"
          />
        </div>
        <button
          type="submit"
          class="inline-flex h-9 items-center gap-2 rounded-lg border border-[var(--color-border)] bg-transparent px-3.5 text-[12.5px] font-medium text-[var(--color-text)] transition hover:bg-[var(--color-bg-hover)] active:bg-[var(--color-bg-active)] disabled:opacity-50"
          :disabled="submitting || !pathInput.trim()"
        >
          <Plus :size="14" />
          添加
        </button>
      </form>
      <button
        class="inline-flex h-9 items-center gap-2 rounded-lg px-3 text-[12.5px] text-[var(--color-text-muted)] transition hover:bg-[var(--color-danger-soft)] hover:text-[var(--color-danger)] disabled:opacity-50"
        :disabled="!state.sites.length"
        @click="stopAll"
      >
        <Trash2 :size="14" />
        清空
      </button>
    </div>

    <div class="flex-1 overflow-auto">
      <div class="w-full px-3 py-3">
        <div v-if="state.sites.length" class="grid grid-cols-1 gap-2">
          <ShareCard
            v-for="site in state.sites"
            :key="site.id"
            :share="site"
            :url="urlFor(site)"
            :ngrok-url="ngrokUrlFor(site)"
            :selected="selectedId === site.id"
            :tunnel-busy="tunnelBusyId === site.id"
            remove-label="移除站点"
            @select="selectedId = site.id"
            @remove="stopSite(site.id)"
            @toggle-tunnel="toggleSiteTunnel(site)"
          />
        </div>

        <div v-else class="flex flex-col items-center justify-center py-24 text-center">
          <div class="flex h-16 w-16 items-center justify-center rounded-2xl border border-dashed border-[var(--color-border-strong)] bg-[var(--color-bg-elevated)] text-[var(--color-text-subtle)]">
            <Globe :size="26" />
          </div>
          <div class="mt-4 text-[14px] font-semibold">暂无站点</div>
          <div class="mt-1 flex items-center gap-2 text-[12px] text-[var(--color-text-subtle)]">
            <Upload :size="13" />
            添加包含 index.html 的文件夹
          </div>
        </div>
      </div>
    </div>
  </section>
</template>
