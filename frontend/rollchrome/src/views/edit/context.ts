import type { ComputedRef, InjectionKey, Ref } from 'vue';
import type { AdjustmentRecipe, AutoTarget, Mask, ToneCurves } from '@host/api';

export type EyedropperTarget =
  | 'white-balance'
  | 'point-colour'
  | 'colour-range-mask'
  | 'colour-range-repick'
  | 'colour-range-component'
  | 'object'
  | null;

export type AutoDownloadState = {
  target: AutoTarget | 'object';
  name: string;
  jobId: string | null;
  asBackground?: boolean;
  componentForMaskId?: string;
} | null;

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

  // Presence section
  resetPresence: () => void;
  isReadOnly: Ref<boolean>;
  onPresenceSliderInput: (prop: 'clarity' | 'texture' | 'dehaze', val: number) => void;
  onPresenceSliderChange: (prop: 'clarity' | 'texture' | 'dehaze', val: number) => void;

  // Masks section
  resetMasks: () => void;
  masks: ComputedRef<Mask[]>;
  MASK_LIMIT: number;
  addMask: (type: 'linear' | 'radial' | 'brush') => void;
  addLuminanceMask: () => void;
  startColourRangeEyedropper: () => void;
  autoBusy: Ref<AutoTarget | 'object' | null>;
  addAutoMask: (target: AutoTarget) => Promise<void>;
  addObjectMask: () => Promise<void>;
  addBackgroundMask: () => Promise<void>;
  exitEyedropper: () => void;
  autoDownload: Ref<AutoDownloadState>;
  AUTO_LABEL: Record<AutoTarget | 'object', string>;
  startModelDownload: () => Promise<void>;
  cancelModelDownload: () => Promise<void>;
  onModelDownloaded: () => Promise<void>;
  stopAutoMask: () => void;
  selectedMaskId: Ref<string | null>;
  toggleMask: (id: string, field: 'enabled' | 'invert') => void;
  deleteMask: (id: string) => void;

  // Tone Curve section
  resetToneCurve: () => void;
  onCurvesInput: (curves: ToneCurves) => void;
  onCurvesChange: (curves: ToneCurves) => void;
}

export const EDIT_CONTEXT: InjectionKey<EditContext> = Symbol('edit');
