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
import { ref } from 'vue';
import FolderPicker from './FolderPicker.vue';
import type { BrowserEntry } from '@phototools/shared';

const props = defineProps<{
  modelValue: string;
  label: string;
  roots: string[];
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
}>();

const emit = defineEmits<{ 'update:modelValue': [value: string] }>();

const picking = ref(false);

function choose(path: string) {
  if (props.keepFileName) {
    const name = props.modelValue.split('/').pop();
    emit('update:modelValue', name ? `${path}/${name}` : path);
  } else {
    emit('update:modelValue', path);
  }
  picking.value = false;
}
</script>

<template>
  <div class="path-field">
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

    <FolderPicker
      v-if="picking"
      :roots="props.roots"
      :list="props.list"
      :choose-label="props.chooseLabel"
      @choose="choose"
      @cancel="picking = false"
    />
  </div>
</template>

<style scoped>
.path-field {
  display: grid;
  gap: 8px;
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
