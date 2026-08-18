<script setup lang="ts">
/**
 * Several paths, typed or chosen.
 *
 * The tools that take "paths, one per line" accept **files as well as
 * folders** — naming three particular frames is a real thing to want — so the
 * textarea stays and the picker adds to it rather than replacing it. Choosing
 * a folder appends a line; nothing already typed is disturbed.
 *
 * Transport-free, like everything in `components/`.
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
  rows?: number;
}>();

const emit = defineEmits<{ 'update:modelValue': [value: string] }>();

const picking = ref(false);

/** Append the folder as its own line, ignoring a duplicate. */
function add(path: string) {
  const lines = props.modelValue.split('\n').map((l) => l.trim()).filter(Boolean);
  if (!lines.includes(path)) lines.push(path);
  emit('update:modelValue', lines.join('\n'));
  picking.value = false;
}
</script>

<template>
  <div class="path-list-field">
    <label class="field">
      <span>{{ props.label }}</span>
      <textarea
        :value="props.modelValue"
        :rows="props.rows ?? 4"
        spellcheck="false"
        :placeholder="props.placeholder"
        @input="emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)"
      ></textarea>
    </label>

    <button
      type="button"
      class="secondary"
      data-testid="path-list-browse"
      :aria-expanded="picking"
      @click="picking = !picking"
    >
      {{ picking ? 'Close' : 'Add a folder…' }}
    </button>

    <FolderPicker
      v-if="picking"
      :roots="props.roots"
      :list="props.list"
      choose-label="Add this folder"
      @choose="add"
      @cancel="picking = false"
    />
  </div>
</template>

<style scoped>
.path-list-field {
  display: grid;
  gap: 8px;
  justify-items: start;
}
.path-list-field .field,
.path-list-field textarea {
  width: 100%;
}
</style>
