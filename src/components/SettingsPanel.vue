<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { Copy, ExternalLink, FolderCog, Globe, HardDrive, Info, Laptop, Minus, Plus, RadioTower, RefreshCw, Settings, Type, Waypoints } from '@lucide/vue';
import { open } from '@tauri-apps/plugin-dialog';
import {
  pushToast,
  refreshLocalIp,
  setFontSize,
  setNgrokAuthtoken,
  setSaveDir,
  setSharePort,
  setSitePort,
  setThemeMode,
  useAppState,
} from '../composables/useAppState';

type SectionKey = 'identity' | 'storage' | 'ports' | 'tunnel' | 'appearance' | 'about';

const sectionItems = [
  { key: 'identity', label: '身份', icon: Laptop },
  { key: 'storage', label: '存储', icon: HardDrive },
  { key: 'ports', label: '端口', icon: RadioTower },
  { key: 'tunnel', label: '穿透', icon: Waypoints },
  { key: 'appearance', label: '界面', icon: Type },
  { key: 'about', label: '关于', icon: Info },
] as const;

const themeOptions = [
  { key: 'light', label: '浅色' },
  { key: 'dark', label: '深色' },
  { key: 'system', label: '跟随系统' },
] as const;

const state = useAppState();
const activeSection = ref<SectionKey>('identity');
const saveDirInput = ref<string>(state.initial?.save_dir ?? '');
const submittingDir = ref(false);
const refreshingIp = ref(false);
const sharePortInput = ref<string>(String(state.initial?.share_port ?? 48721));
const sitePortInput = ref<string>(String(state.initial?.site_port ?? 48800));
const submittingSharePort = ref(false);
const submittingSitePort = ref(false);
const authtokenInput = ref<string>(state.initial?.ngrok_authtoken ?? '');
const applyingToken = ref(false);

const hostname = computed(() => state.initial?.hostname ?? '-');
const localIp = computed(() => state.initial?.local_ip ?? '-');
const port = computed(() => state.initial?.port ?? 0);
const rootUrl = computed(() =>
  state.initial ? 'http://' + state.initial.local_ip + ':' + state.initial.port + '/' : '-',
);
const rootPath = computed(() => state.initial?.save_dir ?? '-');

watch(() => state.initial, (initial) => {
  if (initial) {
    saveDirInput.value = initial.save_dir;
    sharePortInput.value = String(initial.share_port ?? 48721);
    sitePortInput.value = String(initial.site_port ?? 48800);
    authtokenInput.value = initial.ngrok_authtoken ?? '';
  }
}, { immediate: true });

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

function parsePort(raw: string): number | null {
  const n = Number.parseInt(raw, 10);
  return Number.isInteger(n) && n >= 1 && n <= 65535 ? n : null;
}

async function applySharePort() {
  const p = parsePort(sharePortInput.value);
  if (p === null) {
    pushToast('error', '端口无效');
    return;
  }
  submittingSharePort.value = true;
  try {
    const bound = await setSharePort(p);
    sharePortInput.value = String(bound);
    pushToast('success', '分享端口已更新');
  } catch (err) {
    pushToast('error', String(err));
  } finally {
    submittingSharePort.value = false;
  }
}

async function applySitePort() {
  const p = parsePort(sitePortInput.value);
  if (p === null) {
    pushToast('error', '端口无效');
    return;
  }
  submittingSitePort.value = true;
  try {
    const bound = await setSitePort(p);
    sitePortInput.value = String(bound);
    pushToast('success', '站点端口已更新');
  } catch (err) {
    pushToast('error', String(err));
  } finally {
    submittingSitePort.value = false;
  }
}

async function resetPorts() {
  if (submittingSharePort.value || submittingSitePort.value) return;
  sharePortInput.value = '48721';
  sitePortInput.value = '48800';
  await applySharePort();
  await applySitePort();
}

async function saveNgrokToken() {
  if (applyingToken.value) return;
  applyingToken.value = true;
  try {
    await setNgrokAuthtoken(authtokenInput.value);
    pushToast('success', 'ngrok Authtoken 已保存');
  } catch (err) {
    pushToast('error', String(err));
  } finally {
    applyingToken.value = false;
  }
}

