//! Model Context Protocol (MCP) server over newline-delimited JSON-RPC 2.0 (RP-13).
//!
//! Speaks over arbitrary `BufRead` / `Write` streams (stdin/stdout in production,
//! in-memory pipes in tests). One reader thread parses incoming lines and handles
//! `notifications/cancelled` immediately; one worker thread executes requests sequentially.

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use phototools_core::error::Error;
use phototools_core::jobs::Progress;
use serde_json::{json, Map, Value};

use super::{describe, Automation, BatchRequest, DevelopRequest, ExportRequest};
use crate::media::develop::OutputFormat;
use crate::tools::edit::{
    ExportFormat, ExportOptions, ExportResize, MetadataPolicy, OutputSharpening,
};

/// Supported MCP protocol versions, newest first.
pub const SUPPORTED_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

struct CancelState {
    active_id: Option<Value>,
    active_flag: Option<Arc<AtomicBool>>,
    pre_cancelled: Vec<Value>,
    input_closed: bool,
}

impl CancelState {
    fn new() -> Self {
        Self {
            active_id: None,
            active_flag: None,
            pre_cancelled: Vec::new(),
            input_closed: false,
        }
    }

    fn begin(&mut self, id: Value) -> Arc<AtomicBool> {
        let was_pre_cancelled = if let Some(pos) = self.pre_cancelled.iter().position(|x| x == &id)
        {
            self.pre_cancelled.remove(pos);
            true
        } else {
            false
        };
        let flag = Arc::new(AtomicBool::new(was_pre_cancelled || self.input_closed));
        self.active_id = Some(id);
        self.active_flag = Some(Arc::clone(&flag));
        flag
    }

    fn finish(&mut self) {
        self.active_id = None;
        self.active_flag = None;
    }

    fn cancel(&mut self, id: &Value) {
        if self.active_id.as_ref() == Some(id) {
            if let Some(flag) = &self.active_flag {
                flag.store(true, Ordering::Relaxed);
            }
        } else if !self.pre_cancelled.contains(id) {
            self.pre_cancelled.push(id.clone());
        }
    }

    fn close_input(&mut self) {
        self.input_closed = true;
        if let Some(flag) = &self.active_flag {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

struct McpProgress<W: Write + Send + 'static> {
    out: Arc<Mutex<W>>,
    token: Option<Value>,
    cancelled: Arc<AtomicBool>,
}

impl<W: Write + Send + 'static> Progress for McpProgress<W> {
    fn report(&self, done: u64, total: u64, message: &str) {
        if let Some(token) = &self.token {
            let note = json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": {
                    "progressToken": token,
                    "progress": done,
                    "total": total,
                    "message": message,
                }
            });
            if write_json_line(&self.out, &note).is_err() {
                self.cancelled.store(true, Ordering::Relaxed);
            }
        }
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

fn write_json_line<W: Write>(out: &Arc<Mutex<W>>, val: &Value) -> Result<(), Error> {
    let line = serde_json::to_string(val)
        .map_err(|e| Error::Internal(format!("failed to serialize JSON-RPC message: {e}")))?;
    let mut guard = out.lock().unwrap_or_else(|e| e.into_inner());
    guard.write_all(line.as_bytes()).map_err(Error::Io)?;
    guard.write_all(b"\n").map_err(Error::Io)?;
    guard.flush().map_err(Error::Io)?;
    Ok(())
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn rpc_error(id: Value, code: i32, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
        }
    })
}

fn tool_ok(val: Value) -> Value {
    let text = val.to_string();
    json!({
        "content": [{
            "type": "text",
            "text": text,
        }],
        "structuredContent": val,
        "isError": false,
    })
}

fn tool_err(message: impl Into<String>) -> Value {
    let msg = message.into();
    json!({
        "content": [{
            "type": "text",
            "text": msg,
        }],
        "isError": true,
    })
}

