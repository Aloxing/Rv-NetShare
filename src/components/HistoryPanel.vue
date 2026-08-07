<script setup lang="ts">
import { computed, ref } from 'vue';
import {
  CalendarDays,
  Check,
  Clock3,
  Download,
  File,
  Folder,
  Globe,
  Network,
  Search,
  Trash2,
  Users,
  X,
} from '@lucide/vue';
import { clearHistory, removeHistory, useAppState } from '../composables/useAppState';
import { formatBytes, formatTimestamp } from '../utils/format';
import type { AccessRecord } from '../types';
import FilterSelect from './FilterSelect.vue';

const state = useAppState();

type RecordKind = 'file' | 'folder' | 'site' | 'unknown';
type GroupMode = 'peer' | 'resource' | 'date';

const keyword = ref('');
const kindFilter = ref<RecordKind | 'all'>('all');
const statusFilter = ref<'all' | 'success' | 'failed'>('all');
const timeFilter = ref<'all' | 'today' | '7d' | '30d'>('all');
const groupMode = ref<GroupMode>('peer');

const kindOptions = [
  { value: 'all' as const, label: '全部类型' },
  { value: 'file' as const, label: '文件' },
  { value: 'folder' as const, label: '文件夹' },
  { value: 'site' as const, label: '站点' },
  { value: 'unknown' as const, label: '其他' },
];
const statusOptions = [
  { value: 'all' as const, label: '全部状态' },
  { value: 'success' as const, label: '成功' },
  { value: 'failed' as const, label: '失败' },
];
const timeOptions = [
  { value: 'all' as const, label: '全部时间' },
  { value: 'today' as const, label: '今天' },
  { value: '7d' as const, label: '近 7 天' },
  { value: '30d' as const, label: '近 30 天' },
];
const groupOptions = [
  { value: 'peer' as const, label: '按对端' },
  { value: 'resource' as const, label: '按资源' },
  { value: 'date' as const, label: '按日期' },
];

function recordKind(item: AccessRecord): RecordKind {
  if (state.sites.some((site) => site.id === item.share_id)) return 'site';
  const share = state.shares.find((share) => share.id === item.share_id);
  return share?.kind ?? 'unknown';
}

function resourceLabel(item: AccessRecord) {
  const site = state.sites.find((site) => site.id === item.share_id);
  if (site) return site.name;
  const share = state.shares.find((share) => share.id === item.share_id);
  if (share) return share.name;
  return item.share_name;
}

function kindLabel(kind: RecordKind) {
  return { file: '文件', folder: '文件夹', site: '站点', unknown: '其他' }[kind];
}

function kindClass(kind: RecordKind) {
  if (kind === 'file') return 'bg-[var(--color-success-soft)] text-[var(--color-success)]';
  if (kind === 'folder') return 'bg-[var(--color-icon-warning-soft)] text-[var(--color-icon-warning)]';
  if (kind === 'site') return 'bg-[var(--color-icon-accent-soft)] text-[var(--color-icon-accent)]';
  return 'bg-[var(--color-bg-hover)] text-[var(--color-text-muted)]';
}

function dateKey(ts: number) {
  const d = new Date(ts * 1000);
  const mm = String(d.getMonth() + 1).padStart(2, '0');
  const dd = String(d.getDate()).padStart(2, '0');
  return `${d.getFullYear()}-${mm}-${dd}`;
}

function dateLabel(ts: number) {
  const now = Date.now() / 1000;
  const key = dateKey(ts);
  if (key === dateKey(now)) return '今天';
  if (key === dateKey(now - 86400)) return '昨天';
  return key;
}

function isToday(ts: number) {
  return dateKey(ts) === dateKey(Date.now() / 1000);
}

