/**
 * Stub client for check:edit measurement harness.
 *
 * Implements the Tauri desktop API interface with simulated core rendering delays
 * matching ED-4 measured benchmarks (8.3 ms drag, 35.0 ms settle) and returns
 * appropriately sized RGBA frames (1280x854 drag, 2560x1708 settle).
 */
import type {
  AdjustmentRecipe,
  BrowserEntry,
  ExportResult,
  LutEntry,
  LutLibraryList,
  OpenPreviewResult,
  PreviewStage,
  ServerStatus,
} from '../../src/api';

export interface StubState {
  rendersIssued: number;
  renders: Array<{ recipe: AdjustmentRecipe; stage: PreviewStage; time: number }>;
  customDelay: number | null;
  orientation: number;
  readOnly: boolean;
  lastSavedRecipe: AdjustmentRecipe | null;
}

const stubState: StubState = {
  rendersIssued: 0,
  renders: [],
  customDelay: null,
  orientation: 1,
  readOnly: false,
  lastSavedRecipe: null,
};

// Expose state globally for test assertions
declare global {
  interface Window {
    __STUB__: StubState;
  }
}
if (typeof window !== 'undefined') {
  window.__STUB__ = stubState;
}

// Reusable frame buffers to avoid GC pressure during rapid benchmark scrubs
const DRAG_WIDTH = 1280;
const DRAG_HEIGHT = 854;
const SETTLE_WIDTH = 2560;
const SETTLE_HEIGHT = 1708;

const dragBuffer = new Uint8ClampedArray(DRAG_WIDTH * DRAG_HEIGHT * 4);
const settleBuffer = new Uint8ClampedArray(SETTLE_WIDTH * SETTLE_HEIGHT * 4);

// Fill with neutral grey background by default
for (let i = 0; i < dragBuffer.length; i += 4) {
  dragBuffer[i] = 118; // 18% neutral grey (~#767676)
  dragBuffer[i + 1] = 118;
  dragBuffer[i + 2] = 118;
  dragBuffer[i + 3] = 255;
}
for (let i = 0; i < settleBuffer.length; i += 4) {
  settleBuffer[i] = 118;
  settleBuffer[i + 1] = 118;
  settleBuffer[i + 2] = 118;
  settleBuffer[i + 3] = 255;
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
      { name: 'export', is_dir: true, path: `${path}/export` },
    ];
  }

  async openPreview(path: string): Promise<OpenPreviewResult> {
    return {
      session_id: 'stub_session_1',
      drag: [DRAG_WIDTH, DRAG_HEIGHT],
      settle: [SETTLE_WIDTH, SETTLE_HEIGHT],
      orientation: stubState.orientation,
      read_only: stubState.readOnly,
    };
  }

  async renderPreview(
    sessionId: string,
    recipe: AdjustmentRecipe,
    stage: PreviewStage = 'Drag',
  ): Promise<{ width: number; height: number; pixels: Uint8ClampedArray }> {
    stubState.rendersIssued += 1;
    stubState.renders.push({
      recipe: { ...recipe },
      stage,
      time: performance.now(),
    });

    // Simulated core delay: 8.3 ms for drag, 35.0 ms for settle (from ED-4 measurements)
    const delay =
      stubState.customDelay !== null
        ? stubState.customDelay
        : stage === 'Settle'
          ? 35.0
          : 8.3;

    if (delay > 0) {
      await new Promise((resolve) => setTimeout(resolve, delay));
    }

    const isSettle = stage === 'Settle';
    const width = isSettle ? SETTLE_WIDTH : DRAG_WIDTH;
    const height = isSettle ? SETTLE_HEIGHT : DRAG_HEIGHT;
    const buf = isSettle ? settleBuffer : dragBuffer;

    // Marked pixel at (0, 0) is RED (255, 0, 0, 255) for orientation corner testing
    buf[0] = 255;
    buf[1] = 0;
    buf[2] = 0;
    buf[3] = 255;

    // Encode exposure into pixel (1, 0) for test verification of latest-wins
    const exposureVal = Math.min(
      255,
      Math.max(0, Math.round(128 + (recipe.exposure ?? 0) * 20)),
    );
    buf[4] = exposureVal;
    buf[5] = 128;
    buf[6] = 128;
    buf[7] = 255;

    return { width, height, pixels: buf };
  }

  async closePreview(_sessionId: string): Promise<void> {}

  async loadRecipe(path: string): Promise<AdjustmentRecipe | null> {
    if (path.includes('v2_edits')) {
      return {
        version: 2,
        source_sha256: 'sha_v2',
        exposure: 0.5,
        temperature: -10,
        tint: 5,
        highlights: -15,
        shadows: 15,
        contrast: 10,
        saturation: 5,
        vibrance: -5,
        whites: 25,
        blacks: -35,
        brightness: 20,
        hue: 45,
        lut: null,
        lut_intensity: 1.0,
        curves: {
          luma: [
            { x: 0, y: 0 },
            { x: 0.25, y: 0.18 },
            { x: 0.75, y: 0.82 },
            { x: 1, y: 1 },
          ],
          red: [
            { x: 0, y: 0 },
            { x: 0.5, y: 0.55 },
            { x: 1, y: 1 },
          ],
          green: [
            { x: 0, y: 0 },
            { x: 1, y: 1 },
          ],
          blue: [
            { x: 0, y: 0 },
            { x: 0.5, y: 0.45 },
            { x: 1, y: 1 },
          ],
        },
      };
    }
    if (path.includes('existing_edits')) {
      return {
        version: 1,
        source_sha256: 'sha_existing',
        exposure: 1.5,
        temperature: -20,
        tint: 10,
        highlights: -30,
        shadows: 20,
        contrast: 15,
        saturation: 5,
        vibrance: 12,
        lut: null,
        lut_intensity: 1.0,
      };
    }
    if (path.includes('corrupted_sidecar')) {
      throw new Error('Syntax error: invalid JSON in sidecar');
    }
    return null;
  }

  async saveRecipe(path: string, recipe: AdjustmentRecipe): Promise<string> {
    if (stubState.readOnly) {
      throw new Error('Card media is read-only (G5). Copy files to a working folder to save edits.');
    }
    stubState.lastSavedRecipe = { ...recipe };
    return `${path}.photoedit`;
  }

  async exportEditedImage(
    path: string,
    _recipe: AdjustmentRecipe,
    outDir: string,
  ): Promise<ExportResult> {
    if (outDir.includes('Publishing')) {
      throw new Error('Refused: cannot export directly to Publishing folder (MV-16.7)');
    }
    if (outDir.includes('Card') || outDir.includes('DCIM')) {
      throw new Error('Card media is read-only (G5)');
    }
    return {
      path: `${outDir}/IMG_0001_edit.jpg`,
      metadata_skipped: null,
    };
  }

  async listLuts(): Promise<LutLibraryList> {
    return {
      luts: [
        { name: 'Provia_Standard', sha256: 'sha_provia_123', format: 'cube' },
        { name: 'Velvia_Vivid', sha256: 'sha_velvia_456', format: '3dl' },
        { name: 'Astia_Soft', sha256: 'sha_astia_789', format: 'png' },
      ],
      errors: [
        { name: 'corrupted_vintage.cube', error: 'Syntax error at line 42: unknown token' },
      ],
    };
  }

  async importLut(path: string): Promise<LutEntry> {
    const filename = path.split('/').pop() ?? 'imported.cube';
    return {
      name: filename,
      sha256: 'sha_imported_999',
      format: 'cube',
    };
  }
}

export const api = new StubDesktopApiClient();
export const desktop = api;
