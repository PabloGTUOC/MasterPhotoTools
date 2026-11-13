//! Phase 5 acceptance tests.
//!
//! These run a real server on an ephemeral port and talk to it over HTTP, so the
//! extractors, routing, status codes and the SSE stream are all exercised as a
//! client would meet them. No test reaches the network.

use jsonwebtoken::{encode, Algorithm, DecodingKey, EncodingKey, Header};
use phototools_core::config::{Config, Thresholds};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use phototools_server::{auth, build_router, jobs, AppState};

const TEST_PRIVATE_KEY: &[u8] = b"-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDDgTuaUUsi1A/7\ntJHnfp9wBMSVaGpMgGXS11jXPwQaqSPJ+7DYb73Lf6XK7a2PkNtmQOk8vJpp99dZ\nnrmamdEJmS/U/rfFJRjMIFXSOx9pIwbteL+3TPwt48kmKSO/TVdGa+JXZT+utZMQ\natbi7Ta3cVBuy7iRRPqav/xD8gbubCARCxtjymVoUTNTkyYEpYOjMLniX3AQuejC\n1x6e/qCUVVWfE+/CUS2vehYTPtsenQ8XOmbXq0CfURuIIapqGJwjXYV67dWuY1jK\nfYd2Z6s3ZoTAu8EzBP9zflK+vAB3ZyLsg7gthBtdhrmGIH6YqmuiYERjA5SlXZ1J\nzoeWoZnFAgMBAAECggEACBpuEaO6CkD4n+VxL3IQ2bGTFFWHmDQl1bxy51BNVie8\njXe9iRgeY5MTO2PReLWDP5Sm/uhg3hOJ5dxQhRcw1/RGkitLIqdGPx49zXsYxGCi\n7IHuMFQ7c/QzlFT462zyrXlG5jQSrAMh6PinlrvrYh8WxxggXY3JRsgEJ6Ep7L8g\nWrHNTUxJab1UR2T9sld2joFvjuJ31qE9ohzCMflA7VLEI26Ki68guvsGGY1kc5WE\nm46JBQlTwo+CutczZGoCk+hBiNMaMjDyQ66KHZtAfhVGZKJ0O3WbDNFFr5XZ8XHt\nI5xFJRP8KYYaejYW8Y0dEkLidWUfI8AfXCIbwVuT4QKBgQDoCfCPlLOMFeRWGjVb\n9qRt2NaUs9HWmi24vT+Y6jfyUjqdtdaJeSo5b+OM8s96ruM/pTcLIPdoUcRTsPee\n9TAe/T+uZ/Hf6yooka0VzqnA3MT+N+tmIArpRIPUJkOGUgFSSbf4FVYJg0sF1vs4\n6o/ucIDeRaECD7pMwk5fikTrqwKBgQDXsX+k6UJbUwYItq0o101a3YtVnCohRqxG\nL+4pxnownrKarUnKWeyNca4mIjsQA7m5Rh/8/xDqt3rfZw4cC6sr2aHRE+sFGL2I\nNBPj3WIc4T/7N3HTJhBMGjbqzyzwK9GOueX/tdq+iTXF4ui12MPHEnwvzdlE6gNA\ntN+TZjagTwKBgBja17XJi+H5hlfivsx3Au3xSCrtiBCguz0KqIFMtWlzfWvfSne3\nTtqQLaOvbqIJkbYDkH3UriuydoEwd5XDVcA8CFI6OCJwIjfuQsgPNwe9nixM+R4b\nWI/cEvLqllkQ96tE0jv0rR6fva2GdaqHFZvI2UT12GVMIfyO465ANVm5AoGAc6N2\nC7QDH3MjiQhnTb4getbMHNncvHpnYjnQNhVy7R4oI0VEingrmqmX9Fnl0HAu4mX2\nQG1/ZFd6SMu3hNG8s4W6e51yIwlgk+VXxJKsR098PfM70zhVBHgJeVoZfaoAb8S6\nyp106TIm4jEFEnlkfRYr/nUeRxQvKkHOm/fw0YECgYEAiYsibzGcZxVzVOwpG7P5\neBNEEDtnUfydzOfyWDh8F2eCpmZCCgw2C+SOK00rvb9Z74G98I8U4lgF9oSvNljc\nBKpLBdGmIi3Co7F4eoTjEHw6Cvhjy4MHFTRlzMcGwCdhk8buODCoQR2P5lJqF6rk\n3KYEVrmjZQOnYuH0vGQVYtk=\n-----END PRIVATE KEY-----";
const TEST_PUBLIC_KEY: &[u8] = b"-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAw4E7mlFLItQP+7SR536f\ncATElWhqTIBl0tdY1z8EGqkjyfuw2G+9y3+lyu2tj5DbZkDpPLyaaffXWZ65mpnR\nCZkv1P63xSUYzCBV0jsfaSMG7Xi/t0z8LePJJikjv01XRmviV2U/rrWTEGrW4u02\nt3FQbsu4kUT6mr/8Q/IG7mwgEQsbY8plaFEzU5MmBKWDozC54l9wELnowtcenv6g\nlFVVnxPvwlEtr3oWEz7bHp0PFzpm16tAn1EbiCGqahicI12Feu3VrmNYyn2Hdmer\nN2aEwLvBMwT/c35SvrwAd2ci7IO4LYQbXYa5hiB+mKpromBEYwOUpV2dSc6HlqGZ\nxQIDAQAB\n-----END PUBLIC KEY-----";

