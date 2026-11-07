# Built-in Looks Fit Report (DV-3)

Source: `bastibe/Darktable-Film-Simulation-Panel` commit `d6effd55cf37ce208c24cf8d072ac14028ca289b`

Thresholds: Mean ΔE2000 ≤ 2.5, Max unclipped ΔE2000 ≤ 7.0, Leave-one-out mean ΔE2000 ≤ 5.0.

| Look | Maker | ID | g | det M | Mean ΔE | Max unclipped ΔE | LOO Mean ΔE | LOO Max ΔE | Clipped patches | Status | Notes |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Provia | Fujifilm | `fujifilm-provia` | 3.277 | 0.933 | 1.67 | 4.39 | 2.35 | 5.31 | [6] | **PASS** | OK |
| Velvia | Fujifilm | `fujifilm-velvia` | 3.110 | 1.593 | 3.94 | 9.55 | 5.64 | 11.33 | [6, 8, 10, 12, 17] | **FAIL** | mean ΔE2000 3.94 exceeds threshold 2.5; max unclipped ΔE2000 9.55 exceeds threshold 7.0; leave-one-out mean ΔE2000 5.64 exceeds threshold 5.0 |
| Astia | Fujifilm | `fujifilm-astia` | 3.235 | 0.549 | 2.52 | 6.50 | 3.51 | 10.99 | [6, 11] | **FAIL** | mean ΔE2000 2.52 exceeds threshold 2.5 |
| Classic Chrome | Fujifilm | `fujifilm-classic-chrome` | 3.427 | 0.422 | 1.21 | 2.73 | 1.34 | 3.10 | [6] | **PASS** | OK |
| Pro Neg High | Fujifilm | `fujifilm-pro-neg-high` | 3.255 | 0.769 | 1.92 | 3.89 | 2.43 | 4.57 | none | **PASS** | OK |
| Pro Neg Std | Fujifilm | `fujifilm-pro-neg-std` | 3.277 | 0.657 | 1.26 | 2.44 | 1.65 | 3.03 | none | **PASS** | OK |
| Eterna | Fujifilm | `fujifilm-eterna` | 3.205 | 0.459 | 1.18 | 2.67 | 1.56 | 3.20 | none | **PASS** | OK |
| Standard | Ricoh | `ricoh-standard` | 1.206 | 0.998 | 0.95 | 2.11 | 1.46 | 3.00 | none | **PASS** | OK |
| Vivid | Ricoh | `ricoh-vivid` | 1.226 | 1.183 | 1.42 | 2.87 | 2.16 | 4.32 | [8, 12] | **PASS** | OK |
| Positive Film | Ricoh | `ricoh-positive-film` | 0.999 | 0.717 | 1.66 | 3.20 | 2.36 | 4.58 | none | **PASS** | OK |
| Negative Film | Ricoh | `ricoh-negative-film` | 1.477 | 0.515 | 1.09 | 2.47 | 1.57 | 3.36 | none | **PASS** | OK |
| Retro | Ricoh | `ricoh-retro` | 1.506 | 0.159 | 1.12 | 2.54 | 1.24 | 3.06 | none | **PASS** | OK |
| Bleach Bypass | Ricoh | `ricoh-bleach-bypass` | 2.716 | 0.115 | 1.45 | 2.62 | 3.52 | 12.46 | [5] | **PASS** | OK |
| Cross Processing | Ricoh | `ricoh-cross-processing` | 1.992 | 0.735 | 2.47 | 5.21 | 4.52 | 11.62 | [12, 15, 17] | **PASS** | OK |
