//! Acceptance tests for RP-13 (`rollchrome_core::automation::mcp`).

#[allow(dead_code)]
mod support;

use std::collections::VecDeque;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use phototools_core::config::{Config, Thresholds};
use rollchrome_core::automation::mcp::serve;
use rollchrome_core::automation::Automation;
use serde_json::{json, Value};
use support::dng::DngBuilder;

struct TestFixture {
    _temp: tempfile::TempDir,
    config: Config,
    root: PathBuf,
}

impl TestFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("library");
        let staging = temp.path().join("staging");
        fs::create_dir_all(&root).expect("create root");
        fs::create_dir_all(&staging).expect("create staging");
        let root = root.canonicalize().expect("canonicalize root");

        let config = Config {
            roots: vec![root.clone()],
            staging_dir: staging,
            publishing_dir: None,
            thresholds: Thresholds::default(),
            database: temp.path().join("ledger.sqlite3"),
        };

        Self {
            _temp: temp,
            config,
            root,
        }
    }

    fn write_dng(&self, dir: &Path, name: &str, pixel_value: u16) -> PathBuf {
        let builder = DngBuilder::new_bayer(16, 16);
        let pixels = vec![pixel_value; 16 * 16];
        let bytes = builder.build(&pixels);
        let path = dir.join(name);
        fs::write(&path, bytes).expect("write DNG");
        path
    }
}

type PipeState = Arc<(Mutex<(VecDeque<u8>, bool)>, Condvar)>;

/// Creates an in-memory byte pipe (`Read` + `Write`) with condition variable wakeup for
/// interactive client-server tests (`notifications/cancelled`, streaming progress).
fn memory_pipe() -> (PipeWriter, PipeReader) {
    let state: PipeState = Arc::default();
    (
        PipeWriter {
            state: Arc::clone(&state),
        },
        PipeReader { state },
    )
}

struct PipeWriter {
    state: PipeState,
}

impl Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let (lock, cvar) = &*self.state;
        let mut guard = lock.lock().unwrap();
        guard.0.extend(buf);
        cvar.notify_all();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.state;
        if let Ok(mut guard) = lock.lock() {
            guard.1 = true;
            cvar.notify_all();
        }
    }
}

struct PipeReader {
    state: PipeState,
}

impl Read for PipeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let (lock, cvar) = &*self.state;
        let mut guard = lock.lock().unwrap();
        while guard.0.is_empty() && !guard.1 {
            guard = cvar.wait(guard).unwrap();
        }
        if guard.0.is_empty() && guard.1 {
            return Ok(0);
        }
        let n = buf.len().min(guard.0.len());
        for slot in buf.iter_mut().take(n) {
            *slot = guard.0.pop_front().unwrap();
        }
        Ok(n)
    }
}

fn run_session_lines(config: Config, input_lines: &[String]) -> Vec<String> {
    let expected_replies = input_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| match serde_json::from_str::<Value>(l) {
            Ok(Value::Object(m)) => m.contains_key("id"),
            _ => true,
        })
        .count();

    let (mut client_to_server, server_input) = memory_pipe();
    let (server_output, server_to_client) = memory_pipe();

    let server_thread = thread::spawn(move || {
        serve(
            BufReader::new(server_input),
            server_output,
            Automation::new(config),
        )
        .expect("mcp serve");
    });

    for line in input_lines {
        writeln!(client_to_server, "{line}").unwrap();
    }

    let mut client_reader = BufReader::new(server_to_client);
    let mut out_lines = Vec::new();
    let mut replies_seen = 0usize;

    while replies_seen < expected_replies {
        let mut line = String::new();
        let n = client_reader.read_line(&mut line).unwrap();
        if n == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(&['\r', '\n'][..]).to_string();
        if trimmed.trim().is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<Value>(&trimmed) {
            if val.get("method").is_none() {
                replies_seen += 1;
            }
        }
        out_lines.push(trimmed);
    }

    drop(client_to_server);
    loop {
        let mut line = String::new();
        let n = client_reader.read_line(&mut line).unwrap();
        if n == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(&['\r', '\n'][..]).to_string();
        if !trimmed.trim().is_empty() {
            out_lines.push(trimmed);
        }
    }

    server_thread.join().unwrap();
    out_lines
}

