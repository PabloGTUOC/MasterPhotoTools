//! Phase 14: the server serves the built web front end (specification §2.2).
//!
//! The interesting cases are the two boundaries — a client-side route must
//! reach the document, and an unknown `/api` path must not — so both are driven
//! over real HTTP rather than asserted against the router's shape.

use phototools_core::config::{Config, Thresholds};
use phototools_server::{auth, build_router_with_web, jobs, AppState};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// A server with a front end laid out the way Vite builds one.
struct Fixture {
    base: String,
    _temp: tempfile::TempDir,
}

async fn start(with_web: bool) -> Fixture {
    let temp = tempfile::tempdir().unwrap();

    let root = temp.path().join("library");
    std::fs::create_dir_all(&root).unwrap();

    let web = temp.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(
        web.join("index.html"),
        "<!doctype html><title>PhotoTools</title>",
    )
    .unwrap();
    std::fs::write(web.join("assets/index-abc123.js"), "export const x = 1;\n").unwrap();

    let config = Config {
        roots: vec![root.canonicalize().unwrap()],
        staging_dir: temp.path().join("staging"),
        thresholds: Thresholds::default(),
        database: temp.path().join("ledger.sqlite3"),
    };

    let ledger = phototools_core::ledger::Ledger::open(&config.database).unwrap();

    let state = AppState {
        config: Arc::new(config),
        auth: Arc::new(auth::AuthConfig {
            project_id: "phototools-test".into(),
            allowed_uids: vec!["photographer-1".into()],
            admin_token: None,
            // No test here presents a token, so the store is never consulted.
            // `offline` is the one that reaches no network to fill itself.
            keys: auth::KeyStore::offline(),
        }),
        jobs: Arc::new(jobs::JobManager::new(ledger)),
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let web_root: Option<PathBuf> = with_web.then_some(web);

    tokio::spawn(async move {
        axum::serve(listener, build_router_with_web(state, web_root))
            .await
            .unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    Fixture {
        base: format!("http://127.0.0.1:{port}"),
        _temp: temp,
    }
}

#[tokio::test]
async fn the_document_is_served_at_the_root() {
    let f = start(true).await;
    let response = reqwest::get(format!("{}/", f.base)).await.unwrap();

    assert_eq!(response.status(), 200);
    assert!(response.text().await.unwrap().contains("PhotoTools"));
}

#[tokio::test]
async fn a_client_side_route_is_answered_with_the_document_not_a_404() {
    // /publish is a Vue route, not a file. Opening it directly — or reloading
    // on it — has to reach index.html or every route but / breaks.
    let f = start(true).await;
    let response = reqwest::get(format!("{}/publish", f.base)).await.unwrap();

    assert_eq!(response.status(), 200, "a client-side route must not 404");
    assert!(response.text().await.unwrap().contains("PhotoTools"));
}

#[tokio::test]
async fn an_asset_is_served_from_disk_with_its_own_type() {
    let f = start(true).await;
    let response = reqwest::get(format!("{}/assets/index-abc123.js", f.base))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        content_type.contains("javascript"),
        "a bundle served as {content_type} is refused by the browser"
    );
    assert!(response.text().await.unwrap().contains("export const x"));
}

#[tokio::test]
async fn an_unknown_api_path_is_a_json_404_rather_than_the_page() {
    // The failure this guards against is silent: a client calling a withdrawn
    // or misspelled endpoint would get 200 and a page of HTML, and parse it as
    // an empty result rather than as the error it is.
    let f = start(true).await;
    let response = reqwest::get(format!("{}/api/no-such-endpoint", f.base))
        .await
        .unwrap();

    assert_eq!(response.status(), 404);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["code"], "not_found");
}

#[tokio::test]
async fn the_health_route_still_answers_with_the_front_end_mounted() {
    // /api/health is a static segment and the front-end catch-all is a
    // wildcard; this is the assertion that the router still prefers the former.
    let f = start(true).await;
    let response = reqwest::get(format!("{}/api/health", f.base))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn with_no_front_end_configured_the_api_is_unaffected() {
    // A desktop-only deployment, and the development case: Vite serves the
    // front end and proxies /api back here.
    let f = start(false).await;

    let health = reqwest::get(format!("{}/api/health", f.base))
        .await
        .unwrap();
    assert_eq!(health.status(), 200);

    let root = reqwest::get(format!("{}/", f.base)).await.unwrap();
    assert_eq!(root.status(), 404, "nothing is claiming to serve a page");
}
