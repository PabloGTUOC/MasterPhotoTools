## Encoder change — mozjpeg replaces the pure-Rust JPEG encoder

**Status:** complete. Three output specifications now met; the §9.1 performance target is closer but
**still missed**, and is reported rather than relaxed.

Raised as a G8 gate in the Phase 2 and Phase 4 reports and approved before implementation.

### Why

The `image` crate's JPEG encoder blocked four requirements at once:

| | Specified | With `image` |
|---|---|---|
| §9.1 | 24 MP resize + encode < 150 ms | 463 ms (encode alone 345 ms) |
| F4 step 5 | quality 95, **no chroma subsampling** | 4:2:2, fixed |
| F7 step 5 | quality 95, **no chroma subsampling** | 4:2:2, fixed |
| F8 | quality 90, **4:2:0**, **progressive** | 4:2:2 baseline, fixed |

The encoder offers no subsampling control and no progressive mode, so three of these were
unreachable by configuration.

### What changed

- **Added `mozjpeg` 0.10** (libjpeg-turbo with mozjpeg's extensions), plus `nasm` as a build-time
  requirement for its SIMD kernels. Verified AVX2 assembly objects are compiled in, not just the C
  fallback.
- **New `media/jpeg` module** wrapping it with the controls the specification actually names:
  `ChromaSubsampling` (4:4:4 / 4:2:2 / 4:2:0), `progressive`, `optimise`, and `Effort`.
- **Named profiles** so each tool declares its intent rather than repeating flags:
  - `deliverable(q)` — full chroma, optimised. F4, F5, F7.
  - `distributable(q)` — 4:2:0, progressive, optimised. F8.
  - `fast(q)` — minimal encoder work, for latency-bound paths.
- The C encoder uses `setjmp`/`longjmp` internally, so `encode` wraps it in `catch_unwind`: a
  malformed input returns an error rather than taking the process down.

### The `Effort` control, and why it exists

The first working version was **slower than what it replaced** — 851 ms against 463 ms. mozjpeg
defaults to `JCP_MAX_COMPRESSION`, which enables trellis quantisation: a second optimisation pass
over every block that `set_optimize_coding(false)` does not disable.

Measured on the 10 MP output of a 24 MP downscale:

| Profile | Time | Size |
|---|---|---|
| `Max`, q95, 4:4:4 | 621 ms | 126 KB |
| `Fast`, q95, 4:4:4 | **71 ms** | 289 KB |
| `Fast`, q95, 4:2:0 | 32 ms | 169 KB |
| `Max`, q90, 4:2:0 progressive | 481 ms | 35 KB |

Trellis is worth roughly 2.3× the file size for 8.7× the time. That is a good trade for a finished
print and a bad one for a derivative on a latency budget, so it is a choice rather than a default:
`Effort::Fast` calls `set_fastest_defaults()` (libjpeg-turbo's baseline profile), `Effort::Max`
keeps mozjpeg's. The generic `encode_jpeg_bytes` path uses `Fast`; the archive tools opt into `Max`
explicitly.

### Results

**Output specifications: all three met, and asserted from the files themselves.** Three new tests run
the tools and read the properties back with `exiftool`:

| | Specified | Produced |
|---|---|---|
| F4 | no chroma subsampling | `YCbCr4:4:4 (1 1)` |
| F7 | no chroma subsampling | `YCbCr4:4:4 (1 1)` |
| F8 | 4:2:0, progressive | `YCbCr4:2:0 (2 2)`, `Progressive DCT` |

**Performance: 463 ms → 203 ms. The 150 ms target is still missed by 53 ms.**

> **Later note (2026-08-20).** These figures stand as taken, on the Linux container this phase was
> built on. Re-measured in release on an Apple Silicon Mac the same operation is **97.8 ms**, inside
> the target — the hardware differs, nothing here was changed. See
> [`known-gaps.md`](../known-gaps.md#91--the-four-performance-targets-per-machine).

| Stage | Before | After |
|---|---|---|
| Resize 6000×4000 → 3872×2581 | 128 ms | 128 ms |
| JPEG encode | 345 ms | **71 ms** |
| Total | 463 ms | **203 ms** |

The encode is no longer the bottleneck — it is 4.9× faster and now the smaller half of the budget.
**The resize is,** at 128 ms of the 203 ms.

I measured the obvious lever and did not pull it. Changing the resize filter from Lanczos3:

| Filter | Time |
|---|---|
| Lanczos3 (current) | 128 ms |
| CatmullRom | 98 ms |
| Bilinear | 121 ms |

Bilinear being no faster than Lanczos3 says the resize is **memory-bandwidth-bound, not
filter-bound**. CatmullRom saves 30 ms and would still leave the total at ~170 ms, over target, in
exchange for a real quality reduction on every resized frame. That is not a trade worth making
silently, and it does not reach the target anyway, so Lanczos3 stands.

Worth noting: this is a shared cloud container. A 24 MP downscale is largely a memory-bandwidth
exercise, and the figure on the Mac this ingest path actually runs on may differ substantially. The
target should be re-measured there before anyone concludes it is unreachable.

Per build plan §11 the target is not relaxed. The benchmark asserts in release builds and currently
fails there; CI runs debug and is green.

### Acceptance

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — 183 passed, 0 failed
- [x] `cargo test -p phototools-core` — 177 passed, 0 failed
- [x] F4, F7, F8 output properties asserted from the encoded files

### Notes

- **`nasm` is now a build requirement**, added to CI. Phase 14's `Dockerfile` will need it in the
  build stage; the `distroless/cc` runtime stage is unaffected, since mozjpeg links statically.
- **`exiftool`'s `JPEGQualityEstimate` no longer matches the requested quality** — it reports 88 for
  q95 and 76 for q90. That is expected: mozjpeg uses different quantisation tables from IJG's, so
  the estimate is not comparable across encoders. The requested quality is what the code sets.
- **F6's `optimise` flag now has an effect**, where before it was accepted and ignored.
- The `image` crate is still used for decode, PNG/TIFF/WebP encode, and geometry. Only JPEG encoding
  moved.
