/**
 * Settings copied from one photograph, waiting to be pasted onto another (ED-17).
 *
 * Kept in the application rather than on the system clipboard. Reading the system
 * clipboard from a webview raises a permission prompt on macOS at every paste, and
 * a recipe is of no use to any other application. Held at module level, so it
 * survives moving between tabs, which unmounts the Edit view.
 */
import { ref } from 'vue';
import type { AdjustmentRecipe } from './api';

export interface CopiedSettings {
  recipe: AdjustmentRecipe;
  /** The file the settings were copied from, named in the confirmation. */
  from: string;
}

export const copiedSettings = ref<CopiedSettings | null>(null);
