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
  /** 'clipping' renders black and white bands either side of the grey, for ED-15. */
  pattern: 'grey' | 'clipping';
  /** The preset library, as core would hold it on disk (ED-17). */
  presets: Map<string, AdjustmentRecipe>;
  presetErrors: { name: string; error: string }[];
}

const stubState: StubState = {
  rendersIssued: 0,
  renders: [],
  customDelay: null,
  orientation: 1,
  readOnly: false,
  lastSavedRecipe: null,
  pattern: 'grey',
  presets: new Map(),
  presetErrors: [],
};

interface StubHistogram {
  red: Uint32Array;
  green: Uint32Array;
  blue: Uint32Array;
  luminance: Uint32Array;
  pixels: number;
  highlightClipped: number;
  shadowClipped: number;
}

/** The same counts `core::media::histogram` makes, including its integer Rec. 709 luma. */
function countHistogram(buf: Uint8ClampedArray, pixels: number): StubHistogram {
  const h: StubHistogram = {
    red: new Uint32Array(256),
    green: new Uint32Array(256),
    blue: new Uint32Array(256),
    luminance: new Uint32Array(256),
    pixels,
    highlightClipped: 0,
    shadowClipped: 0,
  };
  for (let i = 0; i < pixels * 4; i += 4) {
    const r = buf[i];
    const g = buf[i + 1];
    const b = buf[i + 2];
    h.red[r] += 1;
    h.green[g] += 1;
    h.blue[b] += 1;
    h.luminance[Math.floor((2126 * r + 7152 * g + 722 * b + 5000) / 10000)] += 1;
    if (r === 255 || g === 255 || b === 255) h.highlightClipped += 1;
    if (r === 0 || g === 0 || b === 0) h.shadowClipped += 1;
  }
  return h;
}

/**
 * Counts for a uniform frame of `fill` with the two marker pixels, worked out
 * rather than counted: counting 4.4 million pixels in the stub would add tens of
 * milliseconds to every settle frame the timing checks measure.
 */