fn run_session(config: Config, input_lines: &[String]) -> Vec<Value> {
    run_session_lines(config, input_lines)
        .into_iter()
        .map(|l| serde_json::from_str(&l).expect("valid JSON line"))
        .collect()
}

#[test]
fn initialize_returns_the_clients_version_when_supported() {
    let f = TestFixture::new();
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "1.0" }
        }
    });
    let replies = run_session(f.config, &[req.to_string()]);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["id"], json!(1));
    assert_eq!(replies[0]["result"]["protocolVersion"], json!("2025-03-26"));
    assert_eq!(replies[0]["result"]["capabilities"], json!({ "tools": {} }));
    assert_eq!(
        replies[0]["result"]["serverInfo"]["name"],
        json!("rollchrome")
    );
}

#[test]
fn initialize_offers_the_latest_when_the_clients_is_unknown() {
    let f = TestFixture::new();
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "1999-01-01",
            "capabilities": {},
            "clientInfo": { "name": "old-client", "version": "0.1" }
        }
    });
    let replies = run_session(f.config, &[req.to_string()]);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["id"], json!(1));
    assert_eq!(replies[0]["result"]["protocolVersion"], json!("2025-06-18"));
}

#[test]
fn tools_list_names_the_seven_tools_each_with_a_schema() {
    let f = TestFixture::new();
    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    let replies = run_session(f.config, &[req.to_string()]);
    assert_eq!(replies.len(), 1);
    let tools = replies[0]["result"]["tools"]
        .as_array()
        .expect("tools array");
    assert_eq!(tools.len(), 7);

    let names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().expect("tool name"))
        .collect();
    assert_eq!(
        names,
        vec![
            "list_looks",
            "list_luts",
            "plan_develop",
            "run_develop",
            "plan_batch",
            "run_batch",
            "export",
        ]
    );

    for tool in tools {
        assert!(
            tool["description"]
                .as_str()
                .is_some_and(|d| !d.trim().is_empty()),
            "tool {} missing description",
            tool["name"]
        );
        assert_eq!(tool["inputSchema"]["type"], json!("object"));
        assert!(tool["inputSchema"]["required"].is_array());
    }

    let run_dev = tools.iter().find(|t| t["name"] == "run_develop").unwrap();
    let run_dev_req = run_dev["inputSchema"]["required"].as_array().unwrap();
    assert!(run_dev_req.contains(&json!("plan_sha256")));

    let run_batch = tools.iter().find(|t| t["name"] == "run_batch").unwrap();
    let run_batch_req = run_batch["inputSchema"]["required"].as_array().unwrap();
    assert!(run_batch_req.contains(&json!("plan_sha256")));
}

#[test]
fn a_notification_gets_no_reply() {
    let f = TestFixture::new();
    let notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    let ping = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "ping"
    });
    let replies = run_session(f.config, &[notif.to_string(), ping.to_string()]);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["id"], json!(10));
    assert_eq!(replies[0]["result"], json!({}));
}

#[test]
fn an_unknown_method_gets_minus_32601() {
    let f = TestFixture::new();
    let req = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "resources/list",
        "params": {}
    });
    let replies = run_session(f.config, &[req.to_string()]);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["id"], json!(7));
    assert_eq!(replies[0]["error"]["code"], json!(-32601));
}

#[test]
fn bad_json_gets_minus_32700_and_the_server_carries_on() {
    let f = TestFixture::new();
    let bad = "{not valid json".to_string();
    let ping = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "ping"
    })
    .to_string();

    let replies = run_session(f.config, &[bad, ping]);
    assert_eq!(replies.len(), 2);
    assert!(replies[0]["id"].is_null());
    assert_eq!(replies[0]["error"]["code"], json!(-32700));
    assert_eq!(replies[1]["id"], json!(3));
    assert_eq!(replies[1]["result"], json!({}));
}