fn tools_schema_list() -> Value {
    json!({
        "tools": [
            {
                "name": "list_looks",
                "description": "List all shipped built-in film looks for RAW development.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "list_luts",
                "description": "List all 3D LUTs in the managed LUT library.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "plan_develop",
                "description": "Dry-run planning for developing a folder of RAW files into Developed/. Writes nothing and returns the plan and plan_sha256.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "folder": { "type": "string", "description": "Path to the folder of RAW files" },
                        "look": { "type": "string", "description": "Look identifier (e.g. builtin:fujifilm-classic-chrome or LUT library name)" },
                        "exposure_ev": { "type": "number", "description": "Exposure adjustment in stops (-4.0 to 4.0, default 0.0)" },
                        "format": { "type": "string", "enum": ["jpeg", "tiff"], "description": "Output format (jpeg or tiff, default jpeg)" },
                        "quality": { "type": "integer", "minimum": 1, "maximum": 100, "description": "JPEG quality (1..100, default 92)" },
                        "only": { "type": "array", "items": { "type": "string" }, "description": "Optional list of RAW filenames in the folder to develop" }
                    },
                    "required": ["folder", "look"]
                }
            },
            {
                "name": "run_develop",
                "description": "Execute a reviewed RAW development plan. Requires plan_sha256 from plan_develop.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "folder": { "type": "string", "description": "Path to the folder of RAW files" },
                        "look": { "type": "string", "description": "Look identifier (e.g. builtin:fujifilm-classic-chrome or LUT library name)" },
                        "exposure_ev": { "type": "number", "description": "Exposure adjustment in stops (-4.0 to 4.0, default 0.0)" },
                        "format": { "type": "string", "enum": ["jpeg", "tiff"], "description": "Output format (jpeg or tiff, default jpeg)" },
                        "quality": { "type": "integer", "minimum": 1, "maximum": 100, "description": "JPEG quality (1..100, default 92)" },
                        "only": { "type": "array", "items": { "type": "string" }, "description": "Optional list of RAW filenames in the folder to develop" },
                        "plan_sha256": { "type": "string", "description": "SHA-256 hash of the reviewed plan from plan_develop" }
                    },
                    "required": ["folder", "look", "plan_sha256"]
                }
            },
            {
                "name": "plan_batch",
                "description": "Dry-run planning for batch 3D LUT grading of JPEG and TIFF files. Writes nothing and returns the plan and plan_sha256.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "inputs": { "type": "array", "items": { "type": "string" }, "description": "Input file or folder paths" },
                        "recursive": { "type": "boolean", "description": "Whether to scan input folders recursively (default false)" },
                        "lut": { "type": "string", "description": "LUT library name, SHA-256, or path to a .cube file" },
                        "intensity": { "type": "number", "minimum": 0.0, "maximum": 1.0, "description": "LUT intensity (0.0..1.0, default 1.0)" },
                        "out_dir": { "type": "string", "description": "Output directory path" }
                    },
                    "required": ["inputs", "lut", "out_dir"]
                }
            },
            {
                "name": "run_batch",
                "description": "Execute a reviewed batch 3D LUT grading plan. Requires plan_sha256 from plan_batch.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "inputs": { "type": "array", "items": { "type": "string" }, "description": "Input file or folder paths" },
                        "recursive": { "type": "boolean", "description": "Whether to scan input folders recursively (default false)" },
                        "lut": { "type": "string", "description": "LUT library name, SHA-256, or path to a .cube file" },
                        "intensity": { "type": "number", "minimum": 0.0, "maximum": 1.0, "description": "LUT intensity (0.0..1.0, default 1.0)" },
                        "out_dir": { "type": "string", "description": "Output directory path" },
                        "plan_sha256": { "type": "string", "description": "SHA-256 hash of the reviewed plan from plan_batch" }
                    },
                    "required": ["inputs", "lut", "out_dir", "plan_sha256"]
                }
            },
            {
                "name": "export",
                "description": "Export a single JPEG or TIFF photograph using its saved .photoedit sidecar (or default recipe if none).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the source JPEG or TIFF photograph" },
                        "out_dir": { "type": "string", "description": "Output directory path" },
                        "format": { "type": "string", "enum": ["as-source", "jpeg", "tiff", "png", "webp", "avif"] },
                        "quality": { "type": "integer", "minimum": 1, "maximum": 100 },
                        "bits": { "type": "integer", "enum": [8, 16] },
                        "long_edge": { "type": "integer", "minimum": 1 },
                        "megapixels": { "type": "integer", "minimum": 1 },
                        "sharpen": { "type": "string", "enum": ["off", "screen", "print"] },
                        "template": { "type": "string" },
                        "metadata": { "type": "string", "enum": ["all", "no-location", "none"] }
                    },
                    "required": ["path", "out_dir"]
                }
            }
        ]
    })
}

