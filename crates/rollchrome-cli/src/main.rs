//! `rollchrome-cli` — command-line and MCP interface over `rollchrome-core` (RP-13).
//!
//! Transport only (G1): parses arguments, loads `Config`, delegates to
//! `rollchrome_core::automation::Automation`, formats output, and returns the
//! documented exit code.

use std::io::BufReader;
use std::process::ExitCode;

use phototools_core::config::Config;
use phototools_core::jobs::Progress;
use rollchrome_core::automation::{
    describe, mcp, Automation, BatchRequest, DevelopRequest, ExportRequest,
};
use rollchrome_core::media::develop::OutputFormat;
use rollchrome_core::tools::edit::{
    ExportFormat, ExportOptions, ExportResize, MetadataPolicy, OutputSharpening,
};

const HELP_TEXT: &str = "\
Usage:
  rollchrome-cli looks [--json]
  rollchrome-cli luts [--json]
  rollchrome-cli develop <folder> --look <id> [--exposure <ev>] [--format jpeg|tiff] [--quality <1..100, default 92>] [--only <name>]... [--apply] [--json]
  rollchrome-cli batch <input>... --lut <name|sha256|path> --out <dir> [--intensity <0..1, default 1>] [--recursive] [--apply] [--json]
  rollchrome-cli export <photo> --out <dir> [--format as-source|jpeg|tiff|png|webp|avif] [--quality <n>] [--bits <8|16>] [--long-edge <n>|--megapixels <n>] [--sharpen off|screen|print] [--template <t>] [--metadata all|no-location|none] [--json]
  rollchrome-cli mcp
  rollchrome-cli --help

Exit codes:
  0  Everything done
  1  A run finished with at least one failed photograph
  2  Bad usage
  3  Refused (a path, a card, a plan, a RAW file)
";

struct StderrProgress;

impl Progress for StderrProgress {
    fn report(&self, done: u64, total: u64, message: &str) {
        eprintln!("[{done}/{total}] {message}");
    }

    fn cancelled(&self) -> bool {
        false
    }
}

enum Command {
    Help,
    Looks {
        json: bool,
    },
    Luts {
        json: bool,
    },
    Develop {
        req: DevelopRequest,
        apply: bool,
        json: bool,
    },
    Batch {
        req: BatchRequest,
        apply: bool,
        json: bool,
    },
    Export {
        req: ExportRequest,
        json: bool,
    },
    Mcp,
}

fn parse_args(args: &[String]) -> Result<Command, String> {
    let Some(sub) = args.first().map(|s| s.as_str()) else {
        return Err("missing command (use --help for usage)".to_string());
    };

    if sub == "--help" || sub == "-h" || sub == "help" {
        return Ok(Command::Help);
    }

    let rest = &args[1..];
    if rest.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Command::Help);
    }

    match sub {
        "looks" => {
            let mut json = false;
            for arg in rest {
                match arg.as_str() {
                    "--json" => json = true,
                    other => return Err(format!("unknown option for looks: {other}")),
                }
            }
            Ok(Command::Looks { json })
        }
        "luts" => {
            let mut json = false;
            for arg in rest {
                match arg.as_str() {
                    "--json" => json = true,
                    other => return Err(format!("unknown option for luts: {other}")),
                }
            }
            Ok(Command::Luts { json })
        }
        "mcp" => {
            if !rest.is_empty() {
                return Err("mcp takes no extra arguments".to_string());
            }
            Ok(Command::Mcp)
        }
        "develop" => parse_develop(rest),
        "batch" => parse_batch(rest),
        "export" => parse_export(rest),
        other => Err(format!("unknown command '{other}' (use --help for usage)")),
    }
}