#[test]
fn a_json_array_gets_minus_32600() {
    let f = TestFixture::new();
    let arr = json!([
        { "jsonrpc": "2.0", "id": 1, "method": "ping" }
    ])
    .to_string();
    let replies = run_session(f.config, &[arr]);
    assert_eq!(replies.len(), 1);
    assert!(replies[0]["id"].is_null());
    assert_eq!(replies[0]["error"]["code"], json!(-32600));
}

#[test]
fn a_refused_tool_call_is_is_error_not_a_protocol_error() {
    let f = TestFixture::new();
    let outside = f._temp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();

    let call = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": {
            "name": "plan_develop",
            "arguments": {
                "folder": outside.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome"
            }
        }
    });
    let replies = run_session(f.config, &[call.to_string()]);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["id"], json!(11));
    assert!(replies[0].get("error").is_none());
    assert_eq!(replies[0]["result"]["isError"], json!(true));
    assert_eq!(
        replies[0]["result"]["content"][0]["text"],
        json!("That path is outside the folders this application may touch.")
    );
}

#[test]
fn a_tool_given_bad_arguments_is_an_error_naming_the_argument() {
    let f = TestFixture::new();
    let missing_look = json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "tools/call",
        "params": {
            "name": "plan_develop",
            "arguments": {
                "folder": f.root.to_string_lossy()
            }
        }
    });
    let wrong_type = json!({
        "jsonrpc": "2.0",
        "id": 13,
        "method": "tools/call",
        "params": {
            "name": "plan_develop",
            "arguments": {
                "folder": f.root.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome",
                "exposure_ev": "not-a-float"
            }
        }
    });
    let replies = run_session(
        f.config,
        &[missing_look.to_string(), wrong_type.to_string()],
    );
    assert_eq!(replies.len(), 2);

    assert_eq!(replies[0]["result"]["isError"], json!(true));
    let text0 = replies[0]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text0.contains("look"), "expected 'look' in error: {text0}");

    assert_eq!(replies[1]["result"]["isError"], json!(true));
    let text1 = replies[1]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text1.contains("exposure_ev"),
        "expected 'exposure_ev' in error: {text1}"
    );
}

#[test]
fn plan_then_run_through_the_protocol_develops_a_folder() {
    let f = TestFixture::new();
    f.write_dng(&f.root, "DSCF0001.dng", 2600);

    let plan_call = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "plan_develop",
            "arguments": {
                "folder": f.root.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome"
            }
        }
    });
    let plan_replies = run_session(f.config.clone(), &[plan_call.to_string()]);
    assert_eq!(plan_replies.len(), 1);
    assert_eq!(plan_replies[0]["result"]["isError"], json!(false));
    let plan_sha256 = plan_replies[0]["result"]["structuredContent"]["plan_sha256"]
        .as_str()
        .expect("plan_sha256 string")
        .to_string();
    assert!(!f.root.join("Developed").exists());

    let run_call = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "run_develop",
            "arguments": {
                "folder": f.root.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome",
                "plan_sha256": plan_sha256
            }
        }
    });
    let run_replies = run_session(f.config, &[run_call.to_string()]);
    assert_eq!(run_replies.len(), 1);
    assert_eq!(run_replies[0]["result"]["isError"], json!(false));
    assert_eq!(
        run_replies[0]["result"]["structuredContent"]["developed"],
        json!(["DSCF0001.jpg"])
    );
    assert!(f.root.join("Developed/DSCF0001.jpg").is_file());
}

#[test]
fn run_without_a_plan_hash_is_refused_and_writes_nothing() {
    let f = TestFixture::new();
    f.write_dng(&f.root, "DSCF0001.dng", 2600);

    let missing_hash = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "run_develop",
            "arguments": {
                "folder": f.root.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome"
            }
        }
    });
    let wrong_hash = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "run_develop",
            "arguments": {
                "folder": f.root.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome",
                "plan_sha256": "deadbeef"
            }
        }
    });

    let replies = run_session(
        f.config,
        &[missing_hash.to_string(), wrong_hash.to_string()],
    );
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["result"]["isError"], json!(true));
    assert!(replies[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("plan_sha256"));
    assert_eq!(replies[1]["result"]["isError"], json!(true));
    assert!(!f.root.join("Developed").exists());
}

