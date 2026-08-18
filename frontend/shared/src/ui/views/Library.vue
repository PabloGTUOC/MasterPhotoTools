<script setup lang="ts">
/** F9 — library browser, with breadcrumbs, usable one-handed. */
import { onMounted, ref, watch } from 'vue';
import type { BrowserEntry } from '@phototools/shared';
import { api } from '@host/api';
import { useRoots } from '../useRoots';

const path = ref('');
// Where browsing may start. Without these the screen opens blank and the only
// way in is to already know a path and type it — which is no browser at all.
const { roots } = useRoots();
const entries = ref<BrowserEntry[]>([]);
const failure = ref<string | null>(null);
const loading = ref(false);

async function load(target: string) {
  if (!target) return;
  loading.value = true;
  failure.value = null;
  try {
    entries.value = await api.list(target);
    path.value = target;
  } catch (e) {
    failure.value = e instanceof Error ? e.message : String(e);
    entries.value = [];
  } finally {
    loading.value = false;
  }
}

/**
 * Path segments, each navigable, from the containing root downwards.
 *
 * The trail stops at the root rather than at `/`: G6 refuses every path above
 * one, so a crumb for `/` is a button that can only produce an error.
 */
function crumbs(): { label: string; full: string }[] {
  const root = roots.value.find((r) => path.value === r || path.value.startsWith(`${r}/`));
  if (!root) return [];

  const out = [{ label: root, full: root }];
  let walked = root;
  for (const part of path.value.slice(root.length).split('/').filter(Boolean)) {
    walked += `/${part}`;
    out.push({ label: part, full: walked });
  }
  return out;
}

function size(entry: BrowserEntry): string {
  if (entry.size === null) return '';
  const units = ['B', 'kB', 'MB', 'GB'];
  let value = entry.size;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

onMounted(() => {
  const start = new URLSearchParams(location.search).get('path');
  if (start) void load(start);
});

// With one root there is nowhere else to begin, so begin there. With several,
// the choice is the person's and they are listed instead.
watch(roots, (available) => {
  if (!path.value && available.length === 1) void load(available[0]);
});
</script>

<template>
  <section class="library">
    <header>
      <h1>Library</h1>
      <p class="muted">Browsing is confined to the configured roots.</p>
    </header>

    <form class="path-form" @submit.prevent="load(path)">
      <label class="field">
        <span>Path</span>
        <input v-model="path" type="text" inputmode="url" placeholder="/mnt/photos" />
      </label>
      <button type="submit" class="primary" :disabled="loading">Open</button>
    </form>

    <nav v-if="crumbs().length" class="crumbs" aria-label="Breadcrumb">
      <button
        v-for="crumb in crumbs()"
        :key="crumb.full"
        type="button"
        class="crumb"
        @click="load(crumb.full)"
      >
        {{ crumb.label }}
      </button>
    </nav>

    <p v-if="failure" class="error" role="alert">{{ failure }}</p>
    <p v-else-if="loading" class="muted">Loading…</p>

    <ul v-else-if="!path && roots.length" class="entries">
      <li v-for="root in roots" :key="root">
        <button type="button" class="entry" @click="load(root)">
          <span class="entry-icon" aria-hidden="true">/</span>
          <span class="entry-name">{{ root }}</span>
        </button>
      </li>
    </ul>

    <ul v-else-if="entries.length" class="entries">
      <li v-for="entry in entries" :key="entry.absolute_path">
        <button
          v-if="entry.is_dir"
          type="button"
          class="entry"
          @click="load(entry.absolute_path)"
        >
          <span class="entry-icon" aria-hidden="true">/</span>
          <span class="entry-name">{{ entry.name }}</span>
        </button>
        <div v-else class="entry entry-file">
          <span class="entry-icon" aria-hidden="true">·</span>
          <span class="entry-name">{{ entry.name }}</span>
          <span class="entry-size">{{ size(entry) }}</span>
        </div>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.library { display: grid; gap: 16px; }
.path-form {
  display: flex;
  gap: 8px;
  align-items: flex-end;
  flex-wrap: wrap;
}
.path-form .field { flex: 1 1 14rem; }
.crumbs {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  align-items: center;
}
.crumb {
  background: none;
  border: none;
  color: var(--accent);
  font-family: var(--mono);
  font-size: 0.85rem;
  padding: 6px 4px;
  cursor: pointer;
}
.crumb::after { content: '/'; color: var(--ink-faint); margin-left: 4px; }
.crumb:last-child::after { content: ''; }
.entries { list-style: none; display: grid; gap: 2px; }
.entry {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  /* 44px is the smallest comfortable touch target. */
  min-height: 44px;
  padding: 8px 10px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.entry-file { cursor: default; }
button.entry:hover, button.entry:focus-visible { background: var(--surface-2); }
.entry-icon { font-family: var(--mono); color: var(--ink-faint); }
.entry-name { flex: 1; word-break: break-all; }
.entry-size {
  font-family: var(--mono);
  font-size: 0.8rem;
  color: var(--ink-faint);
  font-variant-numeric: tabular-nums;
}
</style>
