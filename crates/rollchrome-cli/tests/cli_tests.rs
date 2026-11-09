//! Integration tests for `rollchrome-cli` (RP-13).

#[allow(dead_code)]
#[path = "../../rollchrome-core/tests/support/dng.rs"]
mod dng;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

struct CliFixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    config_home: PathBuf,
    root: PathBuf,
    staging: PathBuf,
    db: PathBuf,
}

impl CliFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let config_home = temp.path().join("xdg_config");
        let root = temp.path().join("library");
        let staging = temp.path().join("staging");
        let db = temp.path().join("ledger.sqlite3");

        fs::create_dir_all(&home).expect("create home");
        fs::create_dir_all(&config_home).expect("create config_home");
        fs::create_dir_all(&root).expect("create root");
        fs::create_dir_all(&staging).expect("create staging");
        let root = root.canonicalize().expect("canonicalize root");

        Self {
            _temp: temp,
            home,
            config_home,
            root,
            staging,
            db,
        }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rollchrome-cli"));
        cmd.env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.config_home)
            .env("ROOTS", &self.root)
            .env("STAGING_DIR", &self.staging)
            .env("DATABASE_PATH", &self.db)
            .env_remove("PUBLISHING_DIR");
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command()
            .args(args)
            .output()
            .expect("spawn rollchrome-cli")
    }

    fn write_dng(&self, dir: &Path, name: &str, pixel_value: u16) -> PathBuf {
        let builder = dng::DngBuilder::new_bayer(16, 16);
        let pixels = vec![pixel_value; 16 * 16];
        let bytes = builder.build(&pixels);
        let path = dir.join(name);
        fs::write(&path, bytes).expect("write DNG");
        path
    }
}

#[test]
fn help_lists_every_command_and_exits_zero() {
    let f = CliFixture::new();
    let out = f.run(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    for sub in ["looks", "luts", "develop", "batch", "export", "mcp"] {
        assert!(
            stdout.contains(sub),
            "expected --help output to mention '{sub}', got:\n{stdout}"
        );
    }
}

#[test]
fn an_unknown_command_exits_two() {
    let f = CliFixture::new();
    let out = f.run(&["nosuchcommand"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn develop_without_apply_prints_the_plan_and_writes_nothing() {
    let f = CliFixture::new();
    f.write_dng(&f.root, "DSCF0001.dng", 2500);

    let folder = f.root.to_string_lossy().to_string();
    let out = f.run(&[
        "develop",
        &folder,
        "--look",
        "builtin:fujifilm-classic-chrome",
    ]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("DSCF0001.dng"));
    assert!(stdout.contains("DSCF0001.jpg"));
    assert!(!f.root.join("Developed").exists());
}

#[test]
fn develop_with_apply_writes_and_exits_zero() {
    let f = CliFixture::new();
    f.write_dng(&f.root, "DSCF0001.dng", 2500);

    let folder = f.root.to_string_lossy().to_string();
    let out = f.run(&[
        "develop",
        &folder,
        "--look",
        "builtin:fujifilm-classic-chrome",
        "--apply",
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let developed = f.root.join("Developed/DSCF0001.jpg");
    assert!(developed.is_file());
    assert!(fs::metadata(&developed).unwrap().len() > 0);
}

#[test]
fn a_failed_photograph_exits_one_and_names_it() {
    let f = CliFixture::new();
    fs::write(f.root.join("BROKEN01.dng"), b"not a valid raw file").unwrap();

    let folder = f.root.to_string_lossy().to_string();
    let out = f.run(&[
        "develop",
        &folder,
        "--look",
        "builtin:fujifilm-classic-chrome",
        "--apply",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("BROKEN01.dng"),
        "expected output to name BROKEN01.dng, got:\n{combined}"
    );
}

#[test]
fn json_flag_prints_one_json_document_on_stdout() {
    let f = CliFixture::new();
    f.write_dng(&f.root, "DSCF0001.dng", 2500);

    let folder = f.root.to_string_lossy().to_string();
    let out = f.run(&[
        "develop",
        &folder,
        "--look",
        "builtin:fujifilm-classic-chrome",
        "--apply",
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(0));

    let stdout_str = String::from_utf8(out.stdout).expect("valid utf-8 stdout");
    let docs: Vec<Result<Value, _>> = serde_json::Deserializer::from_str(&stdout_str)
        .into_iter::<Value>()
        .collect();
    assert_eq!(
        docs.len(),
        1,
        "expected exactly one JSON document on stdout, got: {stdout_str}"
    );
    let doc = docs
        .into_iter()
        .next()
        .unwrap()
        .expect("valid JSON document");
    assert_eq!(doc["developed"], json!(["DSCF0001.jpg"]));
}

#[test]
fn a_path_outside_the_roots_exits_three_with_the_fixed_message() {
    let f = CliFixture::new();
    let outside = f._temp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();

    let outside_str = outside.to_string_lossy().to_string();
    let out = f.run(&[
        "develop",
        &outside_str,
        "--look",
        "builtin:fujifilm-classic-chrome",
    ]);
    assert_eq!(out.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        stderr.trim(),
        "That path is outside the folders this application may touch."
    );
}

#[test]
fn mcp_subcommand_answers_initialize_over_real_pipes() {
    let f = CliFixture::new();
    let mut child = f
        .command()
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rollchrome-cli mcp");

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "cli-test", "version": "1.0" }
        }
    });
    {
        let stdin = child.stdin.as_mut().expect("child stdin");
        writeln!(stdin, "{req}").expect("write initialize");
    }
    let out = child.wait_with_output().expect("wait_with_output");
    assert_eq!(out.status.code(), Some(0));

    let stdout_str = String::from_utf8(out.stdout).expect("utf-8 stdout");
    let reply: Value = serde_json::from_str(stdout_str.trim()).expect("valid JSON-RPC reply");
    assert_eq!(reply["jsonrpc"], json!("2.0"));
    assert_eq!(reply["id"], json!(1));
    assert_eq!(reply["result"]["protocolVersion"], json!("2025-06-18"));
    assert_eq!(reply["result"]["serverInfo"]["name"], json!("rollchrome"));
}
