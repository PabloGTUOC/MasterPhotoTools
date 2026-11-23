import type { ComputedRef, InjectionKey, Ref, WritableComputedRef } from 'vue';
import type { BrowserEntry } from '@phototools/shared';
import type {
  AdjustmentRecipe,
  AutoTarget,
  BlackAndWhite,
  ColorWheel as ColorWheelType,
  Detail,
  FilmGrain,
  HslAdjustments,
  LookEffects,
  LutLibraryList,
  LutRef,
  Mask,
  PointColour,
  ToneCurves,
  Vignette,
} from '@host/api';

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

export type BwBandKey = keyof BlackAndWhite;

export interface BwBandDef {
  key: BwBandKey;
  label: string;
}

export interface HslBandDef {
  key: keyof HslAdjustments;
  label: string;
  hueDeg: number;
}

export type GradingWheelKey = 'shadows' | 'midtones' | 'highlights' | 'global';

export interface GradingWheelDef {
  key: GradingWheelKey;
  label: string;
}

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

  // Colour / HSL section
  resetHsl: () => void;
  toggleBlackAndWhite: () => void;
  isBwMixDefault: ComputedRef<boolean> | Ref<boolean>;
  resetBwMix: () => void;
  BW_BANDS: readonly BwBandDef[] | BwBandDef[];
  getBwBand: (key: BwBandKey) => number;
  onBwSliderInput: (key: BwBandKey, val: number) => void;
  onBwSliderChange: (key: BwBandKey, val: number) => void;
  HSL_BANDS: HslBandDef[];
  selectedHslBand: Ref<keyof HslAdjustments>;
  isBandModified: (key: keyof HslAdjustments) => boolean;
  selectHslBand: (key: keyof HslAdjustments) => void;
  currentBandDef: ComputedRef<HslBandDef>;
  resetCurrentHslBand: () => void;
  onHslSliderInput: (prop: 'hue' | 'saturation' | 'luminance', val: number) => void;
  onHslSliderChange: (prop: 'hue' | 'saturation' | 'luminance', val: number) => void;
  togglePointColourEyedropper: () => void;
  selectedPointIndex: Ref<number>;
  selectPoint: (idx: number) => void;
  selectedPoint: ComputedRef<PointColour | null>;
  onPointColourSliderInput: (prop: 'hue_shift' | 'saturation' | 'luminance' | 'range', val: number) => void;
  onPointColourSliderChange: (prop: 'hue_shift' | 'saturation' | 'luminance' | 'range', val: number) => void;
  deleteSelectedPoint: () => void;

  // Colour Grading section
  resetGrading: () => void;
  GRADING_WHEELS: GradingWheelDef[];
  selectedGradingWheel: Ref<GradingWheelKey>;
  isGradingWheelModified: (key: GradingWheelKey) => boolean;
  selectGradingWheel: (key: GradingWheelKey) => void;
  currentGradingWheelDef: ComputedRef<GradingWheelDef>;
  resetCurrentGradingWheel: () => void;
  onGradingWheelInput: (val: ColorWheelType) => void;
  onGradingWheelChange: (val: ColorWheelType) => void;
  onGradingParamInput: (param: 'blending' | 'balance', val: number) => void;
  onGradingParamChange: (param: 'blending' | 'balance', val: number) => void;

  // Look section
  resetLook: () => void;
  lutList: Ref<LutLibraryList>;
  roots: Ref<string[]>;
  rootsError: Ref<string | null>;
  listRoots: (p: string) => Promise<BrowserEntry[]>;
  setLut: (lut: LutRef | null) => void;
  setLutIntensity: (val: number) => void;
  onLutIntensityChange: (val: number) => void;
  handleImportLut: (path: string) => Promise<void>;
  setToneMapper: (mode: 'none' | 'aces') => void;
  onLooksSliderInput: (prop: keyof LookEffects, val: number) => void;
  onLooksSliderChange: (prop: keyof LookEffects, val: number) => void;

  // Detail section
  resetDetail: () => void;
  zoom: Ref<'fit' | number>;
  onDetailSliderInput: (prop: keyof Detail, val: number) => void;
  onDetailSliderChange: (prop: keyof Detail, val: number) => void;
  toggleHotPixels: () => void;

  // Geometry section
  resetGeometry: () => void;
  toggleCropMode: () => void;
  selectedAspect: WritableComputedRef<string> | Ref<string>;
  onAspectChange: () => void;
  straightenAngle: WritableComputedRef<number> | Ref<number>;
  onStraightenInput: (val: number) => void;
  onStraightenChange: (val: number) => void;
  rotateCCW: () => void;
  rotateCW: () => void;
  isFlipH: ComputedRef<boolean>;
  toggleFlipH: () => void;
  isFlipV: ComputedRef<boolean>;
  toggleFlipV: () => void;

  // Effects section
  resetEffects: () => void;
  onVignetteSliderInput: (prop: keyof Vignette, val: number) => void;
  onVignetteSliderChange: (prop: keyof Vignette, val: number) => void;
  onGrainSliderInput: (prop: keyof FilmGrain, val: number) => void;
  onGrainSliderChange: (prop: keyof FilmGrain, val: number) => void;
}

export const EDIT_CONTEXT: InjectionKey<EditContext> = Symbol('edit');
