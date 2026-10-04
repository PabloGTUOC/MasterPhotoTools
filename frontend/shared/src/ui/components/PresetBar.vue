<script setup lang="ts">
/**
 * Presets and copy/paste of settings for the Edit panel (ED-17).
 *
 * Transport-free: the library arrives as props, and saving, renaming and deleting
 * are functions passed in, as `FolderPicker` takes `list`, so a dialog can show
 * the reason a call failed beside the name that caused it rather than somewhere
 * else on the screen.
 *
 * - Choosing a preset applies it at once, as in every darkroom application; the
 *   host offers an Undo, which is what makes applying on a single click safe.
 * - Saving under a name already taken asks before replacing it.
 * - Deleting asks first, in the panel rather than with a browser dialog, which
 *   would block the webview.
 */
import { computed, nextTick, ref } from 'vue';

const props = defineProps<{
  presets: string[];
  /** Preset files that could not be read, so they are reported rather than vanishing. */
  unreadable: { name: string; error: string }[];
  /** The preset last applied to this photograph, if any. */
  selected: string | null;
  canPaste: boolean;
  disabled?: boolean;
  save: (name: string, overwrite: boolean) => Promise<void>;
  rename: (from: string, to: string) => Promise<void>;
  remove: (name: string) => Promise<void>;
}>();

const emit = defineEmits<{
  apply: [name: string];
  copy: [];
  paste: [];
}>();

type Dialog =
  | { kind: 'save'; name: string; replacing: boolean }
  | { kind: 'rename'; from: string; name: string }
  | { kind: 'delete'; name: string };

const dialog = ref<Dialog | null>(null);
const dialogError = ref<string | null>(null);
const working = ref(false);
const nameInput = ref<HTMLInputElement | null>(null);

const hasSelection = computed(() => props.selected !== null && props.presets.includes(props.selected));

function open(d: Dialog) {
  dialog.value = d;
  dialogError.value = null;
  void nextTick(() => nameInput.value?.focus());
}

function close() {
  if (working.value) return;
  dialog.value = null;
  dialogError.value = null;
}

function onSelect(e: Event) {
  const name = (e.target as HTMLSelectElement).value;
  if (name) emit('apply', name);
}

async function run(action: () => Promise<void>) {
  working.value = true;
  dialogError.value = null;
  try {
    await action();
    dialog.value = null;
  } catch (err: unknown) {
    dialogError.value = err instanceof Error ? err.message : String(err);
  } finally {
    working.value = false;
  }
}

async function submit() {
  const d = dialog.value;
  if (!d || working.value) return;
  if (d.kind === 'delete') {
    await run(() => props.remove(d.name));
    return;
  }
  const name = d.name.trim();
  if (!name) {
    dialogError.value = 'Give the preset a name.';
    return;
  }
  if (d.kind === 'save') {
    if (props.presets.includes(name) && !d.replacing) {
      d.replacing = true;
      dialogError.value = null;
      return;
    }
    await run(() => props.save(name, d.replacing));
  } else {
    if (name === d.from) {
      close();
      return;
    }
    await run(() => props.rename(d.from, name));
  }
}

function onNameInput() {
  // A different name is a different question: stop offering to replace the old one.
  if (dialog.value?.kind === 'save') dialog.value.replacing = false;
  dialogError.value = null;
}

const submitLabel = computed(() => {
  const d = dialog.value;
  if (!d) return '';
  if (d.kind === 'delete') return 'Delete';
  if (d.kind === 'rename') return 'Rename';
  return d.replacing ? 'Replace' : 'Save';
});
</script>

