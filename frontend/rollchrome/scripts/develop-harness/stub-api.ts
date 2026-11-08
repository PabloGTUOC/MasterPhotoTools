/**
 * Stub API for check:develop test harness (RP-1).
 */
import type { BrowserEntry } from '@phototools/shared';
import type {
  BuiltinLookInfo,
  DevelopItem,
  DevelopPlan,
  Job,
  JobEvent,
  LutLibraryList,
  OutputFormat,
} from '../../src/api';

export interface StubDevelopState {
  activeThumbnailRequests: number;
  maxConcurrentThumbnailRequests: number;
  thumbnailRequests: string[];
  previewRequests: Array<{ name: string; look: string | null; exposure_ev: number }>;
  previewDelayMs: Record<number, number>; // delay by preview request index
  previewCallCount: number;
  lastPlanFolder: string | null;
  lastPlanItems: DevelopItem[] | null;
  lastPlanFormat: OutputFormat | null;
  lastApplyFolder: string | null;
  lastApplyItems: DevelopItem[] | null;
  lastApplyFormat: OutputFormat | null;
  cancelledJobIds: string[];
  activeJobListeners: Array<(event: JobEvent) => void>;
}

const stubState: StubDevelopState = {
  activeThumbnailRequests: 0,
  maxConcurrentThumbnailRequests: 0,
  thumbnailRequests: [],
  previewRequests: [],
  previewDelayMs: {},
  previewCallCount: 0,
  lastPlanFolder: null,
  lastPlanItems: null,
  lastPlanFormat: null,
  lastApplyFolder: null,
  lastApplyItems: null,
  lastApplyFormat: null,
  cancelledJobIds: [],
  activeJobListeners: [],
};

declare global {
  interface Window {
    __DEVELOP_STUB__: StubDevelopState;
  }
}
if (typeof window !== 'undefined') {
  window.__DEVELOP_STUB__ = stubState;
}

// 1x1 valid JPEG image bytes
const DUMMY_JPEG_BASE64 =
  '/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wgALCAABAAEBAREA/8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxA=';
const DUMMY_JPEG_BYTES = Uint8Array.from(atob(DUMMY_JPEG_BASE64), (c) => c.charCodeAt(0));

// Distinct JPEG bytes (different lengths) so test 5 can distinguish responses
function makeDummyJpeg(length: number): Uint8Array {
  const bytes = new Uint8Array(length);
  bytes.set(DUMMY_JPEG_BYTES.subarray(0, Math.min(DUMMY_JPEG_BYTES.length, length)));
  return bytes;
}

const STUB_RAW_FILES = [
  'DSCF0001.RAF',
  'DSCF0002.RAF',
  'DSCF0003.RAF',
  'DSCF0004.RAF',
  'DSCF0005.RAF',
  'DSCF0006.RAF',
];

const BUILTIN_LOOKS: BuiltinLookInfo[] = [
  // 5 Fujifilm
  { id: 'builtin:fujifilm-astia', name: 'Astia / Soft', maker: 'Fujifilm' },
  { id: 'builtin:fujifilm-classic-chrome', name: 'Classic Chrome', maker: 'Fujifilm' },
  { id: 'builtin:fujifilm-provia', name: 'Provia / Standard', maker: 'Fujifilm' },
  { id: 'builtin:fujifilm-velvia', name: 'Velvia / Vivid', maker: 'Fujifilm' },
  { id: 'builtin:fujifilm-acros', name: 'Acros (B&W)', maker: 'Fujifilm' },
  // 7 Ricoh
  { id: 'builtin:ricoh-standard', name: 'Standard', maker: 'Ricoh' },
  { id: 'builtin:ricoh-vivid', name: 'Vivid', maker: 'Ricoh' },
  { id: 'builtin:ricoh-monotone', name: 'Monotone', maker: 'Ricoh' },
  { id: 'builtin:ricoh-soft-monochrome', name: 'Soft Monotone', maker: 'Ricoh' },
  { id: 'builtin:ricoh-hard-monochrome', name: 'Hard Monotone', maker: 'Ricoh' },
  { id: 'builtin:ricoh-high-contrast-bw', name: 'Hi-Contrast B&W', maker: 'Ricoh' },
  { id: 'builtin:ricoh-positive-film', name: 'Positive Film', maker: 'Ricoh' },
];

export class StubDesktopApiClient {
  async health(): Promise<{ status: string; version: string }> {
    return { status: 'ok', version: 'stub' };
  }

  async roots(): Promise<string[]> {
    return ['/Volumes/Photos', '/Users/test/Pictures'];
  }

  async list(path: string): Promise<BrowserEntry[]> {
    return [
      { name: 'DCIM', is_dir: true, path: `${path}/DCIM` },
      { name: 'RAW', is_dir: true, path: `${path}/RAW` },
      { name: 'DSCF0001.RAF', is_dir: false, path: `${path}/DSCF0001.RAF` },
    ];
  }

  async listBuiltinLooks(): Promise<BuiltinLookInfo[]> {
    return BUILTIN_LOOKS;
  }

