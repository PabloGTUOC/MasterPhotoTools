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
  gap: var(--space-5);
  max-width: 46rem;
}
/* No font-size here: the display scale in base.css owns headings, and a
   scoped override is how a page ends up off the type scale by accident. */
header {
  display: grid;
  gap: var(--space-2);
}
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-3);
}
.gate {
  font-size: 13px;
}
</style>
