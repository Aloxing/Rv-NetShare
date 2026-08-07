<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import {
  Archive,
  File,
  Image,
  Inbox,
  KeyRound,
  Lock,
  Plus,
  Trash2,
  Upload,
  X,
} from '@lucide/vue';
import {
  buildReceiveUrl,
  clearReceivers,
  createReceiver,
  pushToast,
  refreshReceivers,
  removeReceiver,
  startReceiveTunnel,
  stopReceiveTunnel,
  updateReceiver,
  useAppState,
} from '../composables/useAppState';
import ReceiveCard from './ReceiveCard.vue';
import type { ReceiveEncryption, ReceiveSession } from '../types';

const state = useAppState();

const showCreate = ref(false);
const editingReceiver = ref<ReceiveSession | null>(null);
const nameInput = ref('');
const preset = ref<'photos' | 'files' | 'archives' | 'custom'>('photos');
const customExtInput = ref('');
const encryption = ref<ReceiveEncryption>('none');
const customPassword = ref('');
const submitting = ref(false);
const tunnelBusyId = ref<string | null>(null);
let refreshTimer: number | null = null;

const PHOTOS = ['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'heic', 'heif', 'svg', 'ico'];
const ARCHIVES = ['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'zst'];

const presets = [
  { key: 'photos', label: '照片', icon: Image },
  { key: 'files', label: '文件', icon: File },
  { key: 'archives', label: '压缩包', icon: Archive },
  { key: 'custom', label: '自定义', icon: Upload },
] as const;

const encryptionOptions = [
  { key: 'none', label: '不加密', icon: Lock },
  { key: 'common', label: '通用加密', icon: KeyRound },
  { key: 'custom', label: '单独加密', icon: KeyRound },
] as const;

const draftExtensions = computed(() => {
  if (preset.value === 'photos') return PHOTOS;
  if (preset.value === 'archives') return ARCHIVES;
  if (preset.value === 'files') return ['*'];
  return customExtInput.value
    .split(/[\s,，]+/)
    .map((ext) => ext.trim().replace(/^\./, '').toLowerCase())
    .filter(Boolean);
});

const canCreate = computed(() => {
  if (!nameInput.value.trim() || !draftExtensions.value.length) return false;
  if (encryption.value === 'common' && !state.initial?.receive_common_password) return false;
  if (encryption.value === 'custom' && !customPassword.value.trim() && !editingReceiver.value) return false;
  return true;
});

const urlFor = (receiver: ReceiveSession) =>
  buildReceiveUrl(state.initial?.local_ip ?? '127.0.0.1', state.initial?.port ?? 0, receiver.id);

const ngrokUrlFor = (receiver: ReceiveSession) => state.ngrokUrls['receive:' + receiver.id];

function openCreate() {
  editingReceiver.value = null;
  nameInput.value = '';
  preset.value = 'photos';
  customExtInput.value = '';
  encryption.value = 'none';
  customPassword.value = '';
  showCreate.value = true;
}

function presetFromExtensions(extensions: readonly string[]) {
  if (extensions.includes('*')) return 'files' as const;
  if (extensions.every((ext) => PHOTOS.includes(ext))) return 'photos' as const;
  if (extensions.every((ext) => ARCHIVES.includes(ext))) return 'archives' as const;
  return 'custom' as const;
}

function openEdit(receiver: ReceiveSession) {
  editingReceiver.value = receiver;
  nameInput.value = receiver.name;
  preset.value = presetFromExtensions(receiver.extensions);
  customExtInput.value =
    preset.value === 'custom' ? receiver.extensions.join(', ') : '';
  encryption.value = receiver.encryption;
  customPassword.value = '';
  showCreate.value = true;
}

async function submitCreate() {
  if (!canCreate.value || submitting.value) return;
  submitting.value = true;
  try {
    const customPasswordValue =
      encryption.value === 'custom' ? customPassword.value : undefined;
    if (editingReceiver.value) {
      await updateReceiver(
        editingReceiver.value.id,
        nameInput.value.trim(),
        draftExtensions.value,
        encryption.value,
        customPasswordValue,
      );
      pushToast('success', '接收卡片已更新');
    } else {
      await createReceiver(
        nameInput.value.trim(),
        draftExtensions.value,
        encryption.value,
        customPasswordValue,
      );
      pushToast('success', '接收卡片已创建');
    }
    showCreate.value = false;
    editingReceiver.value = null;
  } catch (err) {
    pushToast('error', String(err));
  } finally {
    submitting.value = false;
  }
}

async function toggleTunnel(receiver: ReceiveSession) {
  if (tunnelBusyId.value === receiver.id) return;
  if (!state.initial?.ngrok_authtoken && !state.ngrokUrls['receive:' + receiver.id]) {
    pushToast('error', '请先在设置中配置 ngrok Authtoken');
    return;
  }
  tunnelBusyId.value = receiver.id;
  try {
    if (state.ngrokUrls['receive:' + receiver.id]) {
      await stopReceiveTunnel(receiver.id);
      pushToast('success', '已关闭公网访问');
    } else {
      await startReceiveTunnel(receiver.id);
      pushToast('success', '公网地址已生成');
    }
  } catch (err) {
    pushToast('error', String(err));
  } finally {
    tunnelBusyId.value = null;
  }
}

async function onRemove(receiver: ReceiveSession) {
  if (!confirm('确定删除接收卡片「' + receiver.name + '」？已接收的文件不会被删除。')) return;
  await removeReceiver(receiver.id);
}

async function onClear() {
  if (!confirm('确定移除全部接收卡片？')) return;
  await clearReceivers();
}

onMounted(() => {
  void refreshReceivers();
  refreshTimer = window.setInterval(() => { void refreshReceivers(); }, 5000);
});

onBeforeUnmount(() => {
  if (refreshTimer !== null) {
    window.clearInterval(refreshTimer);
    refreshTimer = null;
  }
});
</script>

<template>
  <section class="flex h-full flex-col overflow-hidden">
    <div class="flex h-14 shrink-0 items-center justify-between gap-2.5 border-b border-[var(--color-border-soft)] bg-[var(--color-bg-elevated)] px-5">
      <div class="flex items-center gap-2.5">
        <Inbox :size="15" class="text-[var(--color-icon-accent)]" />
        <span class="text-[13px] font-semibold">接收</span>
        <span class="rounded-full bg-[var(--color-bg-hover)] px-2 py-0.5 text-[10.5px] font-semibold tabular-nums text-[var(--color-text-muted)]">{{ state.receivers.length }}</span>
      </div>
      <div class="flex items-center gap-2">
        <button
          class="inline-flex h-9 items-center gap-2 rounded-lg border border-[var(--color-border)] bg-transparent px-3.5 text-[12.5px] font-medium text-[var(--color-accent)] transition hover:bg-[var(--color-accent-soft)] hover:text-[var(--color-accent-hover)]"
          @click="openCreate"
        >
          <Plus :size="14" />
          新建接收
        </button>
        <button
          class="inline-flex h-9 items-center gap-2 rounded-lg px-3 text-[12.5px] text-[var(--color-text-muted)] transition hover:bg-[var(--color-danger-soft)] hover:text-[var(--color-danger)] disabled:opacity-50"
          :disabled="!state.receivers.length"
          @click="onClear"
        >
          <Trash2 :size="14" />
          清空
        </button>
      </div>
    </div>

    <div class="flex-1 overflow-auto">
      <div class="w-full px-3 py-3">
        <div v-if="state.receivers.length" class="grid grid-cols-1 gap-2">
          <ReceiveCard
            v-for="receiver in state.receivers"
            :key="receiver.id"
            :receiver="receiver"
            :url="urlFor(receiver)"
            :ngrok-url="ngrokUrlFor(receiver)"
            :tunnel-busy="tunnelBusyId === receiver.id"
            @remove="onRemove(receiver)"
            @edit="openEdit(receiver)"
            @toggle-tunnel="toggleTunnel(receiver)"
          />
        </div>

        <div v-else class="flex flex-col items-center justify-center py-24 text-center">
          <div class="flex h-16 w-16 items-center justify-center rounded-2xl border border-dashed border-[var(--color-border-strong)] bg-[var(--color-bg-elevated)] text-[var(--color-text-subtle)]">
            <Inbox :size="26" />
          </div>
          <div class="mt-4 text-[14px] font-semibold">暂无接收卡片</div>
          <div class="mt-1 flex items-center gap-2 text-[12px] text-[var(--color-text-subtle)]">
            <Upload :size="13" />
            新建卡片后，手机或电脑打开链接即可上传文件
          </div>
        </div>
      </div>
    </div>
  </section>

  <Teleport to="body">
    <div
      v-if="showCreate"
      class="fixed inset-0 z-[70] flex items-center justify-center bg-black/45 p-4 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label="新建接收卡片"
      @click.self="showCreate = false"
    >
      <div class="anim-rise w-[440px] max-w-full overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-elevated)] shadow-[var(--shadow-popup)]">
        <div class="flex h-12 items-center justify-between border-b border-[var(--color-border-soft)] px-4">
          <div class="flex items-center gap-2 text-[13px] font-semibold">
            <Inbox :size="15" class="text-[var(--color-icon-accent)]" />
            {{ editingReceiver ? '编辑接收卡片' : '新建接收卡片' }}
          </div>
          <button
            class="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
            title="关闭"
            @click="showCreate = false"
          >
            <X :size="15" />
          </button>
        </div>

        <div class="space-y-4 p-4">
          <div>
            <div class="mb-2 text-[11.5px] font-medium text-[var(--color-text-muted)]">名称</div>
            <input
              v-model="nameInput"
              type="text"
              maxlength="40"
              placeholder="例如：手机照片、项目压缩包"
              class="rv-input h-9 w-full text-[12.5px]"
            />
          </div>

          <div>
            <div class="mb-2 text-[11.5px] font-medium text-[var(--color-text-muted)]">接收类型</div>
            <div class="grid grid-cols-4 gap-2">
              <button
                v-for="option in presets"
                :key="option.key"
                type="button"
                class="flex h-10 items-center justify-center gap-1.5 rounded-lg border text-[11.5px] transition"
                :class="preset === option.key
                  ? 'border-[var(--color-accent)] bg-[var(--color-accent)] font-medium text-[var(--color-accent-fg)]'
                  : 'border-[var(--color-border)] bg-transparent text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]'"
                @click="preset = option.key"
              >
                <component :is="option.icon" :size="13" />
                {{ option.label }}
              </button>
            </div>
            <input
              v-if="preset === 'custom'"
              v-model="customExtInput"
              type="text"
              placeholder="例如：pdf, docx, xlsx"
              class="rv-input mt-2 h-9 w-full font-mono text-[11.5px]"
            />
            <div v-if="draftExtensions.length" class="mt-2 truncate font-mono text-[10.5px] text-[var(--color-text-subtle)]">
              {{ preset === 'files' ? '*' : draftExtensions.map((ext) => '.' + ext).join(' ') }}
            </div>
          </div>

          <div>
            <div class="mb-2 text-[11.5px] font-medium text-[var(--color-text-muted)]">加密</div>
            <div class="grid grid-cols-3 gap-2">
              <button
                v-for="option in encryptionOptions"
                :key="option.key"
                type="button"
                class="flex h-10 items-center justify-center gap-1.5 rounded-lg border text-[11.5px] transition"
                :class="encryption === option.key
                  ? 'border-[var(--color-accent)] bg-[var(--color-accent)] font-medium text-[var(--color-accent-fg)]'
                  : 'border-[var(--color-border)] bg-transparent text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]'"
                @click="encryption = option.key"
              >
                <component :is="option.icon" :size="13" />
                {{ option.label }}
              </button>
            </div>
            <div v-if="encryption === 'common' && !state.initial?.receive_common_password" class="mt-2 text-[11px] text-[var(--color-danger)]">
              请先在设置 → 接收 中配置通用加密密码
            </div>
            <input
              v-if="encryption === 'custom'"
              v-model="customPassword"
              type="password"
              autocomplete="new-password"
              :placeholder="editingReceiver ? '留空则保持原密码' : '设置单独上传密码'"
              class="rv-input mt-2 h-9 w-full font-mono text-[11.5px]"
            />
          </div>

          <div class="flex justify-end gap-2 border-t border-[var(--color-border-soft)] pt-3">
            <button
              type="button"
              class="inline-flex h-9 items-center rounded-lg border border-[var(--color-border)] bg-transparent px-3.5 text-[12px] text-[var(--color-text-muted)] transition hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
              @click="showCreate = false"
            >
              取消
            </button>
            <button
              type="button"
              class="inline-flex h-9 items-center rounded-lg bg-[var(--color-accent)] px-4 text-[12px] font-medium text-[var(--color-accent-fg)] transition hover:bg-[var(--color-accent-hover)] disabled:opacity-50"
              :disabled="!canCreate || submitting"
              @click="submitCreate"
            >
              <Plus :size="14" />
              {{ editingReceiver ? '保存' : '创建' }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
