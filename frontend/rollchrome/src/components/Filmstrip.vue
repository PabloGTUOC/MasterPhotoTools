<script setup lang="ts">
/**
 * Filmstrip component for Rollchrome (UI-7, UI-8).
 *
 * Horizontal scrollable strip of photograph thumbnail cards along the bottom
 * of Develop and Edit screens. Transport-free: props in, events out.
 */
import { nextTick, ref, watch, onMounted } from 'vue';

export interface FilmstripItem {
  name: string;
  caption?: string;
}

const props = withDefaults(
  defineProps<{
    items: FilmstripItem[];
    thumbnailUrls: Record<string, string>;
    thumbnailErrors?: Record<string, string>;
    selection: string[];
    active: string | null;
  }>(),
  {
    thumbnailErrors: () => ({}),
  },
);

const emit = defineEmits<{
  (e: 'select', name: string, modifiers: { meta: boolean; shift: boolean }): void;
  (e: 'activate', name: string): void;
  (e: 'selectAll'): void;
}>();

const stripRef = ref<HTMLElement | null>(null);

function scrollActiveIntoView(name: string | null) {
  if (!name || !stripRef.value) return;
  nextTick(() => {
    const card = stripRef.value?.querySelector<HTMLElement>(
      `[data-filename="${CSS.escape(name)}"]`,
    );
    if (!card || !stripRef.value) return;
    const cardLeft = card.offsetLeft;
    const cardRight = cardLeft + card.offsetWidth;
    const stripLeft = stripRef.value.scrollLeft;
    const stripRight = stripLeft + stripRef.value.clientWidth;
    if (cardLeft < stripLeft) {
      stripRef.value.scrollLeft = cardLeft;
    } else if (cardRight > stripRight) {
      stripRef.value.scrollLeft = cardRight - stripRef.value.clientWidth;
    }
  });
}

watch(
  () => props.active,
  (newActive) => {
    scrollActiveIntoView(newActive);
  },
);

onMounted(() => {
  scrollActiveIntoView(props.active);
});

function onCardClick(name: string, event: MouseEvent) {
  emit('select', name, {
    meta: Boolean(event.metaKey || event.ctrlKey),
    shift: Boolean(event.shiftKey),
  });
}

function onKeyDown(e: KeyboardEvent) {
  if ((e.metaKey || e.ctrlKey) && (e.key === 'a' || e.key === 'A')) {
    e.preventDefault();
    emit('selectAll');
    return;
  }

  if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
    if (!props.items.length) return;
    e.preventDefault();

    let currentIndex = -1;
    if (props.active) {
      currentIndex = props.items.findIndex((item) => item.name === props.active);
    }
    let targetIndex: number;
    if (currentIndex === -1) {
      targetIndex = e.key === 'ArrowRight' ? 0 : props.items.length - 1;
    } else {
      if (e.key === 'ArrowLeft') {
        targetIndex = Math.max(0, currentIndex - 1);
      } else {
        targetIndex = Math.min(props.items.length - 1, currentIndex + 1);
      }
    }

    const targetItem = props.items[targetIndex];
    if (!targetItem) return;

    if (e.shiftKey) {
      emit('select', targetItem.name, { meta: false, shift: true });
    } else {
      emit('activate', targetItem.name);
    }
  }
}
</script>

<template>
  <div
    ref="stripRef"
    class="filmstrip"
    role="listbox"
    aria-label="Photographs"
    aria-multiselectable="true"
    tabindex="0"
    data-testid="thumbnail-grid"
    @keydown="onKeyDown"
  >
    <div
      v-for="item in items"
      :key="item.name"
      class="thumbnail-card"
      :class="{
        active: selection.includes(item.name),
        'is-last-clicked': active === item.name,
      }"
      :data-filename="item.name"
      data-testid="thumbnail-item"
      role="option"
      :aria-selected="selection.includes(item.name)"
      @click="onCardClick(item.name, $event)"
    >
      <div class="thumb-container">
        <img
          v-if="thumbnailUrls[item.name]"
          :src="thumbnailUrls[item.name]"
          :alt="item.name"
          class="thumb-img"
        />
        <div v-else-if="thumbnailErrors?.[item.name]" class="thumb-error mono">
          <span>{{ item.name }}</span>
        </div>
        <div v-else class="thumb-placeholder">
          <span class="cursor">_</span>
        </div>
      </div>
      <div class="thumb-meta">
        <span class="thumb-name mono" :title="item.name">{{ item.name }}</span>
        <span class="thumb-look mono">
          {{ item.caption ?? '' }}
        </span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.filmstrip {
  display: flex;
  flex-direction: row;
  align-items: stretch;
  gap: var(--space-2);
  height: 132px;
  max-height: 132px;
  box-sizing: border-box;
  padding: var(--space-2);
  background: var(--bg-surface);
  border: var(--border-hair);
  overflow-x: auto;
  overflow-y: hidden;
  outline: none;
  border-radius: 2px;
}

.filmstrip:focus-visible {
  border-color: var(--accent);
}

.thumbnail-card {
  flex: 0 0 120px;
  width: 120px;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  padding: var(--space-1);
  border: var(--border-hair);
  background: var(--bg);
  cursor: pointer;
  border-radius: 2px;
  user-select: none;
}

.thumbnail-card.active {
  border-color: var(--accent);
  background: var(--bg-surface);
}

.thumbnail-card.is-last-clicked {
  outline: 1px solid var(--accent);
}

.thumb-container {
  width: 100%;
  aspect-ratio: 3 / 2;
  background: var(--bg);
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}

.thumb-img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}

.thumb-error {
  padding: 2px;
  font-size: 11px;
  color: var(--text-muted);
  text-align: center;
  word-break: break-all;
}

.thumb-placeholder {
  color: var(--text-muted);
}

.thumb-meta {
  display: grid;
  gap: 2px;
  font-size: 11px;
  margin-top: auto;
}

.thumb-name {
  color: var(--text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.thumb-look {
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
