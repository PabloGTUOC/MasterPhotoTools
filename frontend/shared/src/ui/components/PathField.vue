<script setup lang="ts">
/**
 * A path you can either type or choose.
 *
 * The field stays editable on purpose: typing is faster when the path is
 * already known, and a path pasted from somewhere else has to keep working.
 * The picker is the way in when it is not known, which was the whole complaint
 * about typing one blind.
 *
 * Transport-free, like everything in `components/`: `roots` and `list` arrive
 * as props from a view that has a client.
 */
import { ref, onMounted, onUnmounted } from 'vue';
import FolderPicker from './FolderPicker.vue';
import type { BrowserEntry } from '@phototools/shared';

const props = withDefaults(
  defineProps<{
    modelValue: string;
    label: string;
    roots: string[];
    /** Why `roots` could not be had, if asking failed; see FolderPicker. */
    rootsError?: string | null;
    list: (path: string) => Promise<BrowserEntry[]>;
    placeholder?: string;
    hint?: string;
    chooseLabel?: string;
    /**
     * The value names a file, not a folder.
     *
     * A picker can only choose directories, so choosing one here replaces the
     * directory and keeps the file name that was already there. Typing is
     * untouched — only the picker's result is rewritten.
     */
    keepFileName?: boolean;
    /**
     * File extensions, without the dot, that the picker may choose directly.
     *
     * With these, choosing a file *is* the answer and `keepFileName` never comes
     * into it. Without them the picker offers folders only, which is right for an
     * output path that does not exist yet.
     */
    selectable?: string[];
    /**
     * The name to use when `keepFileName` is set and the field is still empty.
     *
     * Without it, choosing a folder into an empty file field yields the folder
     * itself, and the tool is then asked to write its output over a directory —
     * which fails at the filesystem with "Is a directory", far from the cause.
     */
    fileNameFallback?: string;
    /**
     * When true, FolderPicker opens in an absolutely positioned popover below the field
     * rather than pushing the layout.
     */
    overlay?: boolean;
  }>(),
  {
    overlay: false,
  },
);

const emit = defineEmits<{
  'update:modelValue': [value: string];
  choose: [value: string];
}>();

const picking = ref(false);
const fieldRef = ref<HTMLElement | null>(null);

function onWindowKeyDown(e: KeyboardEvent) {
  if (props.overlay && picking.value && e.key === 'Escape') {
    e.preventDefault();
    picking.value = false;
  }
}

function onWindowPointerDown(e: MouseEvent | PointerEvent) {
  if (!props.overlay || !picking.value) return;
  if (fieldRef.value && !fieldRef.value.contains(e.target as Node)) {
    picking.value = false;
  }
}

onMounted(() => {
  window.addEventListener('keydown', onWindowKeyDown);
  window.addEventListener('pointerdown', onWindowPointerDown);
});

onUnmounted(() => {
  window.removeEventListener('keydown', onWindowKeyDown);
  window.removeEventListener('pointerdown', onWindowPointerDown);
});

/** Whether the picker handed back one of the file types this field accepts. */
function isChosenFile(path: string): boolean {
  const extensions = props.selectable ?? [];
  if (!extensions.length) return false;
  const name = path.split('/').pop() ?? '';
  const dot = name.lastIndexOf('.');
  return dot >= 0 && extensions.includes(name.slice(dot + 1).toLowerCase());
}

function choose(path: string) {
  // A chosen file is the whole answer: it exists, it is the right type, and
  // there is no name left to keep.
  if (props.keepFileName && !isChosenFile(path)) {
    const name = props.modelValue.split('/').pop()?.trim() || props.fileNameFallback;
    // With no name and no fallback the field is left alone rather than being
    // filled with a directory: a picker must not hand a tool a path it cannot
    // write to.
    if (name) {
      const full = `${path}/${name}`;
      emit('update:modelValue', full);
      emit('choose', full);
    }
  } else {
    emit('update:modelValue', path);
    emit('choose', path);
  }
  picking.value = false;
}
</script>

<template>
  <div ref="fieldRef" class="path-field" :class="{ 'path-field--overlay': props.overlay }">
    <label class="field">
      <span>{{ props.label }}</span>
      <div class="row">
        <input
          :value="props.modelValue"
          type="text"
          spellcheck="false"
          :placeholder="props.placeholder"
          @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
        />
        <button
          type="button"
          class="secondary"
          data-testid="path-browse"
          :aria-expanded="picking"
          @click="picking = !picking"
        >
          {{ picking ? 'Close' : 'Choose…' }}
        </button>
      </div>
      <small v-if="props.hint" class="muted">{{ props.hint }}</small>
    </label>

    <div
      v-if="picking"
      class="path-field__picker-wrap"
      :class="{ 'path-field__popover': props.overlay }"
    >
      <FolderPicker
        :roots="props.roots"
        :roots-error="props.rootsError"
        :list="props.list"
        :choose-label="props.chooseLabel"
        :selectable="props.selectable"
        @choose="choose"
        @cancel="picking = false"
      />
    </div>
  </div>
</template>

<style scoped>
.path-field {
  display: grid;
  gap: 8px;
}
.path-field--overlay {
  position: relative;
}
.path-field__popover {
  position: absolute;
  top: 100%;
  left: 0;
  width: 100%;
  max-height: 60vh;
  overflow-y: auto;
  z-index: var(--z-modal);
  background: var(--bg-elevated);
  border: var(--border-hair);
  box-sizing: border-box;
}
.row {
  display: flex;
  gap: 8px;
  align-items: stretch;
}
.row input {
  flex: 1;
  min-width: 0;
}
.row button {
  /* Never let the control wrap under the field on a narrow screen: it reads as
     belonging to the next question when it does. check:layout runs at 390px. */
  white-space: nowrap;
}
</style>