fn next_val<'a>(args: &'a [String], idx: &mut usize, flag: &str) -> Result<&'a str, String> {
    *idx += 1;
    args.get(*idx)
        .map(|s| s.as_str())
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn parse_develop(args: &[String]) -> Result<Command, String> {
    let mut folder: Option<String> = None;
    let mut look: Option<String> = None;
    let mut exposure_ev: f32 = 0.0;
    let mut format_str = "jpeg".to_string();
    let mut quality: u8 = 92;
    let mut only: Vec<String> = Vec::new();
    let mut apply = false;
    let mut json = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--look" => {
                look = Some(next_val(args, &mut i, "--look")?.to_string());
            }
            "--exposure" => {
                let raw = next_val(args, &mut i, "--exposure")?;
                exposure_ev = raw
                    .parse::<f32>()
                    .map_err(|_| format!("invalid --exposure value '{raw}'"))?;
            }
            "--format" => {
                format_str = next_val(args, &mut i, "--format")?.to_ascii_lowercase();
            }
            "--quality" => {
                let raw = next_val(args, &mut i, "--quality")?;
                let q = raw
                    .parse::<u8>()
                    .map_err(|_| format!("invalid --quality value '{raw}'"))?;
                if !(1..=100).contains(&q) {
                    return Err("--quality must be between 1 and 100".to_string());
                }
                quality = q;
            }
            "--only" => {
                only.push(next_val(args, &mut i, "--only")?.to_string());
            }
            "--apply" => apply = true,
            "--json" => json = true,
            s if s.starts_with('-') => {
                return Err(format!("unknown option for develop: {s}"));
            }
            pos => {
                if folder.is_some() {
                    return Err(format!("unexpected extra positional argument '{pos}'"));
                }
                folder = Some(pos.to_string());
            }
        }
        i += 1;
    }

    let Some(folder) = folder else {
        return Err("develop requires <folder>".to_string());
    };
    let Some(look) = look else {
        return Err("develop requires --look <id>".to_string());
    };

    let format = match format_str.as_str() {
        "jpeg" | "jpg" => OutputFormat::Jpeg { quality },
        "tiff" | "tif" => OutputFormat::Tiff,
        other => return Err(format!("--format must be jpeg or tiff (got '{other}')")),
    };

    Ok(Command::Develop {
        req: DevelopRequest {
            folder,
            look,
            exposure_ev,
            format,
            only,
        },
        apply,
        json,
    })
}

fn parse_batch(args: &[String]) -> Result<Command, String> {
    let mut inputs: Vec<String> = Vec::new();
    let mut lut: Option<String> = None;
    let mut out_dir: Option<String> = None;
    let mut intensity: f32 = 1.0;
    let mut recursive = false;
    let mut apply = false;
    let mut json = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--lut" => {
                lut = Some(next_val(args, &mut i, "--lut")?.to_string());
            }
            "--out" => {
                out_dir = Some(next_val(args, &mut i, "--out")?.to_string());
            }
            "--intensity" => {
                let raw = next_val(args, &mut i, "--intensity")?;
                let val = raw
                    .parse::<f32>()
                    .map_err(|_| format!("invalid --intensity value '{raw}'"))?;
                if !val.is_finite() || !(0.0..=1.0).contains(&val) {
                    return Err("--intensity must be between 0 and 1".to_string());
                }
                intensity = val;
            }
            "--recursive" => recursive = true,
            "--apply" => apply = true,
            "--json" => json = true,
            s if s.starts_with('-') => {
                return Err(format!("unknown option for batch: {s}"));
            }
            pos => {
                inputs.push(pos.to_string());
            }
        }
        i += 1;
    }

    if inputs.is_empty() {
        return Err("batch requires at least one <input>".to_string());
    }
    let Some(lut) = lut else {
        return Err("batch requires --lut <name|sha256|path>".to_string());
    };
    let Some(out_dir) = out_dir else {
        return Err("batch requires --out <dir>".to_string());
    };

    Ok(Command::Batch {
        req: BatchRequest {
            inputs,
            recursive,
            lut,
            intensity,
            out_dir,
        },
        apply,
        json,
    })
}