async function openNgrokSite() {
  try {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl('https://dashboard.ngrok.com/get-started/your-authtoken');
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
    <header class="flex h-14 shrink-0 items-center gap-2 border-b border-[var(--color-border-soft)] bg-[var(--color-bg-elevated)] px-5">
      <Settings :size="15" class="text-[var(--color-icon-accent)]" />
      <span class="text-[13px] font-semibold">设置</span>
    </header>

    <div class="flex flex-1 flex-col overflow-hidden p-4">
      <div class="flex h-full w-full flex-col">
        <div class="mb-3 flex h-11 shrink-0 items-center gap-1 overflow-x-auto rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-elevated)] p-1 shadow-[var(--shadow-card)]">
          <button
            v-for="section in sectionItems"
            :key="section.key"
            class="flex h-8 flex-1 items-center justify-center gap-1.5 rounded-lg text-[11.5px] transition"
            :class="activeSection === section.key
              ? 'bg-[var(--color-accent)] font-medium text-[var(--color-accent-fg)]'
              : 'bg-transparent text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]'"
            @click="activeSection = section.key"
          >
            <component :is="section.icon" :size="13" />
            <span class="whitespace-nowrap">{{ section.label }}</span>
          </button>
        </div>

        <div class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-elevated)] shadow-[var(--shadow-card)]">
          <!-- 身份 -->
          <section v-show="activeSection === 'identity'" class="flex min-h-0 flex-1 flex-col overflow-auto p-4">
            <div class="mb-3 flex h-7 items-center justify-between">
              <div class="flex items-center gap-2">
                <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-[var(--color-icon-accent-soft)] text-[var(--color-icon-accent)]">
                  <Laptop :size="14" />
                </div>
                <span class="text-[12.5px] font-semibold">身份</span>
              </div>
              <button
                class="inline-flex h-7 items-center gap-1.5 rounded-lg border border-[var(--color-border)] bg-transparent px-2.5 text-[11.5px] text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)] disabled:opacity-60"
                title="刷新 IP"
                :disabled="refreshingIp"
                @click="onRefreshIp"
              >
                <RefreshCw :size="12" :class="refreshingIp ? 'animate-spin' : ''" />
                刷新 IP
              </button>
            </div>
            <dl class="flex min-h-0 flex-1 flex-col divide-y divide-[var(--color-border-soft)] rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-panel)]">
              <div v-for="row in [
                { label: '主机名', value: hostname, copyLabel: '主机名' },
                { label: '局域网 IP', value: localIp, copyLabel: 'IP' },
                { label: '监听端口', value: port + '（被占用时自动 +1）', copyLabel: '' },
                { label: '根地址', value: rootUrl, copyLabel: '根地址' },
              ]" :key="row.label" class="grid grid-cols-[88px_minmax(0,1fr)_auto] items-center gap-3 px-4 py-2.5">
                <dt class="text-[11.5px] text-[var(--color-text-muted)]">{{ row.label }}</dt>
                <dd class="min-w-0">
                  <span class="block truncate font-mono text-[12px] text-[var(--color-text)]" :title="String(row.value)">{{ row.value }}</span>
                </dd>
                <button
                  v-if="row.copyLabel"
                  class="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-[var(--color-text-subtle)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
                  @click="copy(String(row.value), row.copyLabel)"
                  title="复制"
                >
                  <Copy :size="12" />
                </button>
              </div>
            </dl>
          </section>

          <!-- 存储 -->
          <section v-show="activeSection === 'storage'" class="flex min-h-0 flex-1 flex-col overflow-auto p-4">
            <div class="mb-3 flex h-7 items-center gap-2">
              <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-[var(--color-icon-accent-soft)] text-[var(--color-icon-accent)]">
                <HardDrive :size="14" />
              </div>
              <span class="text-[12.5px] font-semibold">下载保存目录</span>
            </div>
            <div class="flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-panel)] px-3 py-2">
              <FolderCog :size="14" class="shrink-0 text-[var(--color-text-subtle)]" />
              <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--color-text-muted)]" :title="rootPath">{{ rootPath }}</span>
            </div>
            <form class="mt-2.5 flex gap-2" @submit.prevent="applySaveDir">
              <input
                v-model="saveDirInput"
                type="text"
                placeholder="D:\downloads"
                class="rv-input h-9 min-w-0 flex-1 font-mono text-[11.5px]"
                :disabled="submittingDir"
              />
              <button type="button" class="inline-flex h-9 shrink-0 items-center rounded-lg border border-[var(--color-border)] bg-transparent px-3 text-[11.5px] text-[var(--color-text)] transition hover:bg-[var(--color-bg-hover)]" @click="pickFolder">浏览</button>
              <button type="submit" class="inline-flex h-9 shrink-0 items-center rounded-lg bg-transparent px-3.5 text-[11.5px] font-medium text-[var(--color-accent)] transition hover:bg-[var(--color-accent-soft)] hover:text-[var(--color-accent-hover)] disabled:opacity-50" :disabled="submittingDir || !saveDirInput.trim()">应用</button>
            </form>
          </section>

          <!-- 端口 -->
          <section v-show="activeSection === 'ports'" class="flex min-h-0 flex-1 flex-col overflow-auto p-4">
            <div class="mb-3 flex h-7 items-center justify-between gap-2">
              <div class="flex items-center gap-2">
                <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-[var(--color-icon-accent-soft)] text-[var(--color-icon-accent)]">
                  <RadioTower :size="14" />
                </div>
                <span class="text-[12.5px] font-semibold">端口</span>
              </div>
              <button
                class="inline-flex h-7 items-center rounded-lg border border-[var(--color-border)] bg-transparent px-2.5 text-[11.5px] text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)] disabled:opacity-50"
                type="button"
                :disabled="submittingSharePort || submittingSitePort"
                @click="resetPorts"
              >
                重置
              </button>
            </div>
            <div class="grid grid-cols-1 content-start gap-3">
              <div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-panel)] p-3">
                <div class="mb-2 flex items-center">
                  <span class="flex items-center gap-1.5 text-[11.5px] font-medium text-[var(--color-text-muted)]">
                    <RadioTower :size="13" />
                    分享端口
                  </span>
                </div>
                <form class="flex gap-2" @submit.prevent="applySharePort">
                  <input
                    v-model="sharePortInput"
                    type="number"
                    min="1"
                    max="65535"
                    class="rv-input h-9 min-w-0 flex-1 font-mono text-[11.5px]"
                    :disabled="submittingSharePort"
                  />
                  <button
                    type="submit"
                    class="inline-flex h-9 shrink-0 items-center rounded-lg bg-transparent px-3 text-[11.5px] font-medium text-[var(--color-accent)] transition hover:bg-[var(--color-accent-soft)] hover:text-[var(--color-accent-hover)] disabled:opacity-50"
                    :disabled="submittingSharePort"
                  >
                    应用
                  </button>
                </form>
              </div>
              <div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-panel)] p-3">
                <div class="mb-2 flex items-center">
                  <span class="flex items-center gap-1.5 text-[11.5px] font-medium text-[var(--color-text-muted)]">
                    <Globe :size="13" />
                    站点起始端口
                  </span>
                </div>
                <form class="flex gap-2" @submit.prevent="applySitePort">
                  <input
                    v-model="sitePortInput"
                    type="number"
                    min="1"
                    max="65535"
                    class="rv-input h-9 min-w-0 flex-1 font-mono text-[11.5px]"
                    :disabled="submittingSitePort"
                  />
                  <button
                    type="submit"
                    class="inline-flex h-9 shrink-0 items-center rounded-lg bg-transparent px-3 text-[11.5px] font-medium text-[var(--color-accent)] transition hover:bg-[var(--color-accent-soft)] hover:text-[var(--color-accent-hover)] disabled:opacity-50"
                    :disabled="submittingSitePort"
                  >
                    应用
                  </button>
                </form>
              </div>
            </div>
          </section>

          <!-- 穿透 -->
          <section v-show="activeSection === 'tunnel'" class="flex min-h-0 flex-1 flex-col overflow-auto p-4">
            <div class="mb-3 flex h-7 items-center gap-2">
              <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-[var(--color-icon-accent-soft)] text-[var(--color-icon-accent)]">
                <Waypoints :size="14" />
              </div>
              <span class="text-[12.5px] font-semibold">内网穿透</span>
            </div>
            <div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-panel)] p-3">
              <div class="mb-2 flex h-6 items-center justify-between gap-2">
                <span class="text-[11.5px] font-medium leading-none text-[var(--color-text-muted)]">ngrok Authtoken</span>
                <div class="flex h-6 items-center gap-2">
                  <button
                    class="inline-flex h-6 items-center gap-1 rounded-md px-2 text-[10.5px] font-medium leading-none text-[var(--color-accent)] transition hover:bg-[var(--color-accent-soft)] hover:text-[var(--color-accent-hover)]"
                    type="button"
                    @click="openNgrokSite"
                  >
                    <ExternalLink :size="11" />
                    获取 Authtoken
                  </button>
                  <span
                    class="text-[11px] font-medium leading-none"
                    :class="state.initial?.ngrok_authtoken ? 'text-[var(--color-accent)]' : 'text-[var(--color-text-subtle)]'"
                  >
                    {{ state.initial?.ngrok_authtoken ? '已配置' : '未配置' }}
                  </span>
                </div>
              </div>
              <form class="flex gap-2" @submit.prevent="saveNgrokToken">
                <input
                  v-model="authtokenInput"
                  type="password"
                  autocomplete="off"
                  placeholder="粘贴 ngrok Authtoken"
                  class="rv-input h-9 min-w-0 flex-1 font-mono text-[11.5px]"
                  :disabled="applyingToken"
                />
                <button
                  type="submit"
                  class="inline-flex h-9 shrink-0 items-center rounded-lg bg-transparent px-3 text-[11.5px] font-medium text-[var(--color-accent)] transition hover:bg-[var(--color-accent-soft)] hover:text-[var(--color-accent-hover)] disabled:opacity-50"
                  :disabled="applyingToken"
                >
                  保存
                </button>
              </form>
            </div>
          </section>

          <!-- 界面 -->
          <section v-show="activeSection === 'appearance'" class="flex min-h-0 flex-1 flex-col overflow-auto p-4">
            <div class="mb-3 flex h-7 items-center gap-2">
              <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-[var(--color-icon-accent-soft)] text-[var(--color-icon-accent)]">
                <Type :size="14" />
              </div>
              <span class="text-[12.5px] font-semibold">界面</span>
            </div>
            <div class="mb-4">
              <div class="mb-2 text-[11.5px] font-medium text-[var(--color-text-muted)]">主题</div>
              <div class="grid grid-cols-3 gap-2">
                <button
                  v-for="option in themeOptions"
                  :key="option.key"
                  class="h-9 rounded-lg border text-[11.5px] transition"
                  :class="state.theme === option.key
                    ? 'border-[var(--color-accent)] bg-[var(--color-accent)] font-medium text-[var(--color-accent-fg)]'
                    : 'border-[var(--color-border)] bg-transparent text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]'"
                  @click="setThemeMode(option.key)"
                >
                  {{ option.label }}
                </button>
              </div>
            </div>
            <div class="mb-2 text-[11.5px] font-medium text-[var(--color-text-muted)]">字体大小</div>
            <div class="flex items-center gap-4 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-panel)] px-4 py-3">
              <button class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-[var(--color-border)] bg-transparent text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]" @click="bumpFont(-1)" title="缩小">
                <Minus :size="14" />
              </button>
              <div class="min-w-0 flex-1 text-center">
                <div class="font-mono font-semibold" :style="{ fontSize: (state.fontSize + 6) + 'px' }">Aa</div>
                <div class="mt-1 text-[11px] tabular-nums text-[var(--color-text-muted)]">{{ state.fontSize }} px</div>
              </div>
              <button class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-[var(--color-border)] bg-transparent text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]" @click="bumpFont(1)" title="放大">
                <Plus :size="14" />
              </button>
            </div>
          </section>

          <!-- 关于 -->
          <section v-show="activeSection === 'about'" class="flex min-h-0 flex-1 flex-col overflow-auto p-4">
            <div class="mb-3 flex h-7 items-center gap-2">
              <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-[var(--color-icon-accent-soft)] text-[var(--color-icon-accent)]">
                <Info :size="14" />
              </div>
              <span class="text-[12.5px] font-semibold">关于</span>
            </div>
            <div class="flex min-h-0 flex-1 flex-col gap-1.5">
              <div class="flex items-center justify-between gap-3 rounded-lg bg-[var(--color-bg-panel)] px-3 py-2 text-[12px]">
                <span class="text-[var(--color-text-muted)]">版本</span>
                <span class="truncate font-mono text-[var(--color-text)]">v1.1.0</span>
              </div>
              <div class="flex items-center justify-between gap-3 rounded-lg bg-[var(--color-bg-panel)] px-3 py-2 text-[12px]">
                <span class="shrink-0 text-[var(--color-text-muted)]">本地数据</span>
                <span class="truncate font-mono text-[var(--color-text)]">shares.json · sites.json · history.json</span>
              </div>
            </div>
          </section>
        </div>
      </div>
    </div>
  </section>
</template>