fn req_string(args: &Map<String, Value>, name: &str) -> Result<String, String> {
    match args.get(name) {
        None | Some(Value::Null) => Err(format!("missing required argument '{name}'")),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(format!("argument '{name}' must be a string")),
    }
}

fn opt_f32(args: &Map<String, Value>, name: &str, default: f32) -> Result<f32, String> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Number(n)) => n
            .as_f64()
            .map(|v| v as f32)
            .ok_or_else(|| format!("argument '{name}' must be a finite number")),
        Some(_) => Err(format!("argument '{name}' must be a number")),
    }
}

fn opt_u8_range(
    args: &Map<String, Value>,
    name: &str,
    min: u8,
    max: u8,
) -> Result<Option<u8>, String> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            let Some(v) = n.as_u64() else {
                return Err(format!(
                    "argument '{name}' must be an integer between {min} and {max}"
                ));
            };
            if v < min as u64 || v > max as u64 {
                return Err(format!("argument '{name}' must be between {min} and {max}"));
            }
            Ok(Some(v as u8))
        }
        Some(_) => Err(format!("argument '{name}' must be an integer")),
    }
}

fn opt_u32_min(args: &Map<String, Value>, name: &str, min: u32) -> Result<Option<u32>, String> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            let Some(v) = n.as_u64() else {
                return Err(format!("argument '{name}' must be a positive integer"));
            };
            if v < min as u64 || v > u32::MAX as u64 {
                return Err(format!("argument '{name}' must be at least {min}"));
            }
            Ok(Some(v as u32))
        }
        Some(_) => Err(format!("argument '{name}' must be an integer")),
    }
}

fn opt_bool(args: &Map<String, Value>, name: &str, default: bool) -> Result<bool, String> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("argument '{name}' must be a boolean")),
    }
}

fn opt_string_array(args: &Map<String, Value>, name: &str) -> Result<Vec<String>, String> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                match item {
                    Value::String(s) => out.push(s.clone()),
                    _ => return Err(format!("argument '{name}' must be an array of strings")),
                }
            }
            Ok(out)
        }
        Some(_) => Err(format!("argument '{name}' must be an array of strings")),
    }
}

fn req_string_array(args: &Map<String, Value>, name: &str) -> Result<Vec<String>, String> {
    match args.get(name) {
        None | Some(Value::Null) => Err(format!("missing required argument '{name}'")),
        Some(Value::Array(arr)) => {
            if arr.is_empty() {
                return Err(format!("argument '{name}' must not be empty"));
            }
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                match item {
                    Value::String(s) => out.push(s.clone()),
                    _ => return Err(format!("argument '{name}' must be an array of strings")),
                }
            }
            Ok(out)
        }
        Some(_) => Err(format!("argument '{name}' must be an array of strings")),
    }
}

fn parse_develop_request(args: &Map<String, Value>) -> Result<DevelopRequest, String> {
    let folder = req_string(args, "folder")?;
    let look = req_string(args, "look")?;
    let exposure_ev = opt_f32(args, "exposure_ev", 0.0)?;
    let quality = opt_u8_range(args, "quality", 1, 100)?.unwrap_or(92);
    let only = opt_string_array(args, "only")?;

    let format = match args.get("format") {
        None | Some(Value::Null) => OutputFormat::Jpeg { quality },
        Some(Value::String(s)) => match s.to_ascii_lowercase().as_str() {
            "jpeg" | "jpg" => OutputFormat::Jpeg { quality },
            "tiff" | "tif" => OutputFormat::Tiff,
            _ => return Err("argument 'format' must be 'jpeg' or 'tiff'".to_string()),
        },
        Some(_) => return Err("argument 'format' must be a string".to_string()),
    };

    Ok(DevelopRequest {
        folder,
        look,
        exposure_ev,
        format,
        only,
    })
}