fn parse_export(args: &[String]) -> Result<Command, String> {
    let mut path: Option<String> = None;
    let mut out_dir: Option<String> = None;
    let mut format_str = "as-source".to_string();
    let mut quality_opt: Option<u8> = None;
    let mut bits_opt: Option<u8> = None;
    let mut long_edge_opt: Option<u32> = None;
    let mut megapixels_opt: Option<u32> = None;
    let mut sharpen_str = "off".to_string();
    let mut template_opt: Option<String> = None;
    let mut metadata_str = "all".to_string();
    let mut json = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--out" => {
                out_dir = Some(next_val(args, &mut i, "--out")?.to_string());
            }
            "--format" => {
                format_str = next_val(args, &mut i, "--format")?.to_ascii_lowercase();
            }
            "--quality" => {
                let raw = next_val(args, &mut i, "--quality")?;
                let q = raw
                    .parse::<u8>()
                    .map_err(|_| format!("invalid --quality value '{raw}'"))?;
                if !(1..=100).contains(&q) {
                    return Err("--quality must be between 1 and 100".to_string());
                }
                quality_opt = Some(q);
            }
            "--bits" => {
                let raw = next_val(args, &mut i, "--bits")?;
                let b = raw
                    .parse::<u8>()
                    .map_err(|_| format!("invalid --bits value '{raw}'"))?;
                if b != 8 && b != 16 {
                    return Err("--bits must be 8 or 16".to_string());
                }
                bits_opt = Some(b);
            }
            "--long-edge" => {
                let raw = next_val(args, &mut i, "--long-edge")?;
                let le = raw
                    .parse::<u32>()
                    .map_err(|_| format!("invalid --long-edge value '{raw}'"))?;
                if le == 0 {
                    return Err("--long-edge must be greater than zero".to_string());
                }
                long_edge_opt = Some(le);
            }
            "--megapixels" => {
                let raw = next_val(args, &mut i, "--megapixels")?;
                let mp = raw
                    .parse::<u32>()
                    .map_err(|_| format!("invalid --megapixels value '{raw}'"))?;
                if mp == 0 {
                    return Err("--megapixels must be greater than zero".to_string());
                }
                megapixels_opt = Some(mp);
            }
            "--sharpen" => {
                sharpen_str = next_val(args, &mut i, "--sharpen")?.to_ascii_lowercase();
            }
            "--template" => {
                template_opt = Some(next_val(args, &mut i, "--template")?.to_string());
            }
            "--metadata" => {
                metadata_str = next_val(args, &mut i, "--metadata")?.to_ascii_lowercase();
            }
            "--json" => json = true,
            s if s.starts_with('-') => {
                return Err(format!("unknown option for export: {s}"));
            }
            pos => {
                if path.is_some() {
                    return Err(format!("unexpected extra positional argument '{pos}'"));
                }
                path = Some(pos.to_string());
            }
        }
        i += 1;
    }

    let Some(path) = path else {
        return Err("export requires <photo>".to_string());
    };
    let Some(out_dir) = out_dir else {
        return Err("export requires --out <dir>".to_string());
    };

    if long_edge_opt.is_some() && megapixels_opt.is_some() {
        return Err("--long-edge and --megapixels cannot be used together".to_string());
    }

    let format = match format_str.as_str() {
        "as-source" => ExportFormat::AsSource,
        "jpeg" | "jpg" => ExportFormat::Jpeg {
            quality: quality_opt.unwrap_or(92),
        },
        "tiff" | "tif" => ExportFormat::Tiff {
            bits: bits_opt.unwrap_or(16),
        },
        "png" => ExportFormat::Png {
            bits: bits_opt.unwrap_or(8),
        },
        "webp" => ExportFormat::Webp,
        "avif" => ExportFormat::Avif {
            quality: quality_opt.unwrap_or(80),
        },
        other => {
            return Err(format!(
                "--format must be as-source, jpeg, tiff, png, webp, or avif (got '{other}')"
            ))
        }
    };

    let resize = match (long_edge_opt, megapixels_opt) {
        (Some(le), None) => ExportResize::LongEdge(le),
        (None, Some(mp)) => ExportResize::Megapixels(mp),
        _ => ExportResize::None,
    };

    let output_sharpening = match sharpen_str.as_str() {
        "off" => OutputSharpening::Off,
        "screen" => OutputSharpening::Screen,
        "print" => OutputSharpening::Print,
        other => {
            return Err(format!(
                "--sharpen must be off, screen, or print (got '{other}')"
            ))
        }
    };

    let metadata = match metadata_str.as_str() {
        "all" => MetadataPolicy::All,
        "no-location" => MetadataPolicy::NoLocation,
        "none" => MetadataPolicy::None,
        other => {
            return Err(format!(
                "--metadata must be all, no-location, or none (got '{other}')"
            ))
        }
    };

    Ok(Command::Export {
        req: ExportRequest {
            path,
            out_dir,
            options: ExportOptions {
                format,
                resize,
                output_sharpening,
                name_template: template_opt,
                metadata,
            },
        },
        json,
    })
}

