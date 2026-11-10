/**
 * Stub API for check:develop test harness (RP-1).
 */
import type { BrowserEntry } from '@phototools/shared';
import type {
  BuiltinLookInfo,
  DevelopItem,
  DevelopPlan,
  Grain,
  Job,
  JobEvent,
  LutLibraryList,
  OutputFormat,
} from '../../src/api';

export interface StubDevelopState {
  activeThumbnailRequests: number;
  maxConcurrentThumbnailRequests: number;
  thumbnailRequests: string[];
  activePreviewRequests: number;
  maxConcurrentPreviewRequests: number;
  previewRequests: Array<{ name: string; look: string | null; exposure_ev: number; draft: boolean; grain?: Grain }>;
  previewDelayMs: Record<number, number>;
  previewCallCount: number;
  distinctPreviewCallIdx: number | null;
  lastPlanFolder: string | null;
  lastPlanItems: DevelopItem[] | null;
  lastPlanFormat: OutputFormat | null;
  lastApplyFolder: string | null;
  lastApplyItems: DevelopItem[] | null;
  lastApplyFormat: OutputFormat | null;
  cancelledJobIds: string[];
  activeJobListeners: Array<(event: JobEvent) => void>;
  thumbnailDelayMs: number;
}

const stubState: StubDevelopState = {
  activeThumbnailRequests: 0,
  maxConcurrentThumbnailRequests: 0,
  thumbnailRequests: [],
  activePreviewRequests: 0,
  maxConcurrentPreviewRequests: 0,
  previewRequests: [],
  previewDelayMs: {},
  previewCallCount: 0,
  distinctPreviewCallIdx: null,
  lastPlanFolder: null,
  lastPlanItems: null,
  lastPlanFormat: null,
  lastApplyFolder: null,
  lastApplyItems: null,
  lastApplyFormat: null,
  cancelledJobIds: [],
  activeJobListeners: [],
  thumbnailDelayMs: 20,
};

declare global {
  interface Window {
    __DEVELOP_STUB__: StubDevelopState;
  }
}
if (typeof window !== 'undefined') {
  window.__DEVELOP_STUB__ = stubState;
}

// Valid 64x64 and 80x64 JPEG images (RP-1 review item 7)
const VALID_JPEG_64_B64 =
  '/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAUDBAQEAwUEBAQFBQUGBwwIBwcHBw8LCwkMEQ8SEhEPERETFhwXExQaFRERGCEYGh0dHx8fExciJCIeJBweHx7/2wBDAQUFBQcGBw4ICA4eFBEUHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh7/wAARCABAAEADASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDPooor7g+JCiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKAP//Z';
const VALID_JPEG_64_BYTES = Uint8Array.from(atob(VALID_JPEG_64_B64), (c) => c.charCodeAt(0));

const VALID_JPEG_80_B64 =
  '/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAUDBAQEAwUEBAQFBQUGBwwIBwcHBw8LCwkMEQ8SEhEPERETFhwXExQaFRERGCEYGh0dHx8fExciJCIeJBweHx7/2wBDAQUFBQcGBw4ICA4eFBEUHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh7/wAARCABAAFADASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwClRRRXyR+hhRRRQAUUUUAFFFFABRRRQAUUUUAFFFFABRRRQAUUUUAFFFFABRRRQAUUUUAFFFFABRRRQAUUUUAFFFFABRRRQAUUUUAFFFFABRRRQB//2Q==';
const VALID_JPEG_80_BYTES = Uint8Array.from(atob(VALID_JPEG_80_B64), (c) => c.charCodeAt(0));

const STUB_RAW_FILES = [
  'DSCF0001.RAF',
  'DSCF0002.RAF',
  'DSCF0003.RAF',
  'DSCF0004.RAF',
  'DSCF0005.RAF',
  'DSCF0006.RAF',
];

// The twelve shipped looks (RP-1 review item 7)
const BUILTIN_LOOKS: BuiltinLookInfo[] = [
  // 5 Fujifilm
  { id: 'builtin:fujifilm-classic-chrome', name: 'Classic Chrome', maker: 'Fujifilm' },
  { id: 'builtin:fujifilm-eterna', name: 'Eterna', maker: 'Fujifilm' },
  { id: 'builtin:fujifilm-pro-neg-high', name: 'Pro Neg High', maker: 'Fujifilm' },
  { id: 'builtin:fujifilm-pro-neg-std', name: 'Pro Neg Std', maker: 'Fujifilm' },
  { id: 'builtin:fujifilm-provia', name: 'Provia', maker: 'Fujifilm' },
  // 7 Ricoh
  { id: 'builtin:ricoh-bleach-bypass', name: 'Bleach Bypass', maker: 'Ricoh' },
  { id: 'builtin:ricoh-negative-film', name: 'Negative Film', maker: 'Ricoh' },
  { id: 'builtin:ricoh-positive-film', name: 'Positive Film', maker: 'Ricoh' },
  { id: 'builtin:ricoh-retro', name: 'Retro', maker: 'Ricoh' },
  { id: 'builtin:ricoh-standard', name: 'Standard', maker: 'Ricoh' },
  { id: 'builtin:ricoh-vivid', name: 'Vivid', maker: 'Ricoh' },
  { id: 'builtin:ricoh-cross-processing', name: 'Cross Processing', maker: 'Ricoh' },
];