const filtered = computed(() => {
  const q = keyword.value.trim().toLowerCase();
  const now = Date.now() / 1000;
  return state.history.filter((item) => {
    if (kindFilter.value !== 'all' && recordKind(item) !== kindFilter.value) return false;
    if (statusFilter.value !== 'all' && item.status !== statusFilter.value) return false;
    if (timeFilter.value === 'today' && !isToday(item.timestamp)) return false;
    if (timeFilter.value === '7d' && item.timestamp < now - 7 * 86400) return false;
    if (timeFilter.value === '30d' && item.timestamp < now - 30 * 86400) return false;
    if (!q) return true;
    const haystack = [item.share_name, item.path, item.peer, item.user_agent ?? ''];
    return haystack.some((value) => value.toLowerCase().includes(q));
  });
});

const filteredBytes = computed(() =>
  filtered.value.reduce((sum, item) => sum + item.bytes, 0),
);

const uniquePeers = computed(
  () => new Set(filtered.value.map((item) => item.peer || '未知')).size,
);

const successCount = computed(
  () => filtered.value.filter((item) => item.status === 'success').length,
);

const hasFilters = computed(
  () =>
    keyword.value.trim() !== '' ||
    kindFilter.value !== 'all' ||
    statusFilter.value !== 'all' ||
    timeFilter.value !== 'all',
);

function resetFilters() {
  keyword.value = '';
  kindFilter.value = 'all';
  statusFilter.value = 'all';
  timeFilter.value = 'all';
}

interface HistoryGroup {
  key: string;
  label: string;
  kind?: RecordKind;
  items: AccessRecord[];
}

const groups = computed<HistoryGroup[]>(() => {
  const byKey = new Map<string, HistoryGroup>();
  for (const item of filtered.value) {
    let key: string;
    let label: string;
    let kind: RecordKind | undefined;
    if (groupMode.value === 'resource') {
      key = 'resource:' + item.share_id;
      label = resourceLabel(item);
      kind = recordKind(item);
    } else if (groupMode.value === 'date') {
      key = 'date:' + dateKey(item.timestamp);
      label = dateLabel(item.timestamp);
    } else {
      key = 'peer:' + (item.peer || '未知');
      label = item.peer || '未知';
    }
    const group = byKey.get(key);
    if (group) {
      group.items.push(item);
    } else {
      byKey.set(key, { key, label, kind, items: [item] });
    }
  }
  const list = [...byKey.values()];
  list.sort((a, b) => {
    if (groupMode.value === 'date') return b.key.localeCompare(a.key);
    return b.items[0].timestamp - a.items[0].timestamp;
  });
  return list;
});

function groupBytes(items: AccessRecord[]) {
  return items.reduce((sum, item) => sum + item.bytes, 0);
}

async function onClear() {
  if (!confirm('确定清空所有下载记录？此操作不可撤销。')) return;
  await clearHistory();
}

async function onRemove(item: AccessRecord) {
  await removeHistory(item.id);
}
</script>