<template>
  <div class="preset-bar" data-testid="preset-bar">
    <label class="preset-bar__label" for="preset-select">Preset</label>
    <select
      id="preset-select"
      class="preset-bar__select"
      data-testid="preset-select"
      :value="hasSelection ? selected : ''"
      :disabled="disabled || presets.length === 0"
      @change="onSelect"
    >
      <option value="" disabled>{{ presets.length ? 'Apply a preset…' : 'No presets saved yet' }}</option>
      <option v-for="p in presets" :key="p" :value="p">{{ p }}</option>
    </select>

    <div class="preset-bar__actions">
      <button
        type="button"
        class="ghost"
        data-testid="preset-save-btn"
        :disabled="disabled"
        @click="open({ kind: 'save', name: '', replacing: false })"
      >
        Save as preset…
      </button>
      <button
        type="button"
        class="ghost"
        data-testid="preset-rename-btn"
        :disabled="disabled || !hasSelection"
        @click="open({ kind: 'rename', from: selected!, name: selected! })"
      >
        Rename…
      </button>
      <button
        type="button"
        class="ghost"
        data-testid="preset-delete-btn"
        :disabled="disabled || !hasSelection"
        @click="open({ kind: 'delete', name: selected! })"
      >
        Delete…
      </button>
    </div>

    <div class="preset-bar__actions preset-bar__actions--clipboard">
      <button
        type="button"
        class="ghost"
        data-testid="copy-settings-btn"
        title="Copy settings (⌘C)"
        :disabled="disabled"
        @click="emit('copy')"
      >
        Copy settings
      </button>
      <button
        type="button"
        class="ghost"
        data-testid="paste-settings-btn"
        title="Paste settings (⌘V)"
        :disabled="disabled || !canPaste"
        @click="emit('paste')"
      >
        Paste settings
      </button>
    </div>

    <p v-if="unreadable.length" class="preset-bar__unreadable" data-testid="preset-unreadable">
      {{ unreadable.length === 1 ? 'One preset' : `${unreadable.length} presets` }} could not be read:
      {{ unreadable.map((u) => `${u.name} (${u.error})`).join('; ') }}
    </p>

    <div
      v-if="dialog"
      class="preset-dialog"
      role="dialog"
      aria-modal="true"
      :aria-label="submitLabel + ' preset'"
      data-testid="preset-dialog"
      @keydown.esc.stop="close"
    >
      <form class="preset-dialog__body" @submit.prevent="submit">
        <template v-if="dialog.kind === 'delete'">
          <p class="preset-dialog__text">
            Delete the preset <strong>{{ dialog.name }}</strong>? Photographs it was applied to keep
            their settings.
          </p>
        </template>
        <template v-else>
          <label class="preset-dialog__label" for="preset-name">
            {{ dialog.kind === 'save' ? 'Save the current settings as' : `Rename ${dialog.from} to` }}
          </label>
          <input
            id="preset-name"
            ref="nameInput"
            v-model="dialog.name"
            type="text"
            class="preset-dialog__input"
            data-testid="preset-name-input"
            maxlength="64"
            autocomplete="off"
            @input="onNameInput"
          />
          <p v-if="dialog.kind === 'save'" class="preset-dialog__hint">
            Crop, rotation, flip and masks stay with each photograph and are not saved.
          </p>
          <p
            v-if="dialog.kind === 'save' && dialog.replacing"
            class="preset-dialog__warning"
            data-testid="preset-replace-warning"
          >
            A preset called {{ dialog.name.trim() }} already exists. Replace it?
          </p>
        </template>

        <p v-if="dialogError" class="preset-dialog__error" role="alert" data-testid="preset-dialog-error">
          {{ dialogError }}
        </p>

        <div class="preset-dialog__buttons">
          <button type="button" class="ghost" data-testid="preset-cancel-btn" :disabled="working" @click="close">
            Cancel
          </button>
          <button
            type="submit"
            :class="dialog.kind === 'delete' || (dialog.kind === 'save' && dialog.replacing) ? 'danger' : 'primary'"
            data-testid="preset-submit-btn"
            :disabled="working"
          >
            {{ submitLabel }}
          </button>
        </div>
      </form>
    </div>
  </div>
</template>

<style scoped>
.preset-bar {
  display: grid;
  gap: var(--space-2);
}

.preset-bar__label {
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-muted);
}

.preset-bar__select {
  min-height: 40px;
  width: 100%;
  font-size: 16px;
  border-radius: var(--radius-none);
}

.preset-bar__actions {
  display: grid;
  grid-template-columns: 1.6fr 1fr 1fr;
  gap: var(--space-1);
}

.preset-bar__actions--clipboard {
  grid-template-columns: 1fr 1fr;
}

.preset-bar__actions button {
  min-height: 40px;
  padding: 0 var(--space-1);
  font-size: 12px;
}

.preset-bar__unreadable {
  margin: 0;
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--amber);
}

/* Over the whole window, as one modal question at a time. */
.preset-dialog {
  position: fixed;
  inset: 0;
  z-index: var(--z-modal);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-4);
  background: color-mix(in srgb, var(--void) 80%, transparent);
}

.preset-dialog__body {
  display: grid;
  gap: var(--space-3);
  width: min(420px, 100%);
  padding: var(--space-4);
  background: var(--bg-elevated);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-none);
}

.preset-dialog__label,
.preset-dialog__text {
  margin: 0;
  font-family: var(--font-body);
  font-size: 14px;
  color: var(--text);
}

.preset-dialog__input {
  min-height: 40px;
  font-size: 16px;
}

.preset-dialog__hint {
  margin: 0;
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--text-muted);
}

.preset-dialog__warning {
  margin: 0;
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--amber);
}

.preset-dialog__error {
  margin: 0;
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--danger);
}

.preset-dialog__buttons {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
}

.preset-dialog__buttons button {
  min-height: 40px;
  padding: 0 var(--space-4);
}
</style>
