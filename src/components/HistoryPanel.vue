<script setup lang="ts">
import { computed } from 'vue';
import { Clock3, Network, Trash2 } from '@lucide/vue';
import { clearHistory, removeHistory, useAppState } from '../composables/useAppState';
import { formatBytes, formatTimestamp } from '../utils/format';
import type { AccessRecord } from '../types';

const state = useAppState();

interface HistoryGroup {
  peer: string;
  items: AccessRecord[];
}

const groups = computed<HistoryGroup[]>(() => {
  const byPeer = new Map<string, AccessRecord[]>();
  for (const item of state.history) {
    const key = item.peer || '未知';
    const list = byPeer.get(key) ?? [];
    list.push(item);
    byPeer.set(key, list);
  }
  return [...byPeer.entries()]
    .map(([peer, items]) => ({ peer, items }))
    .sort((a, b) => b.items[0].timestamp - a.items[0].timestamp);
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
            <template v-for="group in groups" :key="group.peer">
              <tr class="border-t border-[var(--color-border-soft)] bg-[var(--color-bg-panel)]">
                <td colspan="6" class="px-4 py-2">
                  <div class="flex items-center gap-2">
                    <Network :size="13" class="text-[var(--color-icon-accent)]" />
                    <span class="font-mono text-[11.5px] font-semibold">{{ group.peer }}</span>
                    <span class="rounded-full bg-[var(--color-bg-hover)] px-2 py-0.5 text-[10px] font-semibold tabular-nums text-[var(--color-text-muted)]">{{ group.items.length }}</span>
                    <span class="text-[10.5px] text-[var(--color-text-subtle)]">共 {{ formatBytes(groupBytes(group.items)) }}</span>
                  </div>
                </td>
              </tr>
              <tr
                v-for="item in group.items"
                :key="item.id"
                class="border-t border-[var(--color-border-soft)] transition-colors hover:bg-[var(--color-bg-hover)]"
              >
                <td class="px-4 py-3 align-middle">
                  <div class="truncate font-medium" :title="item.share_name">{{ item.share_name }}</div>
                  <div v-if="item.user_agent" class="truncate font-mono text-[10.5px] text-[var(--color-text-subtle)]" :title="item.user_agent">{{ item.user_agent }}</div>
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
          </tbody>
        </table>
      </div>
    </div>
  </section>
</template>
