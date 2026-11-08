/**
 * Module-level state for Edit export options (RP-9).
 *
 * Held at module level, as recipeClipboard.ts is, so that settings survive
 * navigating between tabs while the application is open.
 */

import { reactive, computed } from 'vue';
import type { ExportFormat, ExportOptions, ExportResize, MetadataPolicy, OutputSharpening } from './api';

export type FormatKind = 'AsSource' | 'Jpeg' | 'Tiff' | 'Png' | 'Webp' | 'Avif';
export type ResizeKind = 'Full' | 'LongEdge' | 'Megapixels';

export interface ExportState {
  formatKind: FormatKind;
  jpegQuality: number;
  avifQuality: number;
  tiffBits: number;
  pngBits: number;
  resizeKind: ResizeKind;
  resizeValue: string;
  outputSharpening: OutputSharpening;
  nameTemplate: string;
  metadataPolicy: MetadataPolicy;
}

export const exportState = reactive<ExportState>({
  formatKind: 'AsSource',
  jpegQuality: 95,
  avifQuality: 70,
  tiffBits: 16,
  pngBits: 8,
  resizeKind: 'Full',
  resizeValue: '2048',
  outputSharpening: 'Off',
  nameTemplate: '',
  metadataPolicy: 'All',
});

/** True when the resize input is valid (not empty and > 0 for LongEdge/Megapixels). */
export const isResizeValid = computed(() => {
  if (exportState.resizeKind === 'Full') return true;
  const str = String(exportState.resizeValue ?? '').trim();
  const num = Number(str);
  return str !== '' && !isNaN(num) && num > 0;
});

/** Disabled reason if resize is invalid. */
export const resizeInvalidReason = computed(() => {
  if (isResizeValid.value) return null;
  return 'Target size must be greater than zero.';
});

/** Build the backend ExportOptions struct from the reactive state. */
export function buildExportOptions(): ExportOptions {
  let format: ExportFormat;
  switch (exportState.formatKind) {
    case 'AsSource':
      format = 'AsSource';
      break;
    case 'Jpeg':
      format = { Jpeg: { quality: Math.min(100, Math.max(1, Math.round(Number(exportState.jpegQuality) || 95))) } };
      break;
    case 'Tiff':
      format = { Tiff: { bits: Number(exportState.tiffBits) === 8 ? 8 : 16 } };
      break;
    case 'Png':
      format = { Png: { bits: Number(exportState.pngBits) === 16 ? 16 : 8 } };
      break;
    case 'Webp':
      format = 'Webp';
      break;
    case 'Avif':
      format = { Avif: { quality: Math.min(100, Math.max(1, Math.round(Number(exportState.avifQuality) || 70))) } };
      break;
  }

  let resize: ExportResize;
  switch (exportState.resizeKind) {
    case 'Full':
      resize = 'None';
      break;
    case 'LongEdge':
      resize = { LongEdge: Math.max(1, Math.round(Number(exportState.resizeValue) || 1)) };
      break;
    case 'Megapixels':
      resize = { Megapixels: Math.max(1, Math.round(Number(exportState.resizeValue) || 1)) };
      break;
  }

  return {
    format,
    resize,
    output_sharpening: exportState.outputSharpening,
    name_template: exportState.nameTemplate.trim() ? exportState.nameTemplate : null,
    metadata: exportState.metadataPolicy,
  };
}
