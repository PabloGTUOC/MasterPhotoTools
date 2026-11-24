<script setup lang="ts">
/**
 * 3D LUT selector, intensity control, and importer (ED-7).
 *
 * Transport-free: props in, events out, never `@host/api`.
 *
 * Features:
 * - Select 3D LUT from library with clear None/Identity option.
 * - Intensity slider (0-100%) adjusting blend factor.
 * - Displays unparseable LUT errors in the library with their error message.
 * - Import trigger accepting user-selected file path.
 */
import { computed, onMounted, ref } from 'vue';
import type { BrowserEntry } from '@phototools/shared';
import AdjustmentSlider from './AdjustmentSlider.vue';
import PathField from '@ui/components/PathField.vue';
import { desktop } from '@host/api';
import type { BuiltinLookInfo } from '@host/api';

export interface LutItem {
  name: string;
  sha256: string;
  format?: string;
}

export interface LutErrorItem {
  name: string;
  error: string;
}

const props = withDefaults(
  defineProps<{
    modelValue: { name: string; sha256: string } | null;
    intensity: number; // 0.0 to 1.0 (internal float)
    luts: LutItem[];
    errors?: LutErrorItem[];
    roots?: string[];
    rootsError?: string | null;
    list?: (path: string) => Promise<BrowserEntry[]>;
    disabled?: boolean;
  }>(),
  {
    errors: () => [],
    roots: () => [],
    rootsError: null,
    list: undefined,
    disabled: false,
  },
);

const emit = defineEmits<{
  'update:modelValue': [lut: { name: string; sha256: string } | null];
  'update:intensity': [intensity: number];
  change: [intensity: number];
  import: [path: string];
}>();

const importing = ref(false);
const importPath = ref('');
const builtinLooks = ref<BuiltinLookInfo[]>([]);
const filmLooks = ref<BuiltinLookInfo[]>([]);
const preparing = ref(false);
const ensureError = ref<string | null>(null);

onMounted(async () => {
  try {
    const [builtins, films] = await Promise.all([
      desktop.listBuiltinLooks(),
      desktop.listFilmLooks(),
    ]);
    builtinLooks.value = builtins;
    filmLooks.value = films;
  } catch (err: unknown) {
    // The library still works without them, so the picker stays usable, but says why the groups are missing.
    ensureError.value = `Camera looks and film simulations could not be listed: ${
      err instanceof Error ? err.message : String(err)
    }`;
  }
});

const builtinFileNames = computed(() => {
  return new Set(builtinLooks.value.map((l) => l.library_file_name));
});

const filmFileNames = computed(() => {
  return new Set(filmLooks.value.map((l) => l.library_file_name));
});

const yourLuts = computed(() => {
  return props.luts.filter(
    (l) => !builtinFileNames.value.has(l.name) && !filmFileNames.value.has(l.name),
  );
});

const selectedOptionValue = computed(() => {
  if (!props.modelValue) return '';
  const currentName = props.modelValue.name;
  const currentSha = props.modelValue.sha256;

  // Check camera looks
  const builtin = builtinLooks.value.find(
    (l) => l.library_file_name === currentName,
  );
  if (builtin) return builtin.id;

  // Check film looks
  const film = filmLooks.value.find(
    (l) => l.library_file_name === currentName,
  );
  if (film) return film.id;

  // Check library LUTs by sha256 or name
  const found = props.luts.find(
    (l) => l.sha256 === currentSha || l.name === currentName,
  );
  if (found) return found.sha256;

  return currentSha || '';
});

const intensityPercent = computed(() => Math.round(props.intensity * 100));

async function onSelectLut(e: Event) {
  const val = (e.target as HTMLSelectElement).value;
  if (!val) {
    ensureError.value = null;
    emit('update:modelValue', null);
    return;
  }

  if (val.startsWith('builtin:') || val.startsWith('film:')) {
    ensureError.value = null;
    preparing.value = true;
    try {
      const lutRef = await desktop.ensureLookLut(val);
      emit('update:modelValue', { name: lutRef.name, sha256: lutRef.sha256 });
    } catch (err: unknown) {
      ensureError.value = err instanceof Error ? err.message : String(err);
      (e.target as HTMLSelectElement).value = selectedOptionValue.value;
    } finally {
      preparing.value = false;
    }
    return;
  }

  ensureError.value = null;
  const found = props.luts.find((l) => l.sha256 === val);
  if (found) {
    emit('update:modelValue', { name: found.name, sha256: found.sha256 });
  } else {
    emit('update:modelValue', null);
  }
}

function onIntensityInput(percent: number) {
  emit('update:intensity', percent / 100);
}

function onIntensityChange(percent: number) {
  emit('change', percent / 100);
}

function handleImport() {
  if (!importPath.value.trim()) return;
  emit('import', importPath.value.trim());
  importPath.value = '';
  importing.value = false;
}
</script>