#[test]
fn progress_notifications_carry_the_callers_token() {
    let f = TestFixture::new();
    f.write_dng(&f.root, "DSCF0001.dng", 2600);
    f.write_dng(&f.root, "DSCF0002.dng", 2600);

    let plan_call = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "plan_develop",
            "arguments": {
                "folder": f.root.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome"
            }
        }
    });
    let plan_replies = run_session(f.config.clone(), &[plan_call.to_string()]);
    let plan_sha256 = plan_replies[0]["result"]["structuredContent"]["plan_sha256"]
        .as_str()
        .unwrap()
        .to_string();

    let run_call = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "run_develop",
            "arguments": {
                "folder": f.root.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome",
                "plan_sha256": plan_sha256
            },
            "_meta": {
                "progressToken": "tok-42"
            }
        }
    });

    let messages = run_session(f.config, &[run_call.to_string()]);
    let progress_msgs: Vec<&Value> = messages
        .iter()
        .filter(|m| m["method"] == json!("notifications/progress"))
        .collect();
    assert!(!progress_msgs.is_empty());
    for m in &progress_msgs {
        assert_eq!(m["params"]["progressToken"], json!("tok-42"));
        assert_eq!(m["params"]["total"], json!(2));
        assert!(m["params"]["progress"].is_number());
    }
    assert_eq!(
        progress_msgs.last().unwrap()["params"]["progress"],
        json!(2)
    );
    assert_eq!(messages.last().unwrap()["id"], json!(2));
    assert_eq!(messages.last().unwrap()["result"]["isError"], json!(false));
}

#[test]
fn notifications_cancelled_stops_a_running_tool() {
    let f = TestFixture::new();
    for i in 1..=6 {
        f.write_dng(&f.root, &format!("DSCF000{i}.dng"), 2600);
    }

    let auto = Automation::new(f.config.clone());
    let (mut client_to_server, server_input) = memory_pipe();
    let (server_output, server_to_client) = memory_pipe();

    let server_thread = thread::spawn(move || {
        serve(BufReader::new(server_input), server_output, auto).expect("serve");
    });

    let mut client_reader = BufReader::new(server_to_client);

    // 1. Plan develop
    let plan_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "plan_develop",
            "arguments": {
                "folder": f.root.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome"
            }
        }
    });
    writeln!(client_to_server, "{plan_req}").unwrap();

    let mut line = String::new();
    client_reader.read_line(&mut line).unwrap();
    let plan_resp: Value = serde_json::from_str(&line).unwrap();
    let plan_sha256 = plan_resp["result"]["structuredContent"]["plan_sha256"]
        .as_str()
        .unwrap()
        .to_string();
    let planned_count = plan_resp["result"]["structuredContent"]["plan"]["actions"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(planned_count, 6);

    // 2. Start run_develop with progress token
    let run_req = json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "tools/call",
        "params": {
            "name": "run_develop",
            "arguments": {
                "folder": f.root.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome",
                "plan_sha256": plan_sha256
            },
            "_meta": {
                "progressToken": "cancel-token"
            }
        }
    });
    writeln!(client_to_server, "{run_req}").unwrap();

    // Wait for the first progress notification (emitted when item 0 starts), then cancel
    let mut sent_cancel = false;
    let mut final_reply: Option<Value> = None;
    loop {
        line.clear();
        let n = client_reader.read_line(&mut line).unwrap();
        if n == 0 {
            break;
        }
        let msg: Value = serde_json::from_str(&line).unwrap();
        if msg["method"] == json!("notifications/progress") && !sent_cancel {
            let cancel_notif = json!({
                "jsonrpc": "2.0",
                "method": "notifications/cancelled",
                "params": { "requestId": 99 }
            });
            writeln!(client_to_server, "{cancel_notif}").unwrap();
            sent_cancel = true;
        } else if msg["id"] == json!(99) {
            final_reply = Some(msg);
            break;
        }
    }

    drop(client_to_server);
    server_thread.join().unwrap();

    let reply = final_reply.expect("final reply for request 99");
    assert_eq!(reply["result"]["isError"], json!(false));
    let developed = reply["result"]["structuredContent"]["developed"]
        .as_array()
        .expect("developed array");
    assert!(
        !developed.is_empty() && developed.len() < planned_count,
        "expected fewer photographs developed than planned ({planned_count}), got {}",
        developed.len()
    );
}