  async listLuts(): Promise<LutLibraryList> {
    return {
      luts: [
        { name: 'Warm Vintage', path: '/luts/warm.cube', size_bytes: 1024, mtime_ms: 1000 },
        { name: 'Cool Film', path: '/luts/cool.cube', size_bytes: 1024, mtime_ms: 1000 },
      ],
      errors: [],
    };
  }

  async listDevelopFolder(_folder: string): Promise<string[]> {
    return STUB_RAW_FILES;
  }

  async developThumbnail(_folder: string, name: string): Promise<Uint8Array> {
    stubState.activeThumbnailRequests++;
    stubState.thumbnailRequests.push(name);
    if (stubState.activeThumbnailRequests > stubState.maxConcurrentThumbnailRequests) {
      stubState.maxConcurrentThumbnailRequests = stubState.activeThumbnailRequests;
    }

    // Small delay to detect any concurrent overlap
    await new Promise((resolve) => setTimeout(resolve, 20));

    stubState.activeThumbnailRequests--;
    return DUMMY_JPEG_BYTES;
  }

  async previewDevelop(req: {
    folder: string;
    name: string;
    look: string | null;
    exposure_ev: number;
  }): Promise<Uint8Array> {
    const callIdx = ++stubState.previewCallCount;
    stubState.previewRequests.push({
      name: req.name,
      look: req.look,
      exposure_ev: req.exposure_ev,
    });

    const delay = stubState.previewDelayMs[callIdx] ?? 10;
    if (delay > 0) {
      await new Promise((resolve) => setTimeout(resolve, delay));
    }

    // Distinguish preview responses by size so test 5 can assert latest-wins
    const size = 150 + callIdx * 10;
    return makeDummyJpeg(size);
  }

  async planDevelop(
    folder: string,
    items: DevelopItem[],
    format: OutputFormat,
  ): Promise<DevelopPlan> {
    stubState.lastPlanFolder = folder;
    stubState.lastPlanItems = JSON.parse(JSON.stringify(items));
    stubState.lastPlanFormat = JSON.parse(JSON.stringify(format));

    const ext = format.kind === 'tiff' ? 'tif' : 'jpg';
    const dest = `${folder.replace(/\/+$/, '')}/Developed`;

    // Stub 5 actions and 1 skip (requirement 7)
    const actions = items.slice(0, 5).map((it) => ({
      file: it.name,
      look: it.look,
      exposure_ev: it.exposure_ev,
      output: `${dest}/${it.name.replace(/\.[^.]+$/, '')}.${ext}`,
    }));

    const skipped = items.slice(5).map((it) => ({
      file: it.name,
      reason: `already developed into ${dest}/${it.name.replace(/\.[^.]+$/, '')}.${ext}`,
    }));

    return {
      actions,
      skipped,
      format,
    };
  }

  async applyDevelop(
    folder: string,
    items: DevelopItem[],
    format: OutputFormat,
  ): Promise<string> {
    stubState.lastApplyFolder = folder;
    stubState.lastApplyItems = JSON.parse(JSON.stringify(items));
    stubState.lastApplyFormat = JSON.parse(JSON.stringify(format));

    return `job_dev_${Date.now()}`;
  }

  async cancelJob(id: string): Promise<boolean> {
    stubState.cancelledJobIds.push(id);
    for (const listener of stubState.activeJobListeners) {
      listener({
        id,
        kind: 'develop',
        state: 'cancelled',
        progress: 2,
        total: 5,
        message: '2 developed, 0 failed, 1 skipped',
        terminal: true,
      });
    }
    return true;
  }

  async watchJob(
    id: string,
    onEvent: (event: JobEvent) => void,
    signal?: AbortSignal,
  ): Promise<void> {
    stubState.activeJobListeners.push(onEvent);

    onEvent({
      id,
      kind: 'develop',
      state: 'running',
      progress: 1,
      total: 5,
      message: 'Developing photo 1/5: DSCF0001.RAF',
      terminal: false,
    });

    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        if (!stubState.cancelledJobIds.includes(id)) {
          onEvent({
            id,
            kind: 'develop',
            state: 'completed',
            progress: 5,
            total: 5,
            message: '5 developed, 0 failed, 1 skipped',
            terminal: true,
          });
        }
        resolve();
      }, 500);

      signal?.addEventListener('abort', () => {
        clearTimeout(timer);
        resolve();
      });
    });
  }

  async job(id: string): Promise<Job> {
    const isCancelled = stubState.cancelledJobIds.includes(id);
    return {
      id,
      kind: 'develop',
      status: isCancelled ? 'cancelled' : 'completed',
      progress: isCancelled ? 2 : 5,
      total: 5,
      started_at: Date.now() - 1000,
      finished_at: Date.now(),
      error: null,
      summary: isCancelled ? '2 developed, 0 failed, 1 skipped' : '5 developed, 0 failed, 1 skipped',
    };
  }
}

export const api = new StubDesktopApiClient();
export const desktop = api;