const KID: &str = "test-kid";
const PROJECT: &str = "phototools-test";
const ALLOWED_UID: &str = "photographer-1";

/// A running server, its base URL, and the library root it is confined to.
struct TestServer {
    base: String,
    root: PathBuf,
    database: PathBuf,
    _temp: tempfile::TempDir,
}

// The phone's credential, when a test wants that route switched on. A
// thread-local rather than an argument to `start()`, so the signature every
// other test uses does not grow a parameter about a route they do not care
// about.
thread_local! {
    static DEVICE: std::cell::RefCell<Option<auth::DeviceCredential>> =
        const { std::cell::RefCell::new(None) };
}

fn device_credential() -> Option<auth::DeviceCredential> {
    DEVICE.with(|d| d.borrow().clone())
}

fn with_device(user: &str, token: &str) {
    DEVICE.with(|d| {
        *d.borrow_mut() = Some(auth::DeviceCredential {
            user: user.into(),
            token: token.into(),
        })
    });
}

async fn start() -> TestServer {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    std::fs::create_dir(&root).unwrap();

    let config = Config {
        roots: vec![root.canonicalize().unwrap()],
        thresholds: Thresholds::default(),
        database: temp.path().join("ledger.sqlite3"),
    };

    let auth_config = auth::AuthConfig {
        project_id: PROJECT.into(),
        allowed_uids: vec![ALLOWED_UID.into()],
        admin_token: None,
        keys: auth::KeyStore::offline(),
        device: device_credential(),
    };
    auth_config
        .keys
        .insert(KID, DecodingKey::from_rsa_pem(TEST_PUBLIC_KEY).unwrap())
        .await;

    let ledger = phototools_core::ledger::Ledger::open(&config.database).unwrap();
    let manager = jobs::JobManager::new(ledger);

    let state = AppState {
        config: std::sync::Arc::new(config),
        auth: std::sync::Arc::new(auth_config),
        jobs: std::sync::Arc::new(manager),
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        axum::serve(listener, build_router(state)).await.unwrap();
    });

    // Give the accept loop a moment to come up.
    tokio::time::sleep(Duration::from_millis(50)).await;

    TestServer {
        base: format!("http://127.0.0.1:{port}"),
        root: root.canonicalize().unwrap(),
        database: temp.path().join("ledger.sqlite3"),
        _temp: temp,
    }
}

fn now() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
}

fn token_for(sub: &str, exp: usize) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KID.into());
    encode(
        &header,
        &auth::Claims {
            iss: format!("https://securetoken.google.com/{PROJECT}"),
            aud: PROJECT.into(),
            exp,
            sub: sub.into(),
        },
        &EncodingKey::from_rsa_pem(TEST_PRIVATE_KEY).unwrap(),
    )
    .unwrap()
}

fn good_token() -> String {
    token_for(ALLOWED_UID, now() + 3600)
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_is_reachable_without_a_token() {
    let s = start().await;
    let response = reqwest::get(format!("{}/api/health", s.base))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

/// **Phase 5 acceptance.** Every `/api/tools/*` route returns 401 without a token.
#[tokio::test]
async fn every_tool_route_refuses_an_anonymous_request() {
    let s = start().await;
    let client = reqwest::Client::new();

    let posts = [
        "/api/tools/dates/scan",
        "/api/tools/dates/fix",
        "/api/tools/rename/plan",
        "/api/tools/rename/apply",
        "/api/tools/split",
        "/api/tools/contact-sheet",
        "/api/tools/transform",
        "/api/tools/border",
        "/api/tools/tiff-to-jpeg",
    ];

    for route in posts {
        let response = client
            .post(format!("{}{route}", s.base))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401, "{route} should require a token");
    }

    for route in [
        "/api/storage/ls?path=/tmp",
        "/api/jobs/anything",
        "/api/jobs/anything/events",
    ] {
        let response = client
            .get(format!("{}{route}", s.base))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401, "{route} should require a token");
    }
}

