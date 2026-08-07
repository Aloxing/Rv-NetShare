<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
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

const navRef = ref<HTMLElement | null>(null);
const itemRefs = ref<(HTMLElement | null)[]>([]);
const indicator = reactive({ left: 0, width: 0 });
let resizeObserver: ResizeObserver | null = null;

const indicatorStyle = computed(() => ({
  left: indicator.left + 'px',
  width: indicator.width + 'px',
}));

function setItemRef(el: unknown, index: number) {
  itemRefs.value[index] = el as HTMLElement | null;
}

async function updateIndicator() {
  await nextTick();
  const nav = navRef.value;
  if (!nav) return;
  const index = items.findIndex((item) => item.key === props.active);
  const el = itemRefs.value[index];
  if (!el) return;
  const navRect = nav.getBoundingClientRect();
  const rect = el.getBoundingClientRect();
  indicator.left = rect.left - navRect.left;
  indicator.width = rect.width;
}

onMounted(() => {
  void updateIndicator();
  resizeObserver = new ResizeObserver(() => void updateIndicator());
  if (navRef.value) resizeObserver.observe(navRef.value);
});

watch(() => props.active, () => { void updateIndicator(); });

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  resizeObserver = null;
});
</script>

<template>
  <nav class="flex h-12 shrink-0 items-center border-b border-[var(--color-border-soft)] bg-[var(--color-bg-elevated)] px-5">
    <div ref="navRef" class="relative flex h-10 items-center rounded-full border border-[var(--color-border)] bg-[var(--color-bg-panel)] p-1 shadow-[inset_0_1px_2px_rgb(0_0_0/0.06)]">
      <div
        class="pointer-events-none absolute bottom-1 top-1 rounded-full bg-[var(--color-accent)] shadow-[var(--shadow-card)] transition-all duration-300 ease-out"
        :style="indicatorStyle"
      ></div>
      <button
        v-for="(item, index) in items"
        :key="item.key"
        :ref="(el) => setItemRef(el, index)"
        class="group relative z-10 flex h-8 w-24 items-center justify-center gap-2 rounded-full text-[13px] transition-all duration-150 active:scale-[0.97]"
        :class="props.active === item.key
          ? 'font-medium text-[var(--color-accent-fg)]'
          : 'text-[var(--color-text-muted)] hover:text-[var(--color-accent)]'"
        @click="emit('change', item.key)"
      >
        <span class="whitespace-nowrap">{{ item.label }}</span>
        <span
          v-if="item.count && item.count() > 0"
          class="min-w-[18px] rounded-full px-1.5 py-0.5 text-center text-[10px] font-semibold tabular-nums"
          :class="props.active === item.key
            ? 'bg-[var(--color-accent-fg)]/15 text-[var(--color-accent-fg)]'
            : 'bg-[var(--color-bg-hover)] text-[var(--color-text-subtle)] group-hover:bg-[var(--color-bg-active)]'"
        >
          {{ item.count() }}
        </span>
      </button>
    </div>
  </nav>
</template>
