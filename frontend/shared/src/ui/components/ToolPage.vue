<script setup lang="ts">
/**
 * The scaffold every tool view shares: title, form, the mandatory dry-run
 * confirmation, and job progress.
 *
 * Specification principle 5 — nothing destructive happens without a preview.
 * The apply button is unreachable until a dry run has been reviewed.
 */
import { ref } from 'vue';
import JobProgress from './JobProgress.vue';

const props = defineProps<{
  title: string;
  blurb: string;
  /** Whether this tool has a reviewable dry run. */
  hasPreview?: boolean;
  /** Label for the destructive action. */
  applyLabel?: string;
  busy?: boolean;
}>();

const emit = defineEmits<{
  preview: [];
  apply: [];
}>();

const jobId = ref<string | null>(null);
const reviewed = ref(false);
const failure = ref<string | null>(null);

function setJob(id: string | null) {
  jobId.value = id;
}
function setReviewed(value: boolean) {
  reviewed.value = value;
}
function setFailure(message: string | null) {
  failure.value = message;
}

defineExpose({ setJob, setReviewed, setFailure });
</script>

<template>
  <article class="tool">
    <header>
      <h1>{{ props.title }}</h1>
      <p class="muted">{{ props.blurb }}</p>
    </header>

    <slot name="form" />

    <p v-if="failure" class="error" role="alert">{{ failure }}</p>

    <slot name="preview" />

    <footer class="actions">
      <button
        v-if="props.hasPreview"
        type="button"
        class="secondary"
        :disabled="props.busy"
        @click="emit('preview')"
      >
        Preview changes
      </button>
      <button
        type="button"
        class="primary"
        :disabled="props.busy || (props.hasPreview && !reviewed)"
        @click="emit('apply')"
      >
        {{ props.applyLabel ?? 'Run' }}
      </button>
    </footer>

    <p v-if="props.hasPreview && !reviewed" class="muted gate">
      Preview first. Nothing is written until you have seen what would change.
    </p>

    <JobProgress :job-id="jobId" />
  </article>
</template>

<style scoped>
.tool {
  display: grid;
  gap: 18px;
  max-width: 46rem;
}
header h1 {
  font-size: 1.4rem;
  letter-spacing: -0.01em;
}
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}
.gate {
  font-size: 0.85rem;
}
</style>