#[tokio::test]
async fn an_expired_token_is_distinguishable_from_a_forbidden_one() {
    let s = start().await;
    let client = reqwest::Client::new();

    // Comfortably past expiry, and past any clock-skew leeway.
    let expired = token_for(ALLOWED_UID, now() - 3600);
    let response = client
        .get(format!(
            "{}/api/storage/ls?path={}",
            s.base,
            s.root.display()
        ))
        .bearer_auth(&expired)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    let body: Value = response.json().await.unwrap();
    assert_eq!(
        body["code"], "token_expired",
        "a client must know to refresh and retry rather than drop to login"
    );

    // A valid token from an account that is simply not invited.
    let stranger = token_for("not-invited", now() + 3600);
    let response = client
        .get(format!(
            "{}/api/storage/ls?path={}",
            s.base,
            s.root.display()
        ))
        .bearer_auth(&stranger)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "not_authorized");
}

#[tokio::test]
async fn a_malformed_authorization_header_is_refused() {
    let s = start().await;
    let client = reqwest::Client::new();

    for header in ["Basic abc", "Bearer", "nonsense"] {
        let response = client
            .get(format!("{}/api/storage/ls?path=/tmp", s.base))
            .header("Authorization", header)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401, "header {header:?}");
    }
}

// ---------------------------------------------------------------------------
// G6 — path confinement, end to end through the API
// ---------------------------------------------------------------------------