<template>
  <section class="flex h-full flex-col overflow-hidden">
    <div class="flex h-14 shrink-0 items-center justify-between border-b border-[var(--color-border-soft)] bg-[var(--color-bg-elevated)] px-5">
      <div class="flex items-center gap-2.5">
        <Clock3 :size="15" class="text-[var(--color-icon-accent)]" />
        <span class="text-[13px] font-semibold">记录</span>
        <span class="rounded-full bg-[var(--color-bg-hover)] px-2 py-0.5 text-[10.5px] font-semibold tabular-nums text-[var(--color-text-muted)]">{{ state.history.length }}</span>
      </div>
      <button
        class="inline-flex h-8 items-center gap-1.5 rounded-lg border border-[var(--color-border)] bg-transparent px-3 text-[12px] text-[var(--color-text-muted)] transition hover:bg-[var(--color-danger-soft)] hover:text-[var(--color-danger)] disabled:opacity-50"
        :disabled="!state.history.length"
        @click="onClear"
      >
        <Trash2 :size="13" />
        清空
      </button>
    </div>

    <div class="flex shrink-0 flex-wrap items-center gap-2 border-b border-[var(--color-border-soft)] px-5 py-2.5">
      <div class="rv-input-wrap h-8 min-w-[180px] max-w-[420px] flex-1">
        <Search :size="13" class="shrink-0 text-[var(--color-text-subtle)]" />
        <input v-model="keyword" type="text" placeholder="搜索资源、对端或路径" class="rv-input text-[12px]" />
        <button
          v-if="keyword"
          class="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--color-text-subtle)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
          title="清空搜索"
          @click="keyword = ''"
        >
          <X :size="11" />
        </button>
      </div>
      <FilterSelect v-model="kindFilter" :options="kindOptions" />
      <FilterSelect v-model="statusFilter" :options="statusOptions" />
      <FilterSelect v-model="timeFilter" :options="timeOptions" />
      <FilterSelect v-model="groupMode" :options="groupOptions" />
      <button
        v-if="hasFilters"
        class="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-lg border border-[var(--color-border)] bg-transparent px-2.5 text-[11.5px] text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
        @click="resetFilters"
      >
        <X :size="12" />
        清除筛选
      </button>
    </div>

    <div class="flex shrink-0 flex-wrap items-center gap-x-5 gap-y-1 border-b border-[var(--color-border-soft)] px-5 py-2 text-[11px] text-[var(--color-text-muted)]">
      <span class="flex items-center gap-1.5">
        <Clock3 :size="12" class="text-[var(--color-icon-accent)]" />
        记录
        <b class="font-mono tabular-nums text-[var(--color-text)]">{{ filtered.length }}</b>
        <template v-if="filtered.length !== state.history.length">
          <span class="text-[var(--color-text-subtle)]">/ {{ state.history.length }}</span>
        </template>
      </span>
      <span class="flex items-center gap-1.5">
        <Download :size="12" class="text-[var(--color-icon-accent)]" />
        传输
        <b class="font-mono tabular-nums text-[var(--color-text)]">{{ formatBytes(filteredBytes) }}</b>
      </span>
      <span class="flex items-center gap-1.5">
        <Users :size="12" class="text-[var(--color-icon-accent)]" />
        对端
        <b class="font-mono tabular-nums text-[var(--color-text)]">{{ uniquePeers }}</b>
      </span>
      <span class="flex items-center gap-1.5">
        <Check :size="12" class="text-[var(--color-success)]" />
        成功
        <b class="font-mono tabular-nums text-[var(--color-text)]">{{ successCount }}</b>
      </span>
    </div>

    <div class="flex-1 overflow-auto p-5">
      <div class="overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-elevated)] shadow-[var(--shadow-card)]">
        <table class="w-full table-fixed text-[12.5px]">
          <colgroup>
            <col class="w-[34%]" />
            <col class="w-[17%]" />
            <col class="w-[10%]" />
            <col class="w-[10%]" />
            <col class="w-[14%]" />
            <col class="w-[15%]" />
          </colgroup>
          <thead>
            <tr class="bg-[var(--color-bg-panel)] text-[10.5px] uppercase tracking-[0.1em] text-[var(--color-text-subtle)]">
              <th class="px-4 py-2.5 text-left font-medium">资源</th>
              <th class="px-4 py-2.5 text-left font-medium">对端</th>
              <th class="px-4 py-2.5 text-right font-medium">大小</th>
              <th class="px-4 py-2.5 text-left font-medium">状态</th>
              <th class="px-4 py-2.5 text-left font-medium">时间</th>
              <th class="px-4 py-2.5 text-right font-medium">操作</th>
            </tr>
          </thead>
          <tbody>
            <template v-for="group in groups" :key="group.key">
              <tr class="border-t border-[var(--color-border-soft)] bg-[var(--color-bg-panel)]">
                <td colspan="6" class="px-4 py-2">
                  <div class="flex items-center gap-2">
                    <Network v-if="groupMode === 'peer'" :size="13" class="shrink-0 text-[var(--color-icon-accent)]" />
                    <File v-else-if="group.kind === 'file'" :size="13" class="shrink-0 text-[var(--color-success)]" />
                    <Folder v-else-if="group.kind === 'folder'" :size="13" class="shrink-0 text-[var(--color-icon-warning)]" />
                    <Globe v-else-if="group.kind === 'site'" :size="13" class="shrink-0 text-[var(--color-icon-accent)]" />
                    <CalendarDays v-else-if="groupMode === 'date'" :size="13" class="shrink-0 text-[var(--color-icon-accent)]" />
                    <Network v-else :size="13" class="shrink-0 text-[var(--color-text-subtle)]" />
                    <span class="truncate font-mono text-[11.5px] font-semibold" :title="group.label">{{ group.label }}</span>
                    <span v-if="group.kind" class="shrink-0 rounded-md px-1.5 py-0.5 text-[9.5px] font-semibold" :class="kindClass(group.kind)">{{ kindLabel(group.kind) }}</span>
                    <span class="shrink-0 rounded-full bg-[var(--color-bg-hover)] px-2 py-0.5 text-[10px] font-semibold tabular-nums text-[var(--color-text-muted)]">{{ group.items.length }}</span>
                    <span class="shrink-0 text-[10.5px] text-[var(--color-text-subtle)]">共 {{ formatBytes(groupBytes(group.items)) }}</span>
                  </div>
                </td>
              </tr>
              <tr
                v-for="item in group.items"
                :key="item.id"
                class="border-t border-[var(--color-border-soft)] transition-colors hover:bg-[var(--color-bg-hover)]"
              >
                <td class="px-4 py-3 align-middle">
                  <div class="flex min-w-0 items-center gap-2">
                    <span class="shrink-0 rounded-md px-1.5 py-0.5 text-[9.5px] font-semibold" :class="kindClass(recordKind(item))">{{ kindLabel(recordKind(item)) }}</span>
                    <div class="min-w-0">
                      <div class="truncate font-medium" :title="item.share_name">{{ item.share_name }}</div>
                      <div v-if="item.user_agent" class="truncate font-mono text-[10.5px] text-[var(--color-text-subtle)]" :title="item.user_agent">{{ item.user_agent }}</div>
                    </div>
                  </div>
                </td>
                <td class="px-4 py-3 align-middle font-mono text-[11px] text-[var(--color-text-muted)]" :title="item.peer">{{ item.peer }}</td>
                <td class="px-4 py-3 text-right align-middle font-mono text-[11.5px] tabular-nums">{{ formatBytes(item.bytes) }}</td>
                <td class="px-4 py-3 align-middle">
                  <span
                    class="inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[10.5px] font-medium"
                    :class="item.status === 'success' ? 'bg-[var(--color-success-soft)] text-[var(--color-success)]' : 'bg-[var(--color-danger-soft)] text-[var(--color-danger)]'"
                  >
                    <span class="h-1 w-1 rounded-full" :class="item.status === 'success' ? 'bg-[var(--color-success)]' : 'bg-[var(--color-danger)]'"></span>
                    {{ item.status === 'success' ? '成功' : '失败' }}
                  </span>
                </td>
                <td class="px-4 py-3 align-middle font-mono text-[11px] text-[var(--color-text-muted)]">{{ formatTimestamp(item.timestamp) }}</td>
                <td class="px-4 py-3 text-right align-middle">
                  <button
                    class="inline-flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] transition hover:bg-[var(--color-danger-soft)] hover:text-[var(--color-danger)]"
                    title="删除记录"
                    @click="onRemove(item)"
                  >
                    <Trash2 :size="13" />
                  </button>
                </td>
              </tr>
            </template>
            <tr v-if="!state.history.length">
              <td colspan="6" class="px-4 py-20 text-center text-[12.5px] text-[var(--color-text-subtle)]">暂无下载记录</td>
            </tr>
            <tr v-else-if="!groups.length">
              <td colspan="6" class="px-4 py-16 text-center">
                <div class="text-[12.5px] text-[var(--color-text-subtle)]">没有符合条件的记录</div>
                <button
                  v-if="hasFilters"
                  class="mt-2 inline-flex h-7 items-center gap-1.5 rounded-lg border border-[var(--color-border)] bg-transparent px-2.5 text-[11.5px] text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
                  @click="resetFilters"
                >
                  <X :size="12" />
                  清除筛选
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </section>
</template>
