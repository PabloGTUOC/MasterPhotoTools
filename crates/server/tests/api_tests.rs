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
    /// Where the desktop writes and the server verifies (F16).
    staging: PathBuf,
    database: PathBuf,
    _temp: tempfile::TempDir,
}

async fn start() -> TestServer {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    std::fs::create_dir(&root).unwrap();

    let staging = temp.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();

    let config = Config {
        roots: vec![root.canonicalize().unwrap()],
        staging_dir: staging,
        thresholds: Thresholds::default(),
        database: temp.path().join("ledger.sqlite3"),
    };

    let auth_config = auth::AuthConfig {
        project_id: PROJECT.into(),
        allowed_uids: vec![ALLOWED_UID.into()],
        admin_token: None,
        keys: auth::KeyStore::offline(),
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
        staging: temp.path().join("staging"),
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
// Phase 11 — the handoff (F16)
// ---------------------------------------------------------------------------
//
// These run against the real routes over real HTTP, so the manifest crosses a
// socket and is deserialised exactly as the desktop would send it.

use phototools_core::ingest::{Handoff, HandoffItem, Manifest};

/// A manifest built from files in a temporary directory, as the desktop builds
/// one.
fn handoff_of(dir: &std::path::Path, files: &[(&str, &[u8])]) -> Handoff {
    std::fs::create_dir_all(dir).unwrap();
    let items: Vec<HandoffItem> = files
        .iter()
        .map(|(stem, bytes)| {
            let path = dir.join(format!("{stem}.jpg"));
            std::fs::write(&path, bytes).unwrap();
            HandoffItem {
                stem: (*stem).into(),
                source_sha256: format!("source-of-{stem}"),
                derived: path,
                width: 3000,
                height: 2000,
                capture: None,
            }
        })
        .collect();
    Handoff::prepare("pending", "card-1", &items).unwrap()
}

async fn open_session(s: &TestServer, manifest: &Manifest) -> Value {
    let response = reqwest::Client::new()
        .post(format!("{}/api/ingest/sessions", s.base))
        .bearer_auth(good_token())
        .json(manifest)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "opening a session");
    response.json().await.unwrap()
}

/// Call `ready` and wait for the verification job it starts.
async fn mark_ready(s: &TestServer, session_id: &str) -> Value {
    let client = reqwest::Client::new();
    let accepted: Value = client
        .post(format!("{}/api/ingest/sessions/{session_id}/ready", s.base))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let job_id = accepted["job_id"].as_str().expect("ready must start a job");

    for _ in 0..100 {
        let job: Value = client
            .get(format!("{}/api/jobs/{job_id}", s.base))
            .bearer_auth(good_token())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if job["status"] == "completed" || job["status"] == "failed" {
            assert_eq!(job["status"], "completed", "verification job: {job}");
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    client
        .get(format!("{}/api/ingest/sessions/{session_id}/shots", s.base))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// Copy a manifest's files into staging, as the desktop's writer does.
fn deliver(s: &TestServer, handoff: &Handoff, only: Option<&[&str]>) {
    std::fs::create_dir_all(&s.staging).unwrap();
    for entry in &handoff.manifest().entries {
        if only.is_some_and(|stems| !stems.contains(&entry.stem.as_str())) {
            continue;
        }
        let local = handoff.local_path(&entry.file_name).unwrap();
        std::fs::copy(local, s.staging.join(&entry.file_name)).unwrap();
    }
}

#[tokio::test]
async fn the_handoff_routes_refuse_an_anonymous_request() {
    let s = start().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/ingest/sessions", s.base))
        .json(&json!({"session_id":"x","card_id":"y","created_at":0,"entries":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);

    for route in [
        "/api/ingest/sessions/abc/ready",
        "/api/ingest/sessions/abc/shots",
    ] {
        let response = if route.ends_with("ready") {
            client.post(format!("{}{route}", s.base)).send().await
        } else {
            client.get(format!("{}{route}", s.base)).send().await
        };
        assert_eq!(response.unwrap().status(), 401, "{route}");
    }
}

/// **Phase 11 acceptance.** A fresh card is all `send`; the same card ingested
/// again after publishing transfers nothing and publishes nothing (F16).
#[tokio::test]
async fn re_ingesting_a_published_card_transfers_nothing() {
    let s = start().await;
    let temp = tempfile::tempdir().unwrap();
    let handoff = handoff_of(temp.path(), &[("IMG_0001", b"one"), ("IMG_0002", b"two")]);

    let plan = open_session(&s, handoff.manifest()).await;
    let dispositions: Vec<&str> = plan["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["disposition"].as_str().unwrap())
        .collect();
    assert_eq!(dispositions, vec!["send", "send"]);

    deliver(&s, &handoff, None);
    let shots = mark_ready(&s, plan["session_id"].as_str().unwrap()).await;
    assert_eq!(shots["state"], "verified");

    // Phase 12 will do this when Google Photos accepts each photograph.
    {
        let ledger = phototools_core::ledger::Ledger::open(&s.database).unwrap();
        for entry in &handoff.manifest().entries {
            ledger
                .record_published(
                    &entry.source_sha256,
                    &entry.stem,
                    &entry.derived_sha256,
                    "session-1",
                    Some("media-item"),
                )
                .unwrap();
        }
    }

    let second = open_session(&s, handoff.manifest()).await;
    let dispositions: Vec<&str> = second["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["disposition"].as_str().unwrap())
        .collect();
    assert_eq!(
        dispositions,
        vec!["already_published", "already_published"],
        "a card already processed must ask for nothing"
    );
}

/// **Phase 11 acceptance.** A truncated staged file is caught by hash and asked
/// for again (specification §2.3).
#[tokio::test]
async fn a_truncated_staged_file_is_reported_for_recopy() {
    let s = start().await;
    let temp = tempfile::tempdir().unwrap();
    let handoff = handoff_of(
        temp.path(),
        &[("IMG_0001", b"a whole photograph"), ("IMG_0002", b"two")],
    );

    let plan = open_session(&s, handoff.manifest()).await;
    let session_id = plan["session_id"].as_str().unwrap().to_string();

    deliver(&s, &handoff, None);
    // The interrupted copy: right name, not all of it.
    let damaged = &handoff.manifest().entries[0];
    std::fs::write(s.staging.join(&damaged.file_name), b"a whole").unwrap();

    let shots = mark_ready(&s, &session_id).await;

    assert_eq!(shots["state"], "incomplete");
    assert_eq!(shots["report"]["recopy"].as_array().unwrap().len(), 1);
    assert_eq!(shots["report"]["recopy"][0]["reason"], "short_file");
    assert_eq!(shots["report"]["recopy"][0]["stem"], "IMG_0001");

    // And the review grid says so per shot.
    let arrivals: Vec<&str> = shots["shots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|shot| shot["arrival"].as_str().unwrap())
        .collect();
    assert_eq!(arrivals, vec!["short_file", "verified"]);

    // Recopying it whole clears the session.
    deliver(&s, &handoff, Some(&["IMG_0001"]));
    let shots = mark_ready(&s, &session_id).await;
    assert_eq!(shots["state"], "verified");
    assert!(shots["report"]["recopy"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_file_that_never_arrived_is_reported_as_missing() {
    let s = start().await;
    let temp = tempfile::tempdir().unwrap();
    let handoff = handoff_of(temp.path(), &[("IMG_0001", b"one"), ("IMG_0002", b"two")]);

    let plan = open_session(&s, handoff.manifest()).await;
    deliver(&s, &handoff, Some(&["IMG_0002"]));

    let shots = mark_ready(&s, plan["session_id"].as_str().unwrap()).await;

    assert_eq!(shots["report"]["recopy"][0]["reason"], "missing");
    assert_eq!(shots["report"]["verified"].as_array().unwrap().len(), 1);
}

/// A manifest naming a file outside the staging directory is refused before
/// anything is read. The one place in this protocol where a client names a file.
#[tokio::test]
async fn a_manifest_that_names_a_path_outside_staging_is_refused() {
    let s = start().await;
    let temp = tempfile::tempdir().unwrap();
    let handoff = handoff_of(temp.path(), &[("IMG_0001", b"one")]);

    let mut manifest = handoff.manifest().clone();
    manifest.entries[0].file_name = "../../../etc/cron.d/pwn".into();

    let response = reqwest::Client::new()
        .post(format!("{}/api/ingest/sessions", s.base))
        .bearer_auth(good_token())
        .json(&manifest)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 403);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "path_not_allowed");
}

#[tokio::test]
async fn an_unknown_session_is_a_404() {
    let s = start().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!(
            "{}/api/ingest/sessions/no-such-session/ready",
            s.base
        ))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);

    let response = client
        .get(format!(
            "{}/api/ingest/sessions/no-such-session/shots",
            s.base
        ))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
}

/// The session id is the server's, not the client's. A client that could name
/// its own session could name one another client is using.
#[tokio::test]
async fn the_server_mints_the_session_id() {
    let s = start().await;
    let temp = tempfile::tempdir().unwrap();
    let handoff = handoff_of(temp.path(), &[("IMG_0001", b"one")]);

    let mut manifest = handoff.manifest().clone();
    manifest.session_id = "chosen-by-the-client".into();

    let plan = open_session(&s, &manifest).await;

    assert_ne!(plan["session_id"], "chosen-by-the-client");
    assert!(plan["session_id"].as_str().unwrap().len() >= 32);
}

/// Before `ready` runs there is no report, and the shots say so rather than
/// claiming anything about files nobody has looked at.
#[tokio::test]
async fn shots_are_awaiting_until_verification_has_run() {
    let s = start().await;
    let temp = tempfile::tempdir().unwrap();
    let handoff = handoff_of(temp.path(), &[("IMG_0001", b"one")]);

    let plan = open_session(&s, handoff.manifest()).await;
    let shots: Value = reqwest::Client::new()
        .get(format!(
            "{}/api/ingest/sessions/{}/shots",
            s.base,
            plan["session_id"].as_str().unwrap()
        ))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(shots["state"], "open");
    assert!(shots["report"].is_null());
    assert_eq!(shots["shots"][0]["arrival"], "awaiting");
    assert_eq!(shots["shots"][0]["stem"], "IMG_0001");
}

// ---------------------------------------------------------------------------
// Phase 12 — Google Photos (F15)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_google_connector_routes_refuse_an_anonymous_request() {
    let s = start().await;
    let client = reqwest::Client::new();

    for route in [
        "/api/connectors/google/status",
        "/api/connectors/google/callback?code=x&state=y",
    ] {
        let response = client
            .get(format!("{}{route}", s.base))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401, "{route}");
    }

    for route in [
        "/api/connectors/google/connect",
        "/api/connectors/google/disconnect",
    ] {
        let response = client
            .post(format!("{}{route}", s.base))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401, "{route}");
    }

    let response = client
        .post(format!("{}/api/ingest/sessions/abc/publish", s.base))
        .json(&json!({"dry_run": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
}

/// An unconfigured connector is a state to show, not a server error. The web UI
/// needs something to render on a fresh deployment.
#[tokio::test]
async fn an_unconfigured_google_connector_reports_itself_rather_than_failing() {
    let s = start().await;

    let response = reqwest::Client::new()
        .get(format!("{}/api/connectors/google/status", s.base))
        .bearer_auth(good_token())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["connected"], false);
    assert!(body["detail"].as_str().unwrap().contains("not configured"));
}

/// **Phase 12 acceptance**, over HTTP: publish is refused without a dry run, and
/// refused before it can become a job.
#[tokio::test]
async fn publishing_without_a_dry_run_is_refused_over_http() {
    let s = start().await;
    let temp = tempfile::tempdir().unwrap();
    let handoff = handoff_of(temp.path(), &[("IMG_0001", b"one")]);
    let plan = open_session(&s, handoff.manifest()).await;
    let session_id = plan["session_id"].as_str().unwrap();

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/ingest/sessions/{session_id}/publish",
            s.base
        ))
        .bearer_auth(good_token())
        .json(&json!({"dry_run": false}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 400, "not a job that fails later");
    let body: Value = response.json().await.unwrap();
    assert!(body["message"].as_str().unwrap().contains("no dry run"));
}

#[tokio::test]
async fn a_dry_run_returns_a_plan_and_records_that_it_was_reviewed() {
    let s = start().await;
    let temp = tempfile::tempdir().unwrap();
    let handoff = handoff_of(
        temp.path(),
        &[
            ("IMG_0001", b"one"),
            ("IMG_0002", b"two"),
            ("IMG_0003", b"three"),
        ],
    );

    let plan = open_session(&s, handoff.manifest()).await;
    let session_id = plan["session_id"].as_str().unwrap().to_string();
    deliver(&s, &handoff, None);
    mark_ready(&s, &session_id).await;

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/ingest/sessions/{session_id}/publish",
            s.base
        ))
        .bearer_auth(good_token())
        .json(&json!({"dry_run": true}))
        .send()
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        200,
        "a dry run answers inline, not as a job"
    );
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["items"].as_array().unwrap().len(), 3);
    assert_eq!(body["upload_requests"], 3);
    assert_eq!(body["batch_create_requests"], 1);
    assert!(body["total_bytes"].as_u64().unwrap() > 0);

    // Having been reviewed, the session is no longer refused for that reason.
    // It now fails on the connector instead, which is the next thing wrong.
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/ingest/sessions/{session_id}/publish",
            s.base
        ))
        .bearer_auth(good_token())
        .json(&json!({"dry_run": false}))
        .send()
        .await
        .unwrap();
    let body: Value = response.json().await.unwrap();
    assert!(
        !body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("no dry run"),
        "got {body}"
    );
}

/// A session whose staged files were never verified publishes nothing, and the
/// dry run says so rather than quietly listing fewer items.
#[tokio::test]
async fn a_dry_run_on_an_unverified_session_plans_nothing() {
    let s = start().await;
    let temp = tempfile::tempdir().unwrap();
    let handoff = handoff_of(temp.path(), &[("IMG_0001", b"one")]);
    let plan = open_session(&s, handoff.manifest()).await;

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/ingest/sessions/{}/publish",
            s.base,
            plan["session_id"].as_str().unwrap()
        ))
        .bearer_auth(good_token())
        .json(&json!({"dry_run": true}))
        .send()
        .await
        .unwrap();

    let body: Value = response.json().await.unwrap();
    assert!(body["items"].as_array().unwrap().is_empty());
    assert_eq!(body["skipped"].as_array().unwrap().len(), 1);
    assert!(body["skipped"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("not been verified"));
}

#[tokio::test]
async fn publishing_an_unknown_session_is_a_404() {
    let s = start().await;

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/ingest/sessions/no-such-session/publish",
            s.base
        ))
        .bearer_auth(good_token())
        .json(&json!({"dry_run": true}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 404);
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
