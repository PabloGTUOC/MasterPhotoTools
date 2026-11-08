/**
 * In-memory history and undo/redo manager for the active photograph (RP-5).
 *
 * Stores recipes with snapshots left out, compares them without snapshots,
 * and maintains a bounded stack under both maxEntries and maxBytes limits.
 * Oldest entries are evicted when limits are exceeded; the current state is never dropped.
 */
import type { AdjustmentRecipe } from './api';

export interface HistoryEntryView {
  label: string;
}

interface HistoryItem {
  label: string;
  recipe: AdjustmentRecipe;
  jsonLength: number;
}

function stripSnapshots(recipe: AdjustmentRecipe): AdjustmentRecipe {
  const clone: AdjustmentRecipe = JSON.parse(JSON.stringify(recipe));
  delete clone.snapshots;
  delete clone.versions;
  delete clone.version_name;
  return clone;
}

export class EditHistory {
  private readonly maxEntries: number;
  private readonly maxBytes: number;
  private items: HistoryItem[] = [];
  private initialRecipe: AdjustmentRecipe;
  private _cursor = -1;
  private totalBytes = 0;

  constructor(maxEntries = 100, maxBytes = 64 * 1024 * 1024) {
    this.maxEntries = maxEntries;
    this.maxBytes = maxBytes;
    this.initialRecipe = { version: 3, source_sha256: '' };
  }

  /**
   * Resets the history with the photograph's loaded recipe.
   * Clears all steps and sets cursor to -1.
   */
  reset(initial: AdjustmentRecipe): void {
    this.initialRecipe = stripSnapshots(initial);
    this.items = [];
    this._cursor = -1;
    this.totalBytes = 0;
  }

  /**
   * Commits a new change.
   * Discards what could have been redone.
   * Returns false if the recipe (excluding snapshots) has not changed.
   */
  commit(recipe: AdjustmentRecipe, label: string): boolean {
    const stripped = stripSnapshots(recipe);
    const json = JSON.stringify(stripped);

    const current =
      this._cursor >= 0 ? this.items[this._cursor].recipe : this.initialRecipe;
    if (JSON.stringify(current) === json) {
      return false;
    }

    // Discard any redo entries ahead of cursor
    if (this._cursor < this.items.length - 1) {
      const removed = this.items.splice(this._cursor + 1);
      for (const r of removed) {
        this.totalBytes -= r.jsonLength;
      }
    }

    const item: HistoryItem = {
      label,
      recipe: stripped,
      jsonLength: json.length,
    };
    this.items.push(item);
    this.totalBytes += item.jsonLength;
    this._cursor = this.items.length - 1;

    // Evict oldest entries if past maxEntries or maxBytes
    // The current state is never dropped (_cursor must remain > 0 to drop items[0]).
    while (
      (this.items.length > this.maxEntries || this.totalBytes > this.maxBytes) &&
      this.items.length > 1 &&
      this._cursor > 0
    ) {
      const oldest = this.items.shift()!;
      this.totalBytes -= oldest.jsonLength;
      this._cursor--;
      this.initialRecipe = oldest.recipe;
    }

    return true;
  }

  /**
   * Undoes the last committed change.
   * Returns null if there is nothing to undo.
   */
  undo(): AdjustmentRecipe | null {
    if (this._cursor < 0) {
      return null;
    }
    this._cursor--;
    if (this._cursor < 0) {
      return JSON.parse(JSON.stringify(this.initialRecipe));
    }
    return JSON.parse(JSON.stringify(this.items[this._cursor].recipe));
  }

  /**
   * Redoes the next undone change.
   * Returns null if there is nothing to redo.
   */
  redo(): AdjustmentRecipe | null {
    if (this._cursor >= this.items.length - 1) {
      return null;
    }
    this._cursor++;
    return JSON.parse(JSON.stringify(this.items[this._cursor].recipe));
  }

  /**
   * Jumps directly to a given history entry index.
   * Jumping only moves the cursor.
   * Index -1 jumps back to initial state.
   */
  jump(index: number): AdjustmentRecipe | null {
    if (index < -1 || index >= this.items.length) {
      return null;
    }
    this._cursor = index;
    if (index === -1) {
      return JSON.parse(JSON.stringify(this.initialRecipe));
    }
    return JSON.parse(JSON.stringify(this.items[index].recipe));
  }

  /**
   * Read-only view of history entries in chronological order.
   */
  get entries(): readonly HistoryEntryView[] {
    return this.items.map((item) => ({ label: item.label }));
  }

  /**
   * Index of the current history entry (-1 if at initial unedited state).
   */
  get cursor(): number {
    return this._cursor;
  }
}
