/**
 * Stub API for check:bulk-lut test harness.
 */
import type {
  AdjustmentRecipe,
  BrowserEntry,
  BulkLutPlanSummary,
  Job,
  JobEvent,
  LutEntry,
  LutLibraryList,
  OpenPreviewResult,
  PreviewStage,
  ServerStatus,
} from '../../src/api';

export interface StubBulkLutState {
  openSessions: number;
  maxConcurrentSessions: number;
  samplePreviewsRequested: string[];
  lastPlanLutSha256: string;
  lastApplyLutSha256: string | null;
  failNextApplyWithLutChanged: boolean;
  failNextCancelWithThrow: boolean;
  failNextCancelWithFalse: boolean;
  failSamplePreviewPath: string | null;
  cancelledJobIds: string[];
  activeJobListeners: Array<(event: JobEvent) => void>;
}

const stubState: StubBulkLutState = {
  openSessions: 0,
  maxConcurrentSessions: 0,
  samplePreviewsRequested: [],
  lastPlanLutSha256: '7f83b1657ff1fc53b92dc18148a1d65dfc2d4b1fa3d677284addd200126d9069',
  lastApplyLutSha256: null,
  failNextApplyWithLutChanged: false,
  failNextCancelWithThrow: false,
  failNextCancelWithFalse: false,
  failSamplePreviewPath: null,
  cancelledJobIds: [],
  activeJobListeners: [],
};

declare global {
  interface Window {
    __BULK_LUT_STUB__: StubBulkLutState;
  }
}
if (typeof window !== 'undefined') {
  window.__BULK_LUT_STUB__ = stubState;
}

const SAMPLE_WIDTH = 300;
const SAMPLE_HEIGHT = 200;
const sampleBuffer = new Uint8ClampedArray(SAMPLE_WIDTH * SAMPLE_HEIGHT * 4);
for (let i = 0; i < sampleBuffer.length; i += 4) {
  sampleBuffer[i] = 118;
  sampleBuffer[i + 1] = 180;
  sampleBuffer[i + 2] = 140;
  sampleBuffer[i + 3] = 255;
}

export class StubDesktopApiClient {
  async health(): Promise<{ status: string; version: string }> {
    return { status: 'ok', version: 'stub' };
  }

  async serverStatus(): Promise<ServerStatus> {
    return {
      reachable: true,
      base_url: 'http://localhost:8080',
      version: '0.1.0',
      detail: null,
    };
  }

  async roots(): Promise<string[]> {
    return ['/Volumes/Photos', '/Users/test/Pictures'];
  }

  async list(path: string): Promise<BrowserEntry[]> {
    return [
      { name: 'DCIM', is_dir: true, path: `${path}/DCIM` },
      { name: 'IMG_0001.JPG', is_dir: false, path: `${path}/IMG_0001.JPG` },
      { name: 'graded', is_dir: true, path: `${path}/graded` },
    ];
  }

  async listLuts(): Promise<LutLibraryList> {
    return {
      luts: [
        {
          name: 'Kodak Portra 400.cube',
          sha256: '7f83b1657ff1fc53b92dc18148a1d65dfc2d4b1fa3d677284addd200126d9069',
          format: 'Cube',
        },
        {
          name: 'Fuji Pro 400H.cube',
          sha256: 'a1b2c3d4e5f60718293a4b5c6d7e8f90123456789abcdef0123456789abcdef0',
          format: 'Cube',
        },
      ],
      errors: [],
    };
  }

  async importLut(path: string): Promise<LutEntry> {
    return {
      name: 'Custom.cube',
      sha256: 'fedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321',
      format: 'Cube',
    };
  }

  async planBulkLut(
    inputs: string[],
    lut: string,
    intensity: number,
    outDir: string,
    recursive?: boolean,
  ): Promise<BulkLutPlanSummary> {
    return {
      actions_count: 3,
      skipped: [
        { file: '/Volumes/Photos/sidecar.photoedit', reason: 'Edit sidecar — travels with its companion image' },
        { file: '/Volumes/Photos/.hidden.jpg', reason: 'Hidden file' },
      ],
      lut_sha256: stubState.lastPlanLutSha256,
      sample_frames: [
        '/Volumes/Photos/frame1.jpg',
        '/Volumes/Photos/frame2.jpg',
        '/Volumes/Photos/frame3.jpg',
      ],
    };
  }