const FILM_LOOKS: BuiltinLookInfo[] = [
  // 13 Kodak
  { id: 'film:kodak_portra_160', name: 'Kodak Portra 160', maker: 'Kodak' },
  { id: 'film:kodak_portra_400', name: 'Kodak Portra 400', maker: 'Kodak' },
  { id: 'film:kodak_portra_800', name: 'Kodak Portra 800', maker: 'Kodak' },
  { id: 'film:kodak_ektar_100', name: 'Kodak Ektar 100', maker: 'Kodak' },
  { id: 'film:kodak_gold_200', name: 'Kodak Gold 200', maker: 'Kodak' },
  { id: 'film:kodak_ultramax_400', name: 'Kodak Ultramax 400', maker: 'Kodak' },
  { id: 'film:kodak_ektachrome_100', name: 'Kodak Ektachrome 100', maker: 'Kodak' },
  { id: 'film:kodak_kodachrome_64', name: 'Kodak Kodachrome 64', maker: 'Kodak' },
  { id: 'film:kodak_vision3_50d', name: 'Kodak Vision3 50D', maker: 'Kodak' },
  { id: 'film:kodak_vision3_200t', name: 'Kodak Vision3 200T', maker: 'Kodak' },
  { id: 'film:kodak_vision3_250d', name: 'Kodak Vision3 250D', maker: 'Kodak' },
  { id: 'film:kodak_vision3_500t', name: 'Kodak Vision3 500T', maker: 'Kodak' },
  { id: 'film:kodak_verita_200d', name: 'Kodak Verita 200D', maker: 'Kodak' },
  // 5 Fujifilm
  { id: 'film:fujifilm_c200', name: 'Fujifilm C200', maker: 'Fujifilm' },
  { id: 'film:fujifilm_pro_400h', name: 'Fujifilm Pro 400H', maker: 'Fujifilm' },
  { id: 'film:fujifilm_xtra_400', name: 'Fujifilm X-Tra 400', maker: 'Fujifilm' },
  { id: 'film:fujifilm_provia_100f', name: 'Fujifilm Provia 100F', maker: 'Fujifilm' },
  { id: 'film:fujifilm_velvia_100', name: 'Fujifilm Velvia 100', maker: 'Fujifilm' },
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
      { name: 'folder2', is_dir: true, path: `${path}/folder2` },
      { name: 'DSCF0001.RAF', is_dir: false, path: `${path}/DSCF0001.RAF` },
    ];
  }

  async listBuiltinLooks(): Promise<BuiltinLookInfo[]> {
    return BUILTIN_LOOKS;
  }

  async listFilmLooks(): Promise<BuiltinLookInfo[]> {
    return FILM_LOOKS;
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

  async listDevelopFolder(folder: string): Promise<string[]> {
    if (folder.includes('folder2')) {
      return ['DSCF0001.RAF', 'DSCF0002.RAF'];
    }
    return STUB_RAW_FILES;
  }

  async developThumbnail(_folder: string, name: string): Promise<Uint8Array> {
    stubState.activeThumbnailRequests++;
    stubState.thumbnailRequests.push(name);
    if (stubState.activeThumbnailRequests > stubState.maxConcurrentThumbnailRequests) {
      stubState.maxConcurrentThumbnailRequests = stubState.activeThumbnailRequests;
    }

    const delay = stubState.thumbnailDelayMs;
    if (delay > 0) {
      await new Promise((resolve) => setTimeout(resolve, delay));
    }

    stubState.activeThumbnailRequests--;
    return VALID_JPEG_64_BYTES;
  }

  async previewDevelop(
    _folder: string,
    name: string,
    look: string | null,
    exposure_ev: number,
    draft = false,
    grain?: Grain,
  ): Promise<Uint8Array> {
    stubState.activePreviewRequests++;
    if (stubState.activePreviewRequests > stubState.maxConcurrentPreviewRequests) {
      stubState.maxConcurrentPreviewRequests = stubState.activePreviewRequests;
    }

    const callIdx = ++stubState.previewCallCount;
    stubState.previewRequests.push({ name, look, exposure_ev, draft: !!draft, grain });

    const delay = stubState.previewDelayMs[callIdx] ?? 10;
    if (delay > 0) {
      await new Promise((resolve) => setTimeout(resolve, delay));
    }

    stubState.activePreviewRequests--;

    if (stubState.distinctPreviewCallIdx === callIdx) {
      return VALID_JPEG_80_BYTES;
    }
    return VALID_JPEG_64_BYTES;
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

    // If 6 items, stub 5 actions and 1 skip for requirement 7
    if (items.length >= 6) {
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

      return { actions, skipped, format };
    }

    const actions = items.map((it) => ({
      file: it.name,
      look: it.look,
      exposure_ev: it.exposure_ev,
      output: `${dest}/${it.name.replace(/\.[^.]+$/, '')}.${ext}`,
    }));

    return { actions, skipped: [], format };
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