/// **Phase 5 acceptance.** A path-traversal attempt through the API is rejected.
#[tokio::test]
async fn path_traversal_through_the_api_is_rejected() {
    let s = start().await;
    let client = reqwest::Client::new();

    // Something real and readable, safely outside the root.
    let outside = s.root.parent().unwrap().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.jpg"), b"private").unwrap();

    let attempts = [
        format!("{}/../outside", s.root.display()),
        outside.display().to_string(),
        "/etc".to_string(),
        format!("{}/../../..", s.root.display()),
    ];

    for path in attempts {
        let response = client
            .get(format!("{}/api/storage/ls", s.base))
            .query(&[("path", &path)])
            .bearer_auth(good_token())
            .send()
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            403,
            "listing {path} should be refused, not served"
        );
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["code"], "path_not_allowed");
    }

    // The root itself is fine, which proves the refusals are not blanket.
    let response = client
        .get(format!("{}/api/storage/ls", s.base))
        .query(&[("path", s.root.display().to_string())])
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn a_tool_refuses_an_output_directory_outside_the_roots() {
    let s = start().await;
    let client = reqwest::Client::new();
    std::fs::write(s.root.join("a.jpg"), b"x").unwrap();

    let response = client
        .post(format!("{}/api/tools/border", s.base))
        .bearer_auth(good_token())
        .json(&json!({
            "inputs": [s.root.join("a.jpg").display().to_string()],
            "out_dir": "/tmp/escape-hatch",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 403);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "path_not_allowed");
}

// ---------------------------------------------------------------------------
// Jobs and SSE
// ---------------------------------------------------------------------------

/// **Phase 5 acceptance.** An SSE client receives progress events and a terminal
/// event.
#[tokio::test]
async fn a_job_streams_progress_and_ends_with_a_terminal_event() {
    let s = start().await;
    let client = reqwest::Client::new();

    // Enough files that the job reports progress.
    let mut frames = Vec::new();
    for i in 0..8 {
        let frame = s.root.join(format!("frame{i}.jpg"));
        std::fs::write(&frame, b"x").unwrap();
        frames.push(frame.display().to_string());
    }

    // A dry-run date repair: still a job, and it writes nothing. The scan that
    // used to stand here answers with its rows directly now, because a scan
    // writes nothing and its whole value is the table.
    let response = client
        .post(format!("{}/api/tools/dates/fix", s.base))
        .bearer_auth(good_token())
        .json(&json!({ "paths": frames, "mode": "Auto", "dry_run": true }))
        .send()
        .await
        .unwrap();

    // F17: the request returns immediately with an id, not a result.
    assert_eq!(response.status(), 202);
    let body: Value = response.json().await.unwrap();
    let job_id = body["job_id"].as_str().unwrap().to_string();

    // The job row exists straight away.
    let state = client
        .get(format!("{}/api/jobs/{job_id}", s.base))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap();
    assert_eq!(state.status(), 200);
    let job: Value = state.json().await.unwrap();
    assert_eq!(job["kind"], "dates_fix");

    // Read the event stream to its end.
    let stream = client
        .get(format!("{}/api/jobs/{job_id}/events", s.base))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap();
    assert_eq!(stream.status(), 200);
    assert!(stream
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/event-stream"));
    // Behind nginx the stream is buffered to the end unless the response says
    // otherwise, and a progress stream that arrives all at once is not one.
    assert_eq!(
        stream
            .headers()
            .get("x-accel-buffering")
            .map(|v| v.to_str().unwrap()),
        Some("no"),
        "the event stream must tell a proxy not to buffer it"
    );

    let text = tokio::time::timeout(Duration::from_secs(20), stream.text())
        .await
        .expect("the stream should terminate rather than hang")
        .unwrap();

    assert!(
        text.contains("event: terminal"),
        "the stream must end with a terminal event; got:\n{text}"
    );
    assert!(text.contains("completed"), "got:\n{text}");

    // And the persisted job agrees.
    let job: Value = client
        .get(format!("{}/api/jobs/{job_id}", s.base))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(job["status"], "completed");
}

#[tokio::test]
async fn an_unknown_job_is_a_404_not_an_empty_stream() {
    let s = start().await;
    let client = reqwest::Client::new();

    for route in ["/api/jobs/no-such-job", "/api/jobs/no-such-job/events"] {
        let response = client
            .get(format!("{}{route}", s.base))
            .bearer_auth(good_token())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 404, "{route}");
    }
}

// ---------------------------------------------------------------------------
// Tool wiring
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_rename_dry_run_returns_a_plan_and_changes_nothing() {
    let s = start().await;
    let client = reqwest::Client::new();

    for name in ["b.jpg", "a.jpg"] {
        std::fs::write(s.root.join(name), b"x").unwrap();
    }

    let response = client
        .post(format!("{}/api/tools/rename/plan", s.base))
        .bearer_auth(good_token())
        .json(&json!({
            "paths": [
                s.root.join("a.jpg").display().to_string(),
                s.root.join("b.jpg").display().to_string(),
            ],
            "date": "20240501",
            "subject": "Lisboa",
            "order": "Numeric",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let plan: Value = response.json().await.unwrap();
    let actions = plan["actions"].as_array().unwrap();
    assert_eq!(actions.len(), 2);
    assert!(actions[0]["target"]
        .as_str()
        .unwrap()
        .ends_with("20240501-Lisboa-01.jpg"));

    // The dry run wrote nothing.
    assert!(s.root.join("a.jpg").exists());
    assert!(!s.root.join("20240501-Lisboa-01.jpg").exists());
}

#[tokio::test]
async fn storage_listing_returns_entries_for_an_allowed_path() {
    let s = start().await;
    let client = reqwest::Client::new();

    std::fs::create_dir(s.root.join("2024")).unwrap();
    std::fs::write(s.root.join("note.jpg"), b"xyz").unwrap();

    let response = client
        .get(format!("{}/api/storage/ls", s.base))
        .query(&[("path", s.root.display().to_string())])
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let entries: Value = response.json().await.unwrap();
    let entries = entries.as_array().unwrap();

    // Directories sort first.
    assert_eq!(entries[0]["name"], "2024");
    assert_eq!(entries[0]["is_dir"], true);
    assert_eq!(entries[1]["name"], "note.jpg");
    assert_eq!(entries[1]["size"], 3);
}

#[tokio::test]
async fn a_request_with_no_paths_is_a_bad_request_not_a_job() {
    let s = start().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/tools/rename/apply", s.base))
        .bearer_auth(good_token())
        .json(&json!({ "paths": [], "order": "Numeric" }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 400);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "bad_request");
}

// ---------------------------------------------------------------------------
// Phase 14 — the roots a folder picker may start from
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_configured_roots_are_reported_so_a_picker_knows_where_to_begin() {
    // G6 refuses every path outside a root, `/` included, so "list the top" is
    // not a question the filesystem can answer — only the configuration can.
    let s = start().await;

    let response = reqwest::Client::new()
        .get(format!("{}/api/storage/roots", s.base))
        .bearer_auth(token_for(ALLOWED_UID, now() + 3600))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let roots: Vec<String> = response.json().await.unwrap();
    assert_eq!(
        roots,
        vec![s.root.display().to_string()],
        "the picker is offered exactly the directories G6 would allow"
    );
}

#[tokio::test]
async fn the_roots_are_not_readable_without_a_token() {
    // The roots name where the library lives on disk. That is not a secret from
    // an invited account, and it is not for anyone else either.
    let s = start().await;

    let response = reqwest::get(format!("{}/api/storage/roots", s.base))
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

/// A scan answers with its table rather than an id.
///
/// It writes nothing, and the rows are the whole point: returning a job id
/// meant every client computed them, counted them and threw them away.
#[tokio::test]
async fn a_date_scan_answers_with_the_rows_themselves() {
    let s = start().await;

    std::fs::write(s.root.join("a.jpg"), b"x").unwrap();
    std::fs::write(s.root.join("b.jpg"), b"y").unwrap();
    std::fs::write(s.root.join("notes.txt"), b"z").unwrap();

    let response = reqwest::Client::new()
        .post(format!("{}/api/tools/dates/scan", s.base))
        .bearer_auth(good_token())
        .json(&json!({ "path": s.root.display().to_string(), "recursive": false }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200, "not 202: there is no job to follow");

    let rows: Vec<Value> = response.json().await.unwrap();
    assert_eq!(rows.len(), 2, "the two media files, not the .txt: {rows:?}");
    for row in &rows {
        assert!(row["name"].is_string());
        assert!(
            row["status"].is_string(),
            "every row carries the state the table shows: {row:?}"
        );
    }
}

/// A date repair preview answers with the plan, not a count.
///
/// MV-9.3 asks somebody to read a clock-offset suggestion before applying it,
/// which needs the resulting date per file. The dry run used to report only
/// how many would move.
#[tokio::test]
async fn a_date_repair_preview_answers_with_what_each_file_would_become() {
    let s = start().await;
    let file = s.root.join("frame.jpg");
    std::fs::write(&file, b"x").unwrap();

    let response = reqwest::Client::new()
        .post(format!("{}/api/tools/dates/plan", s.base))
        .bearer_auth(good_token())
        .json(&json!({
            "paths": [file.display().to_string()],
            "mode": { "Manual": "2024-05-01T12:00:00" },
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200, "a plan, not an accepted job");

    let plan: Value = response.json().await.unwrap();
    let actions = plan["actions"].as_array().unwrap();
    assert_eq!(actions.len(), 1);
    assert!(
        actions[0]["new_date"]
            .as_str()
            .unwrap()
            .starts_with("2024-05-01"),
        "the plan says what the file would become: {actions:?}"
    );
    assert!(plan["skipped"].is_array(), "and what it would not touch");
}

/// The preview writes nothing — the whole reason it is a plan and not a job.
#[tokio::test]
async fn a_date_repair_preview_does_not_touch_the_file() {
    let s = start().await;
    let file = s.root.join("frame.jpg");
    std::fs::write(&file, b"original").unwrap();
    let before = std::fs::metadata(&file).unwrap().modified().unwrap();

    reqwest::Client::new()
        .post(format!("{}/api/tools/dates/plan", s.base))
        .bearer_auth(good_token())
        .json(&json!({
            "paths": [file.display().to_string()],
            "mode": { "Manual": "2024-05-01T12:00:00" },
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(std::fs::read(&file).unwrap(), b"original");
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        before
    );
}

/// §F4's preview answers with the halves, and writes nothing.
///
/// It was specified from the start and reachable from neither build, which
/// left the tool whose thresholds most need judging by eye runnable only
/// blind.
#[tokio::test]
async fn a_split_preview_returns_both_halves_and_writes_nothing() {
    let s = start().await;

    // Two dark-bordered panels with a black divider down the middle: the
    // shape F4 looks for.
    let scan = s.root.join("frame.jpg");
    let mut img = image::RgbImage::from_pixel(400, 300, image::Rgb([200, 180, 160]));
    for y in 0..300 {
        for x in 190..210 {
            img.put_pixel(x, y, image::Rgb([4, 4, 4]));
        }
    }
    image::DynamicImage::ImageRgb8(img).save(&scan).unwrap();

    let before: Vec<_> = std::fs::read_dir(&s.root).unwrap().collect();

    let response = reqwest::Client::new()
        .post(format!("{}/api/tools/split/preview", s.base))
        .bearer_auth(good_token())
        .json(&json!({ "inputs": [scan.display().to_string()] }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();

    for half in ["cropped", "a", "b"] {
        let src = body[half]["src"].as_str().unwrap();
        assert!(
            src.starts_with("data:image/jpeg;base64,"),
            "{half} must arrive ready for an img src, got {src:.40}"
        );
        assert!(body[half]["width"].as_u64().unwrap() > 0);
    }

    let fraction = body["divider_fraction"].as_f64().unwrap();
    assert!(
        (0.4..0.6).contains(&fraction),
        "the divider is down the middle of this fixture; got {fraction}"
    );

    let after: Vec<_> = std::fs::read_dir(&s.root).unwrap().collect();
    assert_eq!(
        before.len(),
        after.len(),
        "a preview writes nothing — that is what makes it a preview"
    );
}

/// F7's dark-edge trim can be turned off.
///
/// It is the tool's only parameter and was reachable from neither build, so a
/// genuinely dark photograph mistaken for a scan border could not be rescued —
/// which is the judgement MV-4.2 asks for.
#[tokio::test]
async fn the_border_trim_can_be_turned_off() {
    let s = start().await;

    // A photograph that is genuinely dark at its edges: the trim would eat it.
    let dark = s.root.join("night.jpg");
    let mut img = image::RgbImage::from_pixel(600, 400, image::Rgb([3, 3, 4]));
    for y in 150..250 {
        for x in 250..350 {
            img.put_pixel(x, y, image::Rgb([240, 230, 200]));
        }
    }
    image::DynamicImage::ImageRgb8(img).save(&dark).unwrap();

    let out = s.root.join("out");
    std::fs::create_dir_all(&out).unwrap();

    let response = reqwest::Client::new()
        .post(format!("{}/api/tools/border", s.base))
        .bearer_auth(good_token())
        .json(&json!({
            "inputs": [dark.display().to_string()],
            "out_dir": out.display().to_string(),
            "trim_dark_edges": false,
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        202,
        "the flag is accepted rather than rejected as an unknown field"
    );
}

/// A folder is expanded for the preview, exactly as the apply expands it.
///
/// It was not: the first input went straight to the decoder, so naming a
/// folder — which the folder picker encourages — answered "Is a directory (os
/// error 21)" from the filesystem, a long way from the cause.
#[tokio::test]
async fn a_split_preview_accepts_a_folder_as_the_apply_does() {
    let s = start().await;

    let folder = s.root.join("roll");
    std::fs::create_dir_all(&folder).unwrap();
    let mut img = image::RgbImage::from_pixel(400, 300, image::Rgb([200, 180, 160]));
    for y in 0..300 {
        for x in 190..210 {
            img.put_pixel(x, y, image::Rgb([4, 4, 4]));
        }
    }
    image::DynamicImage::ImageRgb8(img)
        .save(folder.join("frame.jpg"))
        .unwrap();

    let response = reqwest::Client::new()
        .post(format!("{}/api/tools/split/preview", s.base))
        .bearer_auth(good_token())
        .json(&json!({ "inputs": [folder.display().to_string()] }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200, "a folder is an input, not an error");

    let body: Value = response.json().await.unwrap();
    assert!(
        body["source"].as_str().unwrap().ends_with("frame.jpg"),
        "the preview says which frame it is of: {body:?}"
    );
}

/// A folder with nothing readable says so, rather than failing at the decoder.
#[tokio::test]
async fn a_split_preview_of_an_empty_folder_explains_itself() {
    let s = start().await;
    let folder = s.root.join("empty");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("notes.txt"), b"x").unwrap();

    let response = reqwest::Client::new()
        .post(format!("{}/api/tools/split/preview", s.base))
        .bearer_auth(good_token())
        .json(&json!({ "inputs": [folder.display().to_string()] }))
        .send()
        .await
        .unwrap();

    assert_ne!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    let message = body["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("Nothing here this tool reads"),
        "the message names the cause rather than an errno: {message}"
    );
}

// ---------------------------------------------------------------------------
// Timeline sync (`docs/timeline-sync-plan.md`)
// ---------------------------------------------------------------------------

/// A track document, for the sync tests below.
fn sync_gpx(time: &str, lat: f64, lon: f64) -> String {
    format!(
        "<gpx creator=\"OwnTracks\"><trk><trkseg>\
         <trkpt lat=\"{lat}\" lon=\"{lon}\"><time>{time}</time></trkpt>\
         </trkseg></trk></gpx>"
    )
}

/// The four sync endpoints, in the order the desktop calls them.
///
/// Asserts the claim the feature makes — that a track handed over is *held*
/// afterwards, with its provenance — rather than that four routes answer 200.
#[tokio::test]
async fn a_track_handed_to_the_server_is_in_its_timeline_afterwards() {
    let s = start().await;
    let client = reqwest::Client::new();

    // Empty to begin with, and it says so rather than erroring.
    let inventory: Value = client
        .get(format!("{}/api/timeline/inventory", s.base))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(inventory["tracks"].as_array().unwrap().len(), 0);
    assert_eq!(inventory["deletions"].as_array().unwrap().len(), 0);

    // The Mac hands over a track, with the provenance it carries.
    let transfer = json!({
        "name": "Alexanderplatz",
        "source_path": "(placed on the map)",
        "source": "placed",
        "imported_at": 1_700_000_000_i64,
        "gpx": sync_gpx("2013-05-01T12:00:00Z", 52.521918, 13.413215),
        "decisions": [],
    });
    let accepted = client
        .post(format!("{}/api/timeline/tracks", s.base))
        .bearer_auth(good_token())
        .json(&transfer)
        .send()
        .await
        .unwrap();
    assert_eq!(accepted.status(), 200);
    let result: Value = accepted.json().await.unwrap();
    assert_eq!(result["added"], 1);

    // It is held, it says where it came from, and the fix is on the timeline.
    let tracks: Value = client
        .get(format!("{}/api/tracks", s.base))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(tracks[0]["source"], "placed", "provenance is not laundered");
    assert_eq!(
        tracks[0]["imported_at"], 1_700_000_000_i64,
        "dated as the origin dated it"
    );

    let id = tracks[0]["id"].as_str().unwrap().to_string();

    // And it can be fetched back, text and all, for the other direction.
    let fetched: Value = client
        .get(format!("{}/api/timeline/tracks/{id}", s.base))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fetched["source"], "placed");
    assert!(fetched["gpx"].as_str().unwrap().contains("trkpt"));

    // A deletion from the other machine removes it and is remembered, so the
    // next sync cannot hand it back.
    let deleted = client
        .post(format!("{}/api/timeline/deletions", s.base))
        .bearer_auth(good_token())
        .json(&json!([{ "id": id, "deleted_at": 1_700_000_100_i64 }]))
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 200);

    let after: Value = client
        .get(format!("{}/api/timeline/inventory", s.base))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after["tracks"].as_array().unwrap().len(), 0);
    assert_eq!(after["deletions"][0]["id"], id);
    assert_eq!(after["deletions"][0]["deleted_at"], 1_700_000_100_i64);
}

/// Sync carries geopositions and nothing else.
#[tokio::test]
async fn the_sync_endpoints_expose_the_timeline_and_nothing_else() {
    let s = start().await;
    let client = reqwest::Client::new();

    let inventory: Value = client
        .get(format!("{}/api/timeline/inventory", s.base))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let keys: Vec<&str> = inventory
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    assert_eq!(
        keys,
        vec!["deletions", "tracks"],
        "an inventory is tracks and deletions — no cards, shots, sessions or publishes"
    );
}

/// Every sync route refuses an unauthenticated request (§5.3).
#[tokio::test]
async fn the_sync_endpoints_are_behind_authentication() {
    let s = start().await;
    let client = reqwest::Client::new();

    for (method, path) in [
        ("GET", "/api/timeline/inventory"),
        ("GET", "/api/timeline/tracks/whatever"),
        ("POST", "/api/timeline/tracks"),
        ("POST", "/api/timeline/deletions"),
    ] {
        let request = match method {
            "GET" => client.get(format!("{}{path}", s.base)),
            _ => client.post(format!("{}{path}", s.base)).json(&json!({})),
        };
        let response = request.send().await.unwrap();
        assert_eq!(
            response.status(),
            401,
            "{method} {path} must require a token"
        );
    }
}

// ---------------------------------------------------------------------------
// Positions reported by a phone (`docs/owntracks-plan.md`)
// ---------------------------------------------------------------------------

fn basic(user: &str, token: &str) -> String {
    use std::fmt::Write;
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let raw = format!("{user}:{token}");
    let mut encoded = String::new();
    for chunk in raw.as_bytes().chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        let indices = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        for (position, index) in indices.iter().enumerate() {
            if position <= chunk.len() {
                let _ = write!(encoded, "{}", ALPHABET[*index as usize] as char);
            } else {
                encoded.push('=');
            }
        }
    }
    format!("Basic {encoded}")
}

/// A location report, as OwnTracks posts it.
fn report(tst: i64, lat: f64, lon: f64) -> Value {
    json!({ "_type": "location", "tst": tst, "lat": lat, "lon": lon, "tid": "pg", "batt": 84 })
}

/// **The security properties of the one route open to the internet.**
#[tokio::test]
async fn a_position_report_needs_the_device_credential() {
    with_device("phone", "a-long-random-token");
    let s = start().await;
    let client = reqwest::Client::new();
    let url = format!("{}/api/timeline/owntracks", s.base);

    // No credential.
    let anonymous = client
        .post(&url)
        .json(&report(1, 52.5, 13.4))
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), 401);

    // The wrong token, and the right user.
    let wrong = client
        .post(&url)
        .header("Authorization", basic("phone", "not-the-token"))
        .json(&report(1, 52.5, 13.4))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);

    // A Firebase token is not this credential: the route has one key, and it is
    // not the one every other route uses.
    let firebase = client
        .post(&url)
        .bearer_auth(good_token())
        .json(&report(1, 52.5, 13.4))
        .send()
        .await
        .unwrap();
    assert_eq!(firebase.status(), 401);
}

#[tokio::test]
async fn with_no_credential_configured_the_route_refuses_everything() {
    DEVICE.with(|d| *d.borrow_mut() = None);
    let s = start().await;

    let response = reqwest::Client::new()
        .post(format!("{}/api/timeline/owntracks", s.base))
        .header("Authorization", basic("phone", "anything"))
        .json(&report(1, 52.5, 13.4))
        .send()
        .await
        .unwrap();

    // Not configured is not "allow".
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn a_reported_position_is_stored_and_becomes_that_days_track() {
    with_device("phone", "a-long-random-token");
    let s = start().await;
    let client = reqwest::Client::new();
    let url = format!("{}/api/timeline/owntracks", s.base);

    // 2026-09-13, noon and one in the afternoon UTC.
    let noon = 1_789_300_800_i64;
    for (offset, lat, lon) in [(0, 52.5, 13.4), (3_600, 52.6, 13.5)] {
        let accepted = client
            .post(&url)
            .header("Authorization", basic("phone", "a-long-random-token"))
            .json(&report(noon + offset, lat, lon))
            .send()
            .await
            .unwrap();
        assert_eq!(accepted.status(), 200);
        // OwnTracks reads the reply as commands; an empty list is "carry on".
        let body: Value = accepted.json().await.unwrap();
        assert_eq!(body, json!([]));
    }

    // Anything that is not a location is accepted and kept out of the timeline:
    // refusing would have the phone retry forever.
    let transition = client
        .post(&url)
        .header("Authorization", basic("phone", "a-long-random-token"))
        .json(&json!({ "_type": "transition", "event": "enter", "desc": "home" }))
        .send()
        .await
        .unwrap();
    assert_eq!(transition.status(), 200);

    // The screen can see it before the day ends: a phone reporting perfectly
    // and a phone reporting nothing look identical otherwise.
    let view: Value = client
        .post(format!("{}/api/timeline", s.base))
        .bearer_auth(good_token())
        .json(&json!({ "from": noon - 3_600, "to": noon + 7_200, "offset_minutes": 0 }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(view["pending"], 2, "two fixes waiting for the day to end");
    assert_eq!(
        view["points"].as_array().unwrap().len(),
        0,
        "and none in the timeline yet"
    );

    // Nothing is in the timeline yet: the day is not over.
    let ledger = phototools_core::ledger::Ledger::open(&s.database).unwrap();
    assert_eq!(ledger.tracks().unwrap().len(), 0);
    assert_eq!(ledger.device_fixes_between(0, i64::MAX).unwrap().len(), 2);

    // Once it is, those fixes are a track like any other, with a document.
    let rolled = phototools_core::tools::geotag::owntracks::roll_up_due(
        &ledger,
        noon + 2 * 86_400,
        0,
        phototools_core::tools::geotag::owntracks::GRACE_SECONDS,
    )
    .unwrap();

    assert_eq!(rolled.len(), 1);
    assert_eq!(rolled[0].name, "OwnTracks 2026-09-13");
    assert_eq!(ledger.points_between(0, i64::MAX).unwrap().len(), 2);
    assert!(ledger.track_text(&rolled[0].track_id).unwrap().is_some());
}