fn parse_batch_request(args: &Map<String, Value>) -> Result<BatchRequest, String> {
    let inputs = req_string_array(args, "inputs")?;
    let lut = req_string(args, "lut")?;
    let out_dir = req_string(args, "out_dir")?;
    let recursive = opt_bool(args, "recursive", false)?;
    let intensity = opt_f32(args, "intensity", 1.0)?;
    if !intensity.is_finite() || !(0.0..=1.0).contains(&intensity) {
        return Err("argument 'intensity' must be between 0.0 and 1.0".to_string());
    }

    Ok(BatchRequest {
        inputs,
        recursive,
        lut,
        intensity,
        out_dir,
    })
}

fn parse_export_request(args: &Map<String, Value>) -> Result<ExportRequest, String> {
    let path = req_string(args, "path")?;
    let out_dir = req_string(args, "out_dir")?;

    let quality_opt = opt_u8_range(args, "quality", 1, 100)?;
    let bits_opt = match args.get("bits") {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => match n.as_u64() {
            Some(8) => Some(8u8),
            Some(16) => Some(16u8),
            _ => return Err("argument 'bits' must be 8 or 16".to_string()),
        },
        Some(_) => return Err("argument 'bits' must be 8 or 16".to_string()),
    };

    let format = match args.get("format") {
        None | Some(Value::Null) => ExportFormat::AsSource,
        Some(Value::String(s)) => match s.to_ascii_lowercase().as_str() {
            "as-source" | "assource" => ExportFormat::AsSource,
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
            _ => {
                return Err(
                    "argument 'format' must be one of as-source, jpeg, tiff, png, webp, avif"
                        .to_string(),
                )
            }
        },
        Some(_) => return Err("argument 'format' must be a string".to_string()),
    };

    let long_edge = opt_u32_min(args, "long_edge", 1)?;
    let megapixels = opt_u32_min(args, "megapixels", 1)?;
    let resize = match (long_edge, megapixels) {
        (Some(_), Some(_)) => {
            return Err("arguments 'long_edge' and 'megapixels' are mutually exclusive".to_string())
        }
        (Some(le), None) => ExportResize::LongEdge(le),
        (None, Some(mp)) => ExportResize::Megapixels(mp),
        (None, None) => ExportResize::None,
    };

    let output_sharpening = match args.get("sharpen") {
        None | Some(Value::Null) => OutputSharpening::Off,
        Some(Value::String(s)) => match s.to_ascii_lowercase().as_str() {
            "off" => OutputSharpening::Off,
            "screen" => OutputSharpening::Screen,
            "print" => OutputSharpening::Print,
            _ => return Err("argument 'sharpen' must be 'off', 'screen', or 'print'".to_string()),
        },
        Some(_) => return Err("argument 'sharpen' must be a string".to_string()),
    };

    let name_template = match args.get("template") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err("argument 'template' must be a string".to_string()),
    };

    let metadata = match args.get("metadata") {
        None | Some(Value::Null) => MetadataPolicy::All,
        Some(Value::String(s)) => match s.to_ascii_lowercase().as_str() {
            "all" => MetadataPolicy::All,
            "no-location" | "nolocation" => MetadataPolicy::NoLocation,
            "none" => MetadataPolicy::None,
            _ => {
                return Err(
                    "argument 'metadata' must be 'all', 'no-location', or 'none'".to_string(),
                )
            }
        },
        Some(_) => return Err("argument 'metadata' must be a string".to_string()),
    };

    Ok(ExportRequest {
        path,
        out_dir,
        options: ExportOptions {
            format,
            resize,
            output_sharpening,
            name_template,
            metadata,
        },
    })
}

fn extract_progress_token(req_obj: &Map<String, Value>) -> Option<Value> {
    req_obj
        .get("params")
        .and_then(|v| v.as_object())
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.as_object())
        .and_then(|m| m.get("progressToken"))
        .filter(|v| !v.is_null())
        .cloned()
}