#[test]
fn every_line_written_is_one_json_rpc_message() {
    let f = TestFixture::new();
    f.write_dng(&f.root, "DSCF0001.dng", 2600);

    // First obtain the plan hash so we can run a full session in one stream
    let preview_plan = run_session(
        f.config.clone(),
        &[json!({
            "jsonrpc": "2.0",
            "id": 100,
            "method": "tools/call",
            "params": {
                "name": "plan_develop",
                "arguments": {
                    "folder": f.root.to_string_lossy(),
                    "look": "builtin:fujifilm-classic-chrome"
                }
            }
        })
        .to_string()],
    );
    let plan_sha256 = preview_plan[0]["result"]["structuredContent"]["plan_sha256"]
        .as_str()
        .unwrap()
        .to_string();

    let session_inputs = [
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2025-06-18" }
        })
        .to_string(),
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        })
        .to_string(),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        })
        .to_string(),
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "plan_develop",
                "arguments": {
                    "folder": f.root.to_string_lossy(),
                    "look": "builtin:fujifilm-classic-chrome"
                }
            }
        })
        .to_string(),
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "run_develop",
                "arguments": {
                    "folder": f.root.to_string_lossy(),
                    "look": "builtin:fujifilm-classic-chrome",
                    "plan_sha256": plan_sha256
                },
                "_meta": { "progressToken": "session-tok" }
            }
        })
        .to_string(),
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "ping"
        })
        .to_string(),
    ];

    let lines = run_session_lines(f.config, &session_inputs);
    assert!(lines.len() >= 6);
    for line in &lines {
        assert!(!line.contains('\n'));
        let parsed: Value = serde_json::from_str(line).expect("each line must be valid JSON");
        assert!(parsed.is_object(), "each line must be a JSON object");
        assert_eq!(parsed["jsonrpc"], json!("2.0"));
    }
}

#[test]
fn ping_is_answered_while_a_tool_runs() {
    let f = TestFixture::new();
    for i in 1..=6 {
        f.write_dng(&f.root, &format!("DSCF000{i}.dng"), 2600);
    }

    let auto = Automation::new(f.config.clone());
    let (mut client_to_server, server_input) = memory_pipe();
    let (server_output, server_to_client) = memory_pipe();

    let server_thread = thread::spawn(move || {
        serve(BufReader::new(server_input), server_output, auto).expect("serve");
    });

    let mut client_reader = BufReader::new(server_to_client);

    // 1. Plan develop
    let plan_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "plan_develop",
            "arguments": {
                "folder": f.root.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome"
            }
        }
    });
    writeln!(client_to_server, "{plan_req}").unwrap();

    let mut line = String::new();
    client_reader.read_line(&mut line).unwrap();
    let plan_resp: Value = serde_json::from_str(&line).unwrap();
    let plan_sha256 = plan_resp["result"]["structuredContent"]["plan_sha256"]
        .as_str()
        .unwrap()
        .to_string();

    // 2. Start run_develop (id: 50)
    let run_req = json!({
        "jsonrpc": "2.0",
        "id": 50,
        "method": "tools/call",
        "params": {
            "name": "run_develop",
            "arguments": {
                "folder": f.root.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome",
                "plan_sha256": plan_sha256
            },
            "_meta": {
                "progressToken": "ping-tok"
            }
        }
    });
    writeln!(client_to_server, "{run_req}").unwrap();

    let mut sent_ping = false;
    let mut ping_answered_before_tool_finished = false;
    let mut tool_finished = false;

    loop {
        line.clear();
        let n = client_reader.read_line(&mut line).unwrap();
        if n == 0 {
            break;
        }
        let msg: Value = serde_json::from_str(&line).unwrap();
        if msg["method"] == json!("notifications/progress") && !sent_ping {
            // Tool is actively running on the worker; send ping (id: 51)
            let ping_req = json!({
                "jsonrpc": "2.0",
                "id": 51,
                "method": "ping"
            });
            writeln!(client_to_server, "{ping_req}").unwrap();
            sent_ping = true;
        } else if msg["id"] == json!(51) {
            assert_eq!(msg["result"], json!({}));
            if !tool_finished {
                ping_answered_before_tool_finished = true;
            }
            // Cancel the tool now that ping has been answered
            let cancel_notif = json!({
                "jsonrpc": "2.0",
                "method": "notifications/cancelled",
                "params": { "requestId": 50 }
            });
            writeln!(client_to_server, "{cancel_notif}").unwrap();
        } else if msg["id"] == json!(50) {
            tool_finished = true;
            if sent_ping && ping_answered_before_tool_finished {
                break;
            }
        }
        if tool_finished && sent_ping && msg["id"] == json!(51) {
            break;
        }
    }

    drop(client_to_server);
    server_thread.join().unwrap();

    assert!(
        ping_answered_before_tool_finished,
        "expected ping (id 51) to be answered while run_develop (id 50) was still running"
    );
}