fn print_json<T: serde::Serialize>(val: &T) -> ExitCode {
    match serde_json::to_string(val) {
        Ok(s) => {
            println!("{s}");
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("failed to serialize JSON output: {e}");
            ExitCode::from(1)
        }
    }
}

fn load_config_for(needs_roots: bool) -> Result<Config, ExitCode> {
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", describe(&e));
            return Err(ExitCode::from(3));
        }
    };
    if needs_roots && config.roots.is_empty() {
        eprintln!(
            "No library roots are configured in {}. That path is outside the folders this application may touch.",
            Config::config_path().display()
        );
        return Err(ExitCode::from(3));
    }
    Ok(config)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match parse_args(&args) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::from(2);
        }
    };

    match cmd {
        Command::Help => {
            print!("{HELP_TEXT}");
            ExitCode::from(0)
        }
        Command::Looks { json } => {
            let config = match load_config_for(false) {
                Ok(c) => c,
                Err(code) => return code,
            };
            let auto = Automation::new(config);
            let looks = auto.list_looks();
            if json {
                print_json(&looks)
            } else {
                for l in looks {
                    println!("{}\t{}\t{}", l.id, l.maker, l.name);
                }
                ExitCode::from(0)
            }
        }
        Command::Luts { json } => {
            let config = match load_config_for(false) {
                Ok(c) => c,
                Err(code) => return code,
            };
            let auto = Automation::new(config);
            match auto.list_luts() {
                Ok(list) => {
                    if json {
                        print_json(&list)
                    } else {
                        for l in &list.luts {
                            println!("{}\t{}\t{}", l.name, l.format, l.sha256);
                        }
                        for err in &list.errors {
                            eprintln!("invalid LUT {}: {}", err.name, err.error);
                        }
                        ExitCode::from(0)
                    }
                }
                Err(e) => {
                    eprintln!("{}", describe(&e));
                    ExitCode::from(3)
                }
            }
        }
        Command::Develop { req, apply, json } => {
            let config = match load_config_for(true) {
                Ok(c) => c,
                Err(code) => return code,
            };
            let auto = Automation::new(config);
            if !apply {
                match auto.plan_develop(&req) {
                    Ok(reviewed) => {
                        if json {
                            print_json(&reviewed)
                        } else {
                            println!("plan_sha256: {}", reviewed.plan_sha256);
                            for a in &reviewed.plan.actions {
                                println!(
                                    "plan: {} -> Developed/{} ({}, {:+.2} EV)",
                                    a.source, a.output, a.look, a.exposure_ev
                                );
                            }
                            for s in &reviewed.plan.skipped {
                                println!("skip: {} ({})", s.file, s.reason);
                            }
                            ExitCode::from(0)
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", describe(&e));
                        ExitCode::from(3)
                    }
                }
            } else {
                let progress = StderrProgress;
                match auto.run_develop(&req, None, &progress) {
                    Ok(summary) => {
                        for f in &summary.failures {
                            eprintln!("failed {}: {}", f.file, f.reason);
                        }
                        let code = if summary.failures.is_empty() { 0 } else { 1 };
                        if json {
                            let print_code = print_json(&summary);
                            if print_code != ExitCode::from(0) {
                                return print_code;
                            }
                        } else {
                            for out in &summary.developed {
                                println!("developed: Developed/{out}");
                            }
                            for s in &summary.skipped {
                                println!("skipped: {} ({})", s.file, s.reason);
                            }
                            for f in &summary.failures {
                                println!("failed: {} ({})", f.file, f.reason);
                            }
                        }
                        ExitCode::from(code)
                    }
                    Err(e) => {
                        eprintln!("{}", describe(&e));
                        ExitCode::from(3)
                    }
                }
            }
        }
        Command::Batch { req, apply, json } => {
            let config = match load_config_for(true) {
                Ok(c) => c,
                Err(code) => return code,
            };
            let auto = Automation::new(config);
            if !apply {
                match auto.plan_batch(&req) {
                    Ok(reviewed) => {
                        if json {
                            print_json(&reviewed)
                        } else {
                            println!("plan_sha256: {}", reviewed.plan_sha256);
                            for a in &reviewed.plan.actions {
                                println!(
                                    "plan: {} -> {} (lut {}, intensity {:.2})",
                                    a.source.display(),
                                    a.out_dir.display(),
                                    a.lut_path.display(),
                                    a.intensity
                                );
                            }
                            for s in &reviewed.plan.skipped {
                                println!("skip: {} ({})", s.file, s.reason);
                            }
                            ExitCode::from(0)
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", describe(&e));
                        ExitCode::from(3)
                    }
                }
            } else {
                let progress = StderrProgress;
                match auto.run_batch(&req, None, &progress) {
                    Ok(summary) => {
                        for (file, reason) in &summary.failures {
                            eprintln!("failed {}: {}", file.display(), reason);
                        }
                        let code = if summary.failures.is_empty() { 0 } else { 1 };
                        if json {
                            let print_code = print_json(&summary);
                            if print_code != ExitCode::from(0) {
                                return print_code;
                            }
                        } else {
                            for out in &summary.written {
                                println!("written: {}", out.display());
                            }
                            for s in &summary.skipped {
                                println!("skipped: {} ({})", s.file, s.reason);
                            }
                            for (file, reason) in &summary.failures {
                                println!("failed: {} ({})", file.display(), reason);
                            }
                        }
                        ExitCode::from(code)
                    }
                    Err(e) => {
                        eprintln!("{}", describe(&e));
                        ExitCode::from(3)
                    }
                }
            }
        }
        Command::Export { req, json } => {
            let config = match load_config_for(true) {
                Ok(c) => c,
                Err(code) => return code,
            };
            let auto = Automation::new(config);
            match auto.export(&req) {
                Ok(res) => {
                    if json {
                        print_json(&res)
                    } else {
                        println!("{}", res.path.display());
                        ExitCode::from(0)
                    }
                }
                Err(e) => {
                    eprintln!("{}", describe(&e));
                    ExitCode::from(3)
                }
            }
        }
        Command::Mcp => {
            let config = match load_config_for(false) {
                Ok(c) => c,
                Err(code) => return code,
            };
            if config.roots.is_empty() {
                eprintln!(
                    "Warning: no library roots are configured in {}.",
                    Config::config_path().display()
                );
            }
            let auto = Automation::new(config);
            let stdin = BufReader::new(std::io::stdin());
            let stdout = std::io::stdout();
            match mcp::serve(stdin, stdout, auto) {
                Ok(()) => ExitCode::from(0),
                Err(e) => {
                    eprintln!("{}", describe(&e));
                    ExitCode::from(1)
                }
            }
        }
    }
}
