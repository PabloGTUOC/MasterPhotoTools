/**
 * Module-level state for Develop view (RP-1).
 *
 * Held at module level (as `recipeClipboard.ts` does), so it survives moving
 * between tabs, which unmounts the Develop view. Nothing is written to disk.
 */
import { ref } from 'vue';
import type { Grain, OutputFormat } from './api';

export interface PhotoAssignment {
  look: string | null;
  exposure_ev: number;
  grain?: Grain;
  noise_reduction?: number;
}

export const moduleFolder = ref('');
export const moduleAssignments = ref<Record<string, PhotoAssignment>>({});
export const moduleSelection = ref<string[]>([]);
export const moduleLastClicked = ref<string | null>(null);
export const moduleFormat = ref<OutputFormat>({ kind: 'jpeg', quality: 95 });
export const moduleScope = ref<'all' | 'selected'>('all');
export const moduleJobId = ref<string | null>(null);
export const moduleIsDeveloping = ref(false);
