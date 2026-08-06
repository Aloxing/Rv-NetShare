<script setup lang="ts">
import { Clock3, FolderOpen } from '@lucide/vue';
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';

const props = defineProps<{ shares: number; sites: number; history: number; }>();

const clock = ref(new Date());
let timer: number | null = null;

onMounted(() => {
  timer = window.setInterval(() => { clock.value = new Date(); }, 1000);
});
onBeforeUnmount(() => {
  if (timer !== null) window.clearInterval(timer);
});

const time = computed(() => {
  const d = clock.value;
  const pad = (n: number) => n.toString().padStart(2, '0');
  return pad(d.getHours()) + ':' + pad(d.getMinutes()) + ':' + pad(d.getSeconds());
});
</script>

<template>
  <footer class="flex h-8 shrink-0 select-none items-center justify-between border-t border-[var(--color-border-soft)] bg-[var(--color-bg-elevated)] px-5 text-[11.5px] text-[var(--color-text-muted)]">
    <div class="flex items-center gap-4">
      <span class="flex items-center gap-1.5"><FolderOpen :size="13" />{{ props.shares }} 个分享</span>
      <span class="flex items-center gap-1.5"><span class="h-1.5 w-1.5 rounded-full bg-[var(--color-icon-accent)]"></span>{{ props.sites }} 个站点</span>
      <span class="flex items-center gap-1.5"><Clock3 :size="13" />{{ props.history }} 条记录</span>
    </div>
    <div class="flex items-center gap-4">
      <span class="flex items-center gap-1.5"><span class="h-1.5 w-1.5 rounded-full bg-[var(--color-success)]"></span>LAN</span>
      <span class="font-mono tabular-nums">{{ time }}</span>
    </div>
  </footer>
</template>
