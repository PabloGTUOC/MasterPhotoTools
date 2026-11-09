# Rollchrome Automation: Command Line and MCP Server

`rollchrome-cli` exposes Rollchrome's non-destructive RAW development, batch 3D LUT grading, single-photograph sidecar export, and Model Context Protocol (MCP) server over the same `rollchrome-core` library used by the desktop application.

Every command enforces the same safety invariants as the desktop application:
- **Configured roots (G6):** Every input file, input folder, LUT path, and output directory is resolved against `Config` roots before any filesystem work runs. Any path outside the configured roots is refused with the fixed sentence `"That path is outside the folders this application may touch."` without revealing whether the path exists.
- **Card media read-only (G5):** Developing or writing outputs onto a volume with `DCIM` at its mount root is refused before any file or directory is created.
- **Never overwrite originals (G1):** `develop` writes exclusively into `<folder>/Developed/` and skips existing outputs; `batch` writes `<stem>_lut.<ext>` with numeric collision suffixes (`_1`, `_2`, ...); `export` uses the naming template (`{name}_edit` by default) with collision suffixes.
- **Dry-run by default:** `develop` and `batch` print the plan and its SHA-256 (`plan_sha256`) and write nothing unless `--apply` is passed.

---

## Configuration

`rollchrome-cli` loads configuration via `phototools_core::config::Config::load()`:
1. The platform configuration file (`~/Library/Application Support/masterphototools/config.json` on macOS, `~/.config/masterphototools/config.json` on Linux, `%APPDATA%\masterphototools\config.json` on Windows) if present.
2. Otherwise, environment variables (`ROOTS`, `STAGING_DIR`, `DATABASE_PATH`, `PUBLISHING_DIR`).

If `roots` is empty, `rollchrome-cli` refuses to run and prints the configuration file path so you can add at least one library root folder.

---

## Commands

```sh
rollchrome-cli looks [--json]
rollchrome-cli luts [--json]
rollchrome-cli develop <folder> --look <id> [--exposure <ev>] [--format jpeg|tiff] [--quality <1..100, default 92>] [--only <name>]... [--apply] [--json]
rollchrome-cli batch <input>... --lut <name|sha256|path> --out <dir> [--intensity <0..1, default 1>] [--recursive] [--apply] [--json]
rollchrome-cli export <photo> --out <dir> [--format as-source|jpeg|tiff|png|webp|avif] [--quality <n>] [--bits <8|16>] [--long-edge <n>|--megapixels <n>] [--sharpen off|screen|print] [--template <t>] [--metadata all|no-location|none] [--json]
rollchrome-cli mcp
rollchrome-cli --help
```

### `looks`
Lists all shipped built-in film looks (`builtin:<id>`, label, family, and description).

### `luts`
Lists all `.cube` 3D LUTs imported into the managed LUT library (`name`, `title`, `grid_size`, `sha256`), along with any unparseable files skipped.

### `develop`
Plans or executes RAW development for a folder inside the configured roots:
- `--look <id>`: A built-in look (`builtin:fujifilm-classic-chrome`, `builtin:ricoh-standard`, etc.) or a LUT name/SHA-256 in the LUT library.
- `--exposure <ev>`: Exposure compensation in EV (`-3.0` to `+3.0`, default `0.0`).
- `--format jpeg|tiff`: Output format (default `jpeg`).
- `--quality <1..100>`: JPEG quality when `--format jpeg` is used (default `92`).
- `--only <name>`: Restrict development to specific filenames in `<folder>` (repeatable).
- `--apply`: Execute the plan and write developed files into `<folder>/Developed/`. Without `--apply`, prints the dry-run plan and its `plan_sha256` and writes nothing.

### `batch`
Plans or executes batch 3D LUT grading on JPEG and TIFF files or folders:
- `<input>...`: One or more JPEG/TIFF files or folders inside the configured roots (RAW files are skipped with a reason).
- `--lut <name|sha256|path>`: A LUT name or SHA-256 from the managed LUT library, or a path to a `.cube` file inside the configured roots.
- `--out <dir>`: Output directory inside the configured roots.
- `--intensity <0..1>`: LUT blend intensity (default `1.0`).
- `--recursive`: Traverse input folders recursively.
- `--apply`: Execute the batch plan. Without `--apply`, prints the dry-run plan and its `plan_sha256` and writes nothing.

