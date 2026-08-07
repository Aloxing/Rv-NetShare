<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { Check, ChevronDown } from '@lucide/vue';

const props = defineProps<{
  modelValue: string;
  options: { value: string; label: string; count?: number }[];
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
}>();

const open = ref(false);
const root = ref<HTMLElement | null>(null);

const current = computed(() =>
  props.options.find((option) => option.value === props.modelValue),
);

function toggle() {
  open.value = !open.value;
}

function select(value: string) {
  emit('update:modelValue', value);
  open.value = false;
}

function onDocumentClick(event: MouseEvent) {
  if (root.value && !root.value.contains(event.target as Node)) {
    open.value = false;
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') open.value = false;
}

onMounted(() => {
  document.addEventListener('click', onDocumentClick, true);
  document.addEventListener('keydown', onKeydown);
});

onBeforeUnmount(() => {
  document.removeEventListener('click', onDocumentClick, true);
  document.removeEventListener('keydown', onKeydown);
});
</script>

<template>
  <div ref="root" class="relative shrink-0">
    <button
      type="button"
      class="rv-filter-trigger"
      :class="open ? 'rv-filter-trigger-open' : ''"
      :aria-expanded="open"
      @click.stop="toggle"
    >
      <span class="min-w-0 flex-1 truncate text-left">{{ current?.label ?? '请选择' }}</span>
      <ChevronDown
        :size="13"
        class="shrink-0 text-[var(--color-text-subtle)] transition-transform duration-150"
        :class="open ? 'rotate-180' : ''"
      />
    </button>

    <Transition name="drop">
      <div v-if="open" class="rv-filter-popup" role="listbox">
        <button
          v-for="option in options"
          :key="option.value"
          type="button"
          class="rv-filter-option"
          :class="option.value === modelValue ? 'rv-filter-option-active' : ''"
          role="option"
          :aria-selected="option.value === modelValue"
          @click="select(option.value)"
        >
          <span class="min-w-0 flex-1 truncate text-left">{{ option.label }}</span>
          <span v-if="option.count !== undefined" class="rv-filter-count">{{ option.count }}</span>
          <Check
            v-if="option.value === modelValue"
            :size="13"
            class="shrink-0 text-[var(--color-accent)]"
          />
        </button>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.rv-filter-trigger {
  display: flex;
  height: 2rem;
  min-width: 7.5rem;
  max-width: 11rem;
  align-items: center;
  gap: 0.5rem;
  border: 1px solid var(--color-border);
  border-radius: 0.5rem;
  background-color: var(--color-bg-panel);
  color: var(--color-text);
  padding: 0 0.65rem;
  font-size: 11.5px;
  cursor: pointer;
  transition: border-color 0.15s ease, box-shadow 0.15s ease, background-color 0.15s ease;
}

.rv-filter-trigger:hover {
  border-color: var(--color-border-strong);
  background-color: var(--color-bg-hover);
}

.rv-filter-trigger-open {
  border-color: var(--color-ring-soft);
  box-shadow: 0 0 0 3px rgb(128 128 140 / 0.14);
}

.rv-filter-popup {
  position: absolute;
  z-index: 60;
  top: calc(100% + 0.35rem);
  left: 0;
  min-width: 100%;
  max-height: 16rem;
  overflow-y: auto;
  border: 1px solid var(--color-border);
  border-radius: 0.6rem;
  background-color: var(--color-bg-elevated);
  padding: 0.25rem;
  box-shadow: var(--shadow-popup);
}

.rv-filter-option {
  display: flex;
  width: 100%;
  height: 2rem;
  align-items: center;
  gap: 0.5rem;
  border-radius: 0.4rem;
  padding: 0 0.5rem;
  font-size: 11.5px;
  color: var(--color-text-muted);
  transition: background-color 0.12s ease, color 0.12s ease;
}

.rv-filter-option:hover {
  background-color: var(--color-bg-hover);
  color: var(--color-text);
}

.rv-filter-option-active {
  background-color: var(--color-accent-soft);
  color: var(--color-text);
  font-weight: 500;
}

.rv-filter-count {
  flex-shrink: 0;
  border-radius: 0.35rem;
  background-color: var(--color-bg-hover);
  padding: 0.1rem 0.4rem;
  font-size: 10px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--color-text-subtle);
}

.drop-enter-active,
.drop-leave-active {
  transition: opacity 0.12s ease, transform 0.12s ease;
}

.drop-enter-from,
.drop-leave-to {
  opacity: 0;
  transform: translateY(-3px);
}
</style>