fn handle_tool_call<W: Write + Send + 'static>(
    automation: &Automation,
    tool_name: &str,
    args: &Map<String, Value>,
    progress: &McpProgress<W>,
) -> Value {
    match tool_name {
        "list_looks" => {
            let looks = automation.list_looks();
            match serde_json::to_value(looks) {
                Ok(val) => tool_ok(val),
                Err(e) => tool_err(e.to_string()),
            }
        }
        "list_luts" => match automation.list_luts() {
            Ok(list) => match serde_json::to_value(list) {
                Ok(val) => tool_ok(val),
                Err(e) => tool_err(e.to_string()),
            },
            Err(e) => tool_err(describe(&e)),
        },
        "plan_develop" => {
            let req = match parse_develop_request(args) {
                Ok(r) => r,
                Err(msg) => return tool_err(msg),
            };
            match automation.plan_develop(&req) {
                Ok(reviewed) => match serde_json::to_value(reviewed) {
                    Ok(val) => tool_ok(val),
                    Err(e) => tool_err(e.to_string()),
                },
                Err(e) => tool_err(describe(&e)),
            }
        }
        "run_develop" => {
            let plan_sha256 = match req_string(args, "plan_sha256") {
                Ok(s) if !s.trim().is_empty() => s,
                Ok(_) => return tool_err("argument 'plan_sha256' must not be empty"),
                Err(msg) => return tool_err(msg),
            };
            let req = match parse_develop_request(args) {
                Ok(r) => r,
                Err(msg) => return tool_err(msg),
            };
            match automation.run_develop(&req, Some(&plan_sha256), progress) {
                Ok(summary) => match serde_json::to_value(summary) {
                    Ok(val) => tool_ok(val),
                    Err(e) => tool_err(e.to_string()),
                },
                Err(e) => tool_err(describe(&e)),
            }
        }
        "plan_batch" => {
            let req = match parse_batch_request(args) {
                Ok(r) => r,
                Err(msg) => return tool_err(msg),
            };
            match automation.plan_batch(&req) {
                Ok(reviewed) => match serde_json::to_value(reviewed) {
                    Ok(val) => tool_ok(val),
                    Err(e) => tool_err(e.to_string()),
                },
                Err(e) => tool_err(describe(&e)),
            }
        }
        "run_batch" => {
            let plan_sha256 = match req_string(args, "plan_sha256") {
                Ok(s) if !s.trim().is_empty() => s,
                Ok(_) => return tool_err("argument 'plan_sha256' must not be empty"),
                Err(msg) => return tool_err(msg),
            };
            let req = match parse_batch_request(args) {
                Ok(r) => r,
                Err(msg) => return tool_err(msg),
            };
            match automation.run_batch(&req, Some(&plan_sha256), progress) {
                Ok(summary) => match serde_json::to_value(summary) {
                    Ok(val) => tool_ok(val),
                    Err(e) => tool_err(e.to_string()),
                },
                Err(e) => tool_err(describe(&e)),
            }
        }
        "export" => {
            let req = match parse_export_request(args) {
                Ok(r) => r,
                Err(msg) => return tool_err(msg),
            };
            match automation.export(&req) {
                Ok(res) => match serde_json::to_value(res) {
                    Ok(val) => tool_ok(val),
                    Err(e) => tool_err(e.to_string()),
                },
                Err(e) => tool_err(describe(&e)),
            }
        }
        other => tool_err(format!("unknown tool '{other}'")),
    }
}

const KNOWN_TOOLS: [&str; 7] = [
    "list_looks",
    "list_luts",
    "plan_develop",
    "run_develop",
    "plan_batch",
    "run_batch",
    "export",
];

struct ToolCall {
    id: Value,
    obj: Map<String, Value>,
}

