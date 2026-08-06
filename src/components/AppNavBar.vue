<script setup lang="ts">
import { RadioTower } from '@lucide/vue';
import type { Tab } from '../types';
import { useAppState } from '../composables/useAppState';

const props = defineProps<{ active: Tab }>();
const emit = defineEmits<{ (e: 'change', tab: Tab): void }>();
const state = useAppState();

interface Item { key: Tab; label: string; count?: () => number; }

const items: Item[] = [
  { key: 'shares', label: '分享', count: () => state.shares.length },
  { key: 'sites', label: '站点', count: () => state.sites.length },
  { key: 'history', label: '记录', count: () => state.history.length },
  { key: 'settings', label: '设置' },
];
</script>

<template>
  <nav class="flex h-12 shrink-0 items-center justify-between gap-2 border-b border-[var(--color-border-soft)] bg-[var(--color-bg-elevated)] px-5">
    <div class="flex items-center gap-1">
      <button
        v-for="item in items"
        :key="item.key"
        class="flex h-8 items-center gap-2 rounded-lg px-3.5 text-[13px] transition-colors"
        :class="props.active === item.key
          ? 'bg-transparent font-medium text-[var(--color-accent)]'
          : 'bg-transparent text-[var(--color-text-muted)] hover:bg-transparent hover:text-[var(--color-text)]'"
        @click="emit('change', item.key)"
      >
        <span class="whitespace-nowrap">{{ item.label }}</span>
        <span
          v-if="item.count && item.count() > 0"
          class="min-w-[18px] rounded-full px-1.5 py-0.5 text-center text-[10px] font-semibold tabular-nums"
          :class="props.active === item.key
            ? 'bg-[var(--color-accent)]/10 text-[var(--color-accent)]'
            : 'bg-[var(--color-bg-hover)] text-[var(--color-text-subtle)]'"
        >
          {{ item.count() }}
        </span>
      </button>
    </div>
    <div class="flex items-center gap-2 rounded-full border border-[var(--color-border)] py-1.5 pl-3 pr-4 text-[12px]">
      <span class="relative flex h-2 w-2">
        <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-[var(--color-success)] opacity-50"></span>
        <span class="relative inline-flex h-2 w-2 rounded-full bg-[var(--color-success)]"></span>
      </span>
      <RadioTower :size="14" class="text-[var(--color-text-muted)]" />
      <span class="font-medium text-[var(--color-text-muted)]">Receiver</span>
      <span class="text-[var(--color-border-strong)]">·</span>
      <span class="font-mono text-[11.5px] text-[var(--color-text-muted)]">{{ state.initial?.local_ip ?? '-' }}:{{ state.initial?.port ?? '-' }}</span>
    </div>
  </nav>
</template>