function uniformHistogram(pixels: number, fill: number, markers: number[][]): StubHistogram {
  const one = new Uint8ClampedArray(4);
  const h = countHistogram(one, 0);
  h.pixels = pixels;
  const add = (r: number, g: number, b: number, n: number) => {
    h.red[r] += n;
    h.green[g] += n;
    h.blue[b] += n;
    h.luminance[Math.floor((2126 * r + 7152 * g + 722 * b + 5000) / 10000)] += n;
    if (r === 255 || g === 255 || b === 255) h.highlightClipped += n;
    if (r === 0 || g === 0 || b === 0) h.shadowClipped += n;
  };
  add(fill, fill, fill, pixels - markers.length);
  for (const [r, g, b] of markers) add(r, g, b, 1);
  return h;
}

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
      source_sha256: 'stub_source_sha256_e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
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
  ): Promise<{ width: number; height: number; pixels: Uint8ClampedArray; orientation: number }> {
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
    const isSwapped = [5, 6, 7, 8].includes(stubState.orientation);
    let baseW = isSettle ? SETTLE_WIDTH : DRAG_WIDTH;
    let baseH = isSettle ? SETTLE_HEIGHT : DRAG_HEIGHT;

    // In ED-13: Backend bakes orientation whenever geometry is present (not only when non-identity)
    const hasGeom = recipe.geometry !== null && recipe.geometry !== undefined;
    const exifSwaps = [5, 6, 7, 8].includes(stubState.orientation);
    const userSwaps = ((recipe.geometry?.rotate ?? 0) % 360 + 360) % 180 !== 0;
    const uprightSwapped = hasGeom ? (exifSwaps !== userSwaps) : false;

    if (uprightSwapped) {
      const tmp = baseW;
      baseW = baseH;
      baseH = tmp;
    }

    let width = baseW;
    let height = baseH;

    if (recipe.geometry?.crop) {
      width = Math.max(16, Math.round(baseW * recipe.geometry.crop.width));
      height = Math.max(16, Math.round(baseH * recipe.geometry.crop.height));
    }

    const pixelCount = width * height;
    let buf: Uint8ClampedArray;
    if (width === (isSettle ? SETTLE_WIDTH : DRAG_WIDTH) && height === (isSettle ? SETTLE_HEIGHT : DRAG_HEIGHT)) {
      buf = isSettle ? settleBuffer : dragBuffer;
    } else {
      buf = new Uint8ClampedArray(pixelCount * 4);
      buf.fill(128);
      for (let i = 3; i < buf.length; i += 4) {
        buf[i] = 255;
      }
    }

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

    let histogram: StubHistogram;
    if (stubState.pattern === 'clipping') {
      // Left quarter black, right quarter white, markers kept.
      const q = Math.floor(width / 4);
      for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x++) {
          if (y === 0 && x < 2) continue;
          const v = x < q ? 0 : x >= width - q ? 255 : 118;
          const i = (y * width + x) * 4;
          buf[i] = v;
          buf[i + 1] = v;
          buf[i + 2] = v;
        }
      }
      histogram = countHistogram(buf, pixelCount);
    } else {
      if (buf === settleBuffer || buf === dragBuffer) {
        // Undo a clipping pattern a previous frame left in a shared buffer.
        if (buf[(pixelCount - 1) * 4] !== 118) {
          for (let i = 8; i < buf.length; i += 4) {
            buf[i] = 118;
            buf[i + 1] = 118;
            buf[i + 2] = 118;
          }
        }
      }
      histogram = uniformHistogram(pixelCount, buf === settleBuffer || buf === dragBuffer ? 118 : 128, [
        [255, 0, 0],
        [exposureVal, 128, 128],
      ]);
    }

    const deliveredOrientation = hasGeom ? 1 : stubState.orientation;
    return { width, height, pixels: buf, orientation: deliveredOrientation, histogram };
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
        hsl: {
          red: { hue: 15, saturation: 20, luminance: -10 },
          orange: { hue: 0, saturation: 0, luminance: 0 },
          yellow: { hue: 0, saturation: 0, luminance: 0 },
          green: { hue: 0, saturation: 0, luminance: 0 },
          aqua: { hue: 0, saturation: 0, luminance: 0 },
          blue: { hue: -25, saturation: 40, luminance: 15 },
          purple: { hue: 0, saturation: 0, luminance: 0 },
          magenta: { hue: 0, saturation: 0, luminance: 0 },
        },
        grading: {
          shadows: { hue: 210, saturation: 0.35, luminance: -0.1 },
          midtones: { hue: 45, saturation: 0.2, luminance: 0.05 },
          highlights: { hue: 35, saturation: 0.4, luminance: 0.15 },
          global: { hue: 0, saturation: 0, luminance: 0 },
          blending: 60,
          balance: -15,
        },
        geometry: {
          crop: { x: 0.1, y: 0.1, width: 0.8, height: 0.6 },
          rotate: 90,
          straighten: 3.5,
          flip_h: false,
          flip_v: false,
          aspect: '4:3',
        },
        vignette: {
          amount: -45,
          midpoint: 40,
          roundness: 20,
          feather: 65,
        },
        grain: {
          amount: 35,
          size: 40,
          roughness: 60,
        },
        looks: {
          glow_amount: 30,
          glow_threshold: 65,
          glow_radius: 25,
          halation_amount: 20,
          halation_threshold: 75,
          halation_radius: 15,
          tone_mapper: 'aces',
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

  async listPresets(): Promise<{ presets: { name: string }[]; errors: { name: string; error: string }[] }> {
    return {
      presets: [...stubState.presets.keys()].sort().map((name) => ({ name })),
      errors: [...stubState.presetErrors],
    };
  }

  async loadPreset(name: string): Promise<AdjustmentRecipe> {
    const r = stubState.presets.get(name);
    if (!r) throw new Error(`No preset called ${name}.`);
    return JSON.parse(JSON.stringify(r));
  }

  async savePreset(name: string, recipe: AdjustmentRecipe, overwrite = false): Promise<void> {
    if (!name.trim()) throw new Error('A preset name cannot be empty.');
    if (name.includes('/')) throw new Error("A preset name cannot contain '/', '\\', ':', or control characters.");
    if (stubState.presets.has(name) && !overwrite) throw new Error(`A preset called ${name} already exists.`);
    // As core does: a preset keeps the look, not the photograph's hash or framing.
    const saved = { ...JSON.parse(JSON.stringify(recipe)), source_sha256: '', geometry: null };
    stubState.presets.set(name, saved);
  }

  async renamePreset(from: string, to: string): Promise<void> {
    const r = stubState.presets.get(from);
    if (!r) throw new Error(`No preset called ${from}.`);
    if (stubState.presets.has(to)) throw new Error(`A preset called ${to} already exists.`);
    stubState.presets.delete(from);
    stubState.presets.set(to, r);
  }

  async deletePreset(name: string): Promise<void> {
    if (!stubState.presets.delete(name)) throw new Error(`No preset called ${name}.`);
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