<template>
  <div class="lut-picker" :class="{ 'lut-picker--disabled': props.disabled }">
    <div class="lut-picker__header">
      <span class="lut-picker__title">3D LUT Film Simulation</span>
      <button
        type="button"
        class="secondary lut-picker__import-btn"
        data-testid="lut-import-toggle"
        :disabled="props.disabled"
        @click="importing = !importing"
      >
        {{ importing ? 'Cancel' : 'Import LUT…' }}
      </button>
    </div>

    <!-- Import row -->
    <div v-if="importing" class="lut-picker__import-panel">
      <PathField
        v-if="props.list"
        v-model="importPath"
        label="LUT file to import (.cube, .3dl, .png)"
        :roots="props.roots"
        :roots-error="props.rootsError"
        :list="props.list"
        :selectable="['cube', '3dl', 'png']"
        placeholder="/path/to/lut.cube"
        :overlay="true"
      />
      <div v-else class="lut-picker__simple-import">
        <label class="field">
          <span>LUT file path (.cube, .3dl, .png)</span>
          <input
            v-model="importPath"
            type="text"
            placeholder="/path/to/lut.cube"
            data-testid="lut-import-input"
          />
        </label>
      </div>
      <button
        type="button"
        class="primary lut-picker__submit-import"
        data-testid="lut-import-submit"
        :disabled="!importPath.trim()"
        @click="handleImport"
      >
        Import to library
      </button>
    </div>

    <!-- LUT Select dropdown -->
    <label class="field lut-picker__select-field">
      <span>Active LUT</span>
      <select
        :value="selectedOptionValue"
        class="lut-picker__select"
        data-testid="lut-select"
        :disabled="props.disabled || preparing"
        @change="onSelectLut"
      >
        <option value="">None (untouched / no LUT)</option>
        <optgroup v-if="builtinLooks.length > 0" label="Camera looks">
          <option v-for="look in builtinLooks" :key="look.id" :value="look.id">
            {{ look.maker }} {{ look.name }}
          </option>
        </optgroup>
        <optgroup v-if="filmLooks.length > 0" label="Film simulations">
          <option v-for="look in filmLooks" :key="look.id" :value="look.id">
            {{ look.name }}
          </option>
        </optgroup>
        <optgroup v-if="yourLuts.length > 0" label="Your LUTs">
          <option v-for="lut in yourLuts" :key="lut.sha256" :value="lut.sha256">
            {{ lut.name }} {{ lut.format ? `(${lut.format.toUpperCase()})` : '' }}
          </option>
        </optgroup>
      </select>
    </label>

    <div v-if="preparing" class="lut-picker__preparing" data-testid="lut-preparing">
      Preparing…
    </div>

    <!-- Intensity Slider (only relevant when a LUT is selected) -->
    <AdjustmentSlider
      v-if="props.modelValue"
      label="LUT Intensity"
      :model-value="intensityPercent"
      :min="0"
      :max="100"
      :step="1"
      unit="%"
      :default-value="100"
      :disabled="props.disabled"
      test-id="lut-intensity"
      @update:model-value="onIntensityInput"
      @change="onIntensityChange"
    />

    <!-- Unparseable LUT errors from library or look preparation error -->
    <div
      v-if="ensureError || (props.errors && props.errors.length > 0)"
      class="lut-picker__errors"
      data-testid="lut-errors"
    >
      <div v-if="ensureError" class="lut-picker__error-item error" data-testid="lut-ensure-error">
        <span class="lut-picker__error-msg">{{ ensureError }}</span>
      </div>
      <template v-if="props.errors && props.errors.length > 0">
        <div class="lut-picker__errors-title error">Unparseable LUT files in library:</div>
        <div
          v-for="err in props.errors"
          :key="err.name"
          class="lut-picker__error-item error"
          data-testid="lut-error-item"
        >
          <span class="lut-picker__error-name">{{ err.name }}:</span>
          <span class="lut-picker__error-msg">{{ err.error }}</span>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.lut-picker {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-3);
  border: var(--border-hair);
  background: var(--bg-panel);
}

.lut-picker--disabled {
  opacity: 0.5;
  pointer-events: none;
}

.lut-picker__header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.lut-picker__title {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-muted);
}

.lut-picker__import-btn {
  min-height: 40px;
  padding: var(--space-1) var(--space-3);
  font-size: 12px;
}

.lut-picker__import-panel {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-2);
  border: var(--border-dash);
  background: var(--bg);
}

.lut-picker__submit-import {
  justify-self: start;
  min-height: 40px;
}

.lut-picker__select-field {
  gap: var(--space-1);
}

.lut-picker__select {
  width: 100%;
  min-height: 44px;
  height: 44px;
  font-family: var(--font-body);
  font-size: 16px;
  padding: var(--space-2) var(--space-3);
  border: var(--border-hair);
  background: var(--bg);
  color: var(--text);
  border-radius: var(--radius-none);
}

.lut-picker__preparing {
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--accent);
  letter-spacing: 0.05em;
}

.lut-picker__errors {
  display: grid;
  gap: var(--space-1);
  padding: var(--space-2);
  background: rgba(255, 45, 85, 0.05);
  border: 1px solid var(--danger);
}

.lut-picker__errors-title {
  font-weight: bold;
  font-size: 12px;
}

.lut-picker__error-item {
  display: flex;
  gap: var(--space-1);
  font-size: 12px;
  word-break: break-all;
}

.lut-picker__error-name {
  font-weight: bold;
}
</style>