/// Runs the MCP server over `input` and `output` until `input` reaches EOF.
pub fn serve(
    input: impl BufRead + Send + 'static,
    output: impl Write + Send + 'static,
    automation: Automation,
) -> Result<(), Error> {
    let out = Arc::new(Mutex::new(output));
    let cancel_state = Arc::new(Mutex::new(CancelState::new()));
    let (tx, rx) = mpsc::channel::<ToolCall>();

    let worker_out = Arc::clone(&out);
    let worker_cancel = Arc::clone(&cancel_state);

    let worker = std::thread::Builder::new()
        .name("mcp-worker".to_string())
        .spawn(move || -> Result<(), Error> {
            for ToolCall { id, obj } in rx {
                let params_opt = obj.get("params").and_then(|p| p.as_object());
                let tool_name = params_opt
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str());

                let Some(name) = tool_name else {
                    let reply = rpc_error(id, -32602, "Invalid params: missing tool name");
                    write_json_line(&worker_out, &reply)?;
                    continue;
                };

                if !KNOWN_TOOLS.contains(&name) {
                    let reply =
                        rpc_error(id, -32602, format!("Invalid params: unknown tool '{name}'"));
                    write_json_line(&worker_out, &reply)?;
                    continue;
                }

                let empty_map = Map::new();
                let args = match params_opt.and_then(|p| p.get("arguments")) {
                    None | Some(Value::Null) => &empty_map,
                    Some(Value::Object(m)) => m,
                    Some(_) => {
                        let reply =
                            rpc_result(id, tool_err("argument 'arguments' must be an object"));
                        write_json_line(&worker_out, &reply)?;
                        continue;
                    }
                };

                let flag = {
                    let mut guard = worker_cancel.lock().unwrap_or_else(|e| e.into_inner());
                    guard.begin(id.clone())
                };

                let progress = McpProgress {
                    out: Arc::clone(&worker_out),
                    token: extract_progress_token(&obj),
                    cancelled: flag,
                };

                let call_result = handle_tool_call(&automation, name, args, &progress);

                {
                    let mut guard = worker_cancel.lock().unwrap_or_else(|e| e.into_inner());
                    guard.finish();
                }

                let reply = rpc_result(id, call_result);
                write_json_line(&worker_out, &reply)?;
            }
            Ok(())
        })
        .map_err(|e| Error::Internal(format!("failed to spawn MCP worker thread: {e}")))?;

    for line_res in input.lines() {
        let line = line_res.map_err(Error::Io)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                write_json_line(&out, &rpc_error(Value::Null, -32700, "Parse error"))?;
                continue;
            }
        };

        let Value::Object(obj) = parsed else {
            write_json_line(&out, &rpc_error(Value::Null, -32600, "Invalid Request"))?;
            continue;
        };

        // A JSON-RPC notification has no "id" member and never gets a reply.
        let Some(id) = obj.get("id").cloned() else {
            if let Some("notifications/cancelled") = obj.get("method").and_then(|m| m.as_str()) {
                if let Some(req_id) = obj.get("params").and_then(|p| p.get("requestId")) {
                    let mut guard = cancel_state.lock().unwrap_or_else(|e| e.into_inner());
                    guard.cancel(req_id);
                }
            }
            continue;
        };

        let Some(method) = obj.get("method").and_then(|m| m.as_str()) else {
            let reply = rpc_error(id, -32600, "Invalid Request: missing method");
            write_json_line(&out, &reply)?;
            continue;
        };

        match method {
            "initialize" => {
                let client_ver = obj
                    .get("params")
                    .and_then(|p| p.get("protocolVersion"))
                    .and_then(|v| v.as_str());
                let chosen = match client_ver {
                    Some(v) if SUPPORTED_VERSIONS.contains(&v) => v,
                    _ => SUPPORTED_VERSIONS[0],
                };
                let reply = rpc_result(
                    id,
                    json!({
                        "protocolVersion": chosen,
                        "capabilities": { "tools": {} },
                        "serverInfo": {
                            "name": "rollchrome",
                            "version": env!("CARGO_PKG_VERSION"),
                        }
                    }),
                );
                write_json_line(&out, &reply)?;
            }
            "ping" => {
                let reply = rpc_result(id, json!({}));
                write_json_line(&out, &reply)?;
            }
            "tools/list" => {
                let reply = rpc_result(id, tools_schema_list());
                write_json_line(&out, &reply)?;
            }
            "tools/call" => {
                if tx.send(ToolCall { id, obj }).is_err() {
                    break;
                }
            }
            other => {
                let reply = rpc_error(id, -32601, format!("Method not found: {other}"));
                write_json_line(&out, &reply)?;
            }
        }
    }

    {
        let mut guard = cancel_state.lock().unwrap_or_else(|e| e.into_inner());
        guard.close_input();
    }
    drop(tx);
    match worker.join() {
        Ok(res) => res,
        Err(_) => Err(Error::Internal("MCP worker thread panicked".to_string())),
    }
}