#[test]
fn closing_the_input_stops_a_running_tool() {
    let f = TestFixture::new();
    for i in 1..=6 {
        f.write_dng(&f.root, &format!("DSCF000{i}.dng"), 2600);
    }

    let auto = Automation::new(f.config.clone());
    let (mut client_to_server, server_input) = memory_pipe();
    let (server_output, server_to_client) = memory_pipe();

    let server_thread = thread::spawn(move || {
        serve(BufReader::new(server_input), server_output, auto).expect("serve");
    });

    let mut client_reader = BufReader::new(server_to_client);

    // 1. Plan develop
    let plan_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "plan_develop",
            "arguments": {
                "folder": f.root.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome"
            }
        }
    });
    writeln!(client_to_server, "{plan_req}").unwrap();

    let mut line = String::new();
    client_reader.read_line(&mut line).unwrap();
    let plan_resp: Value = serde_json::from_str(&line).unwrap();
    let plan_sha256 = plan_resp["result"]["structuredContent"]["plan_sha256"]
        .as_str()
        .unwrap()
        .to_string();
    let planned_count = plan_resp["result"]["structuredContent"]["plan"]["actions"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(planned_count, 6);

    // 2. Start run_develop (id: 60) with progress token
    let run_req = json!({
        "jsonrpc": "2.0",
        "id": 60,
        "method": "tools/call",
        "params": {
            "name": "run_develop",
            "arguments": {
                "folder": f.root.to_string_lossy(),
                "look": "builtin:fujifilm-classic-chrome",
                "plan_sha256": plan_sha256
            },
            "_meta": {
                "progressToken": "eof-tok"
            }
        }
    });
    writeln!(client_to_server, "{run_req}").unwrap();

    // Wait for the first progress notification, then immediately close the input pipe
    let mut closed_input = false;
    let mut final_reply: Option<Value> = None;
    let mut maybe_client_to_server = Some(client_to_server);

    loop {
        line.clear();
        let n = client_reader.read_line(&mut line).unwrap();
        if n == 0 {
            break;
        }
        let msg: Value = serde_json::from_str(&line).unwrap();
        if msg["method"] == json!("notifications/progress") && !closed_input {
            drop(maybe_client_to_server.take());
            closed_input = true;
        } else if msg["id"] == json!(60) {
            final_reply = Some(msg);
            break;
        }
    }

    drop(maybe_client_to_server);
    server_thread.join().unwrap();

    let reply = final_reply.expect("final reply for request 60");
    assert_eq!(reply["result"]["isError"], json!(false));
    let developed = reply["result"]["structuredContent"]["developed"]
        .as_array()
        .expect("developed array");
    assert!(
        !developed.is_empty() && developed.len() < planned_count,
        "expected closing input to stop run_develop before all {planned_count} photographs developed, got {}",
        developed.len()
    );
}