### `export`
Exports a single JPEG or TIFF photograph using its saved `.photoedit` sidecar recipe (or the identity recipe if no sidecar exists):
- `<photo>`: Path to the source JPEG or TIFF inside the configured roots (RAW files are refused; use `develop` first).
- `--out <dir>`: Output directory inside the configured roots.
- `--format as-source|jpeg|tiff|png|webp|avif`: Output format (default `as-source`).
- `--quality <n>`: Quality (`1..100`) for `jpeg`, `webp`, or `avif`.
- `--bits <8|16>`: Bit depth (`8` or `16`) for `tiff` or `png`.
- `--long-edge <n>` or `--megapixels <n>`: Downscale constraint (never upscales).
- `--sharpen off|screen|print`: Output sharpening preset (default `off`).
- `--template <t>`: Filename template using `{name}`, `{date}`, `{seq}` (default `{name}_edit`).
- `--metadata all|no-location|none`: EXIF metadata policy (default `all`).

### Output Formatting (`--json`) and Progress
- Progress updates during `--apply` runs are written to `stderr`.
- When `--json` is passed, `stdout` carries **exactly one** newline-terminated JSON document containing the structured result.

---

## Exit Codes

| Code | Meaning |
| :--- | :--- |
| `0` | Everything requested succeeded. |
| `1` | A `--apply` run completed with at least one failed photograph (the failed filenames and reasons are reported). |
| `2` | Bad command-line usage (unknown subcommand, missing required flag, invalid flag value). |
| `3` | Operation refused before or during execution (path outside configured roots, card volume read-only, RAW passed to `export`, missing LUT, or empty roots). |

---

## MCP Server (`rollchrome-cli mcp`)

`rollchrome-cli mcp` runs a newline-delimited JSON-RPC 2.0 Model Context Protocol server over `stdin` and `stdout` (supporting protocol versions `2025-06-18`, `2025-03-26`, and `2024-11-05`).

### Exposed Tools

1. `list_looks`: Lists all built-in film looks.
2. `list_luts`: Lists all 3D LUTs in the managed LUT library.
3. `plan_develop`: Computes a dry-run RAW development plan and returns `{ plan, plan_sha256 }`. Writes nothing.
4. `run_develop`: Executes a RAW development plan. Requires `plan_sha256` from `plan_develop`; re-computes the plan and refuses if the folder contents, existing outputs, or LUT changed.
5. `plan_batch`: Computes a dry-run batch 3D LUT plan and returns `{ plan, plan_sha256 }`. Writes nothing.
6. `run_batch`: Executes a batch 3D LUT plan. Requires `plan_sha256` from `plan_batch`; re-computes the plan and refuses if the inputs, outputs, or `.cube` file changed.
7. `export`: Exports a single JPEG or TIFF photograph using its `.photoedit` sidecar.

### Progress and Cancellation
- If a `tools/call` request includes `_meta.progressToken`, the server emits `notifications/progress` messages carrying that `progressToken`, `progress`, and `total`.
- Sending `notifications/cancelled` with `params.requestId` matching an in-flight `run_develop` or `run_batch` request stops the run cleanly after the current photograph finishes and returns the summary of photographs processed so far.

### Registering with an MCP Client

Add `rollchrome` to your MCP client configuration (for example, `claude_desktop_config.json` or `.mcp.json`). The server reads your existing Rollchrome / PhotoTools `config.json` and uses its configured folders:

```json
{
  "mcpServers": {
    "rollchrome": {
      "command": "/ABSOLUTE/PATH/TO/target/release/rollchrome-cli",
      "args": ["mcp"]
    }
  }
}
```

When no `config.json` exists on the machine, you can supply the paths via environment variables instead:

```json
{
  "mcpServers": {
    "rollchrome": {
      "command": "/ABSOLUTE/PATH/TO/target/release/rollchrome-cli",
      "args": ["mcp"],
      "env": {
        "ROOTS": "/Users/you/Pictures/RollchromeLibrary",
        "STAGING_DIR": "/Users/you/Pictures/RollchromeStaging",
        "DATABASE_PATH": "/Users/you/Pictures/RollchromeStaging/ledger.sqlite3"
      }
    }
  }
}
```
