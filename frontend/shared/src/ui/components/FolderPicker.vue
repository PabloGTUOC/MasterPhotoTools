<script setup lang="ts">
/**
 * Choose a folder by clicking rather than by typing.
 *
 * Transport-free, as everything in `components/` is: the caller supplies the
 * two questions this needs answered — which roots may be browsed, and what is
 * inside a directory — so the picker works over HTTP, over Tauri, or over a
 * stub in a test with no application around it.
 *
 * **The two front ends see different filesystems, and that is not a bug.** The
 * desktop's lister runs on the Mac, so it offers local folders and any mounted
 * NAS share; the web's runs inside the container on the NAS and offers what is
 * mounted into it. Each shows what its own side can reach.
 *
 * Only directories are selectable by default. Files are shown, greyed, because
 * a folder with nothing in it looks identical to a wrong turn otherwise.
 *
 * `selectable` turns named file types into choices too — a `.gpx` track is a
 * *file*, and asking somebody to browse to its folder and then type its name is
 * how the pickers came to exist in the first place.
 */
import { ref, watch } from 'vue';
import type { BrowserEntry } from '@phototools/shared';

const props = defineProps<{
  /** Directories the browse may start from. G6 refuses everything else. */
  roots: string[];
  /** Lists one directory. Rejects for anything outside a root. */
  list: (path: string) => Promise<BrowserEntry[]>;
  /** Wording for the confirm button, e.g. "Use this folder". */
  chooseLabel?: string;
  /**
   * Extensions, without the dot, that may be chosen as well as folders.
   *
   * Empty or absent keeps the picker to directories, which is what every
   * caller but the track loader wants.
   */
  selectable?: string[];
}>();

const emit = defineEmits<{
  /** A folder was confirmed. Carries its absolute path. */
  choose: [path: string];
  cancel: [];
}>();

const current = ref<string | null>(null);
const entries = ref<BrowserEntry[]>([]);
const loading = ref(false);
const failure = ref<string | null>(null);

/**
 * `..` is filtered out of the listing and expressed as the breadcrumb instead.
 *
 * `f9_browser` emits it as an entry, but a picker that shows both a breadcrumb
 * trail and a `..` row offers two controls for one idea, and the row is the one
 * that reads as a folder you could choose.
 */
async function open(path: string) {
  loading.value = true;
  failure.value = null;
  try {
    const listed = await props.list(path);
    entries.value = listed.filter((e) => e.name !== '..');
    current.value = path;
  } catch (e) {
    // A root that no longer exists, or a path G6 refuses. Either way the
    // previous listing stays on screen rather than emptying under the cursor.
    failure.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

/** Whether this file is one of the types the caller will accept. */
function isSelectable(name: string): boolean {
  const extensions = props.selectable ?? [];
  if (!extensions.length) return false;
  const dot = name.lastIndexOf('.');
  if (dot < 0) return false;
  return extensions.includes(name.slice(dot + 1).toLowerCase());
}

/** The trail from the containing root to the current directory. */
function crumbs(): { label: string; full: string }[] {
  if (!current.value) return [];
  const root = props.roots.find((r) => current.value === r || current.value!.startsWith(`${r}/`));
  if (!root) return [];

  const rest = current.value.slice(root.length).split('/').filter(Boolean);
  const trail = [{ label: root, full: root }];
  let walked = root;
  for (const part of rest) {
    walked = `${walked}/${part}`;
    trail.push({ label: part, full: walked });
  }
  return trail;
}

// Start at the only root when there is exactly one; with several, the choice of
// where to begin is the person's rather than ours.
watch(
  () => props.roots,
  (roots) => {
    if (roots.length === 1 && current.value === null) open(roots[0]);
  },
  { immediate: true },
);
</script>

<template>
  <div class="picker" role="group" aria-label="Choose a folder">
    <p v-if="!props.roots.length" class="error" role="alert">
      No folders are configured, so there is nothing to browse. Set ROOTS to the directories this
      application may touch.
    </p>

    <template v-else>
      <nav v-if="current" class="crumbs" aria-label="Breadcrumb">
        <button
          v-for="crumb in crumbs()"
          :key="crumb.full"
          type="button"
          class="crumb"
          @click="open(crumb.full)"
        >
          {{ crumb.label }}
        </button>
      </nav>

      <ul v-if="!current" class="entries">
        <li v-for="root in props.roots" :key="root">
          <button type="button" class="entry" @click="open(root)">
            <span class="entry-icon" aria-hidden="true">/</span>
            <span class="entry-name">{{ root }}</span>
          </button>
        </li>
      </ul>

      <p v-if="failure" class="error" role="alert">{{ failure }}</p>
      <p v-else-if="loading" class="muted">Loading…</p>

      <ul v-else-if="current" class="entries">
        <li v-for="entry in entries" :key="entry.absolute_path">
          <button
            v-if="entry.is_dir"
            type="button"
            class="entry"
            data-testid="picker-dir"
            @click="open(entry.absolute_path)"
          >
            <span class="entry-icon" aria-hidden="true">/</span>
            <span class="entry-name">{{ entry.name }}</span>
          </button>
          <button
            v-else-if="isSelectable(entry.name)"
            type="button"
            class="entry"
            data-testid="picker-file"
            @click="emit('choose', entry.absolute_path)"
          >
            <span class="entry-icon" aria-hidden="true">·</span>
            <span class="entry-name">{{ entry.name }}</span>
          </button>
          <div v-else class="entry entry-file">
            <span class="entry-icon" aria-hidden="true">·</span>
            <span class="entry-name">{{ entry.name }}</span>
          </div>
        </li>
        <li v-if="!entries.length" class="muted empty">This folder is empty.</li>
      </ul>

      <footer class="actions">
        <button
          type="button"
          class="primary"
          data-testid="picker-choose"
          :disabled="!current"
          @click="current && emit('choose', current)"
        >
          {{ props.chooseLabel ?? 'Use this folder' }}
        </button>
        <button type="button" class="secondary" @click="emit('cancel')">Cancel</button>
      </footer>
    </template>
  </div>
</template>

<style scoped>
/* A terminal file browser: bracketed frame, breadcrumb trail, one listing.
   The entry and crumb faces live in components.css rather than here, so every
   picker in the application looks the same without each restating it. */
.picker {
  display: grid;
  gap: var(--space-3);
  border: var(--border-hair);
  border-radius: var(--radius-none);
  padding: var(--space-3);
  background: var(--bg-elevated);
}

.entries {
  /* Bounded so the picker cannot push the form's buttons off the screen. */
  max-height: 40vh;
  overflow-y: auto;
}

.empty {
  padding: var(--space-3);
  font-family: var(--font-body);
  font-size: 13px;
}

.actions {
  display: flex;
  gap: var(--space-3);
  flex-wrap: wrap;
}
</style>
