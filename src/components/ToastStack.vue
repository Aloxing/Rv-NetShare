<script setup lang="ts">
import { AlertCircle, CheckCircle2, Info } from '@lucide/vue';
import { useAppState } from '../composables/useAppState';

const state = useAppState();

function classFor(kind: string) {
  if (kind === 'success') return 'border-[var(--color-success)]/30 bg-[var(--color-bg-elevated)]';
  if (kind === 'error') return 'border-[var(--color-danger)]/40 bg-[var(--color-bg-elevated)]';
  return 'border-[var(--color-border)] bg-[var(--color-bg-elevated)]';
}
</script>

<template>
  <div class="pointer-events-none fixed bottom-10 right-5 z-50 flex w-80 flex-col gap-2">
    <transition-group name="toast">
      <div
        v-for="t in state.toasts"
        :key="t.id"
        class="pointer-events-auto flex items-center gap-2.5 rounded-lg border px-3.5 py-2.5 text-[12.5px] shadow-[var(--shadow-popup)]"
        :class="classFor(t.kind)"
      >
        <CheckCircle2 v-if="t.kind === 'success'" :size="15" class="shrink-0 text-[var(--color-success)]" />
        <AlertCircle v-else-if="t.kind === 'error'" :size="15" class="shrink-0 text-[var(--color-danger)]" />
        <Info v-else :size="15" class="shrink-0 text-[var(--color-icon-accent)]" />
        <span class="min-w-0 flex-1 break-all line-clamp-3">{{ t.text }}</span>
      </div>
    </transition-group>
  </div>
</template>

<style scoped>
.toast-enter-active, .toast-leave-active { transition: opacity 0.18s ease, transform 0.18s ease; }
.toast-enter-from { opacity: 0; transform: translateX(8px); }
.toast-leave-to { opacity: 0; transform: translateX(8px); }
</style>