  async openPreview(path: string): Promise<OpenPreviewResult> {
    if (stubState.failSamplePreviewPath && path.includes(stubState.failSamplePreviewPath)) {
      throw new Error(`Corrupted preview for ${path}`);
    }
    stubState.openSessions++;
    stubState.samplePreviewsRequested.push(path);
    if (stubState.openSessions > stubState.maxConcurrentSessions) {
      stubState.maxConcurrentSessions = stubState.openSessions;
    }
    // Simulate brief asynchronous session setup
    await new Promise((r) => setTimeout(r, 10));
    return {
      session_id: `session_${Date.now()}_${Math.random()}`,
      drag: [SAMPLE_WIDTH, SAMPLE_HEIGHT],
      settle: [SAMPLE_WIDTH, SAMPLE_HEIGHT],
      orientation: 1,
      read_only: false,
    };
  }

  async renderPreview(
    sessionId: string,
    recipe: AdjustmentRecipe,
    stage: PreviewStage = 'Settle',
  ): Promise<{ width: number; height: number; pixels: Uint8ClampedArray; orientation: number }> {
    // Simulate render timing
    await new Promise((r) => setTimeout(r, 15));
    return {
      width: SAMPLE_WIDTH,
      height: SAMPLE_HEIGHT,
      pixels: sampleBuffer,
      orientation: 1,
    };
  }

  async closePreview(sessionId: string): Promise<void> {
    await new Promise((r) => setTimeout(r, 5));
    stubState.openSessions = Math.max(0, stubState.openSessions - 1);
  }

  async applyBulkLut(
    inputs: string[],
    lut: string,
    intensity: number,
    outDir: string,
    reviewedLutSha256: string,
    recursive?: boolean,
  ): Promise<string> {
    stubState.lastApplyLutSha256 = reviewedLutSha256;

    if (stubState.failNextApplyWithLutChanged) {
      throw new Error(
        `The LUT file changed on disk since the reviewed plan (expected ${reviewedLutSha256}, found e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855); please run another dry run`,
      );
    }

    const jobId = `job_bulk_${Date.now()}`;
    return jobId;
  }

  async cancelJob(id: string): Promise<boolean> {
    if (stubState.failNextCancelWithThrow) {
      stubState.failNextCancelWithThrow = false;
      throw new Error('IPC cancel error');
    }
    if (stubState.failNextCancelWithFalse) {
      stubState.failNextCancelWithFalse = false;
      return false;
    }
    stubState.cancelledJobIds.push(id);
    // Broadcast cancelled state to active listeners
    for (const listener of stubState.activeJobListeners) {
      listener({
        id,
        kind: 'bulk_lut',
        state: 'cancelled',
        progress: 2,
        total: 3,
        message: '2 graded, 0 failed, 1 skipped',
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

    // Initial running event
    onEvent({
      id,
      kind: 'bulk_lut',
      state: 'running',
      progress: 1,
      total: 3,
      message: 'Grading photo 1/3: /Volumes/Photos/frame1.jpg',
      terminal: false,
    });

    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        if (!stubState.cancelledJobIds.includes(id)) {
          onEvent({
            id,
            kind: 'bulk_lut',
            state: 'completed',
            progress: 3,
            total: 3,
            message: '3 graded, 0 failed, 1 skipped',
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
      kind: 'bulk_lut',
      status: isCancelled ? 'cancelled' : 'completed',
      progress: isCancelled ? 2 : 3,
      total: 3,
      started_at: Date.now() - 1000,
      finished_at: Date.now(),
      error: null,
      summary: isCancelled ? '2 graded, 0 failed, 1 skipped' : '3 graded, 0 failed, 1 skipped',
    };
  }
}

export const api = new StubDesktopApiClient();
export const desktop = api;
