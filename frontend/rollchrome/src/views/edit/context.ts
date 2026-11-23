import type { ComputedRef, InjectionKey, Ref } from 'vue';
import type { AdjustmentRecipe, Mask } from '@host/api';

export type EyedropperTarget =
  | 'white-balance'
  | 'point-colour'
  | 'colour-range-mask'
  | 'colour-range-repick'
  | 'colour-range-component'
  | 'object'
  | null;

export interface EditContext {
  // Basic section
  selectedMask: ComputedRef<Mask | null> | Ref<Mask | null>;
  resetBasic: () => void;
  sessionId: Ref<string | null>;
  loading: Ref<boolean>;
  triggerAutoTone: () => Promise<void>;
  recipe: Ref<AdjustmentRecipe>;
  onRecipeValueInput: () => void;
  onRecipeValueChange: () => void;
  eyedropperTarget: Ref<EyedropperTarget>;
  isCropMode: Ref<boolean>;
  isBefore: Ref<boolean>;
  compareMode: Ref<'off' | 'side-by-side' | 'split'>;
  toggleEyedropper: () => void;
  isBwActive: ComputedRef<boolean> | Ref<boolean>;
}

export const EDIT_CONTEXT: InjectionKey<EditContext> = Symbol('edit');
