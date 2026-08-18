//! Phase 12 acceptance: publishing to Google Photos (F15, specification §6).
//!
//! **No test here reaches Google.** The API is a trait; every test drives a fake
//! that records what it was asked and answers however the test needs. The one
//! test of the real HTTP client points it at a socket on `127.0.0.1`.

mod fixtures;

use fixtures::Fixtures;
use phototools_core::error::Error;
use phototools_core::ingest::handoff::{
    ArrivalReport, Handoff, HandoffItem, Manifest, Recopy, RecopyReason, SessionPlan,
};
use phototools_core::jobs::InMemoryProgress;
use phototools_core::ledger::Ledger;
use phototools_core::publish::api::MAX_RATE_LIMIT_RETRIES;
use phototools_core::publish::{
    batch_count, dry_run, publishable, AccessTokens, ApiError, Connector, CreateResult,
    HttpPhotosApi, NewMediaItem, OAuthConfig, PhotosApi, PublishPlan, Publisher, Sleeper,
    TokenCipher, TokenEndpoint, TokenError, TokenResponse, MAX_BATCH, RATE_LIMIT_FLOOR,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

/// A Google that does whatever the test says, and remembers being asked.
#[derive(Default)]
struct FakeApi {
    uploads: Mutex<Vec<String>>,
    /// One entry per `batchCreate`, holding that call's item count.
    batches: Mutex<Vec<usize>>,
    /// Answers to give to successive `batchCreate` calls; the last repeats.
    create_script: Mutex<Vec<Result<(), ApiError>>>,
    /// Answers to give to successive uploads; the last repeats.
    upload_script: Mutex<Vec<Result<(), ApiError>>>,
}

impl FakeApi {
    fn scripted_creates(script: Vec<Result<(), ApiError>>) -> Self {
        Self {
            create_script: Mutex::new(script),
            ..Default::default()
        }
    }

    fn next(script: &Mutex<Vec<Result<(), ApiError>>>) -> Result<(), ApiError> {
        let mut queue = script.lock().unwrap();
        if queue.len() > 1 {
            queue.remove(0)
        } else {
            queue.first().cloned().unwrap_or(Ok(()))
        }
    }

    fn batch_sizes(&self) -> Vec<usize> {
        self.batches.lock().unwrap().clone()
    }

    fn upload_count(&self) -> usize {
        self.uploads.lock().unwrap().len()
    }
}

impl PhotosApi for FakeApi {
    fn upload(&self, _token: &str, path: &Path, _name: &str) -> Result<String, ApiError> {
        Self::next(&self.upload_script)?;
        let token = format!(
            "upload-token-{}",
            path.file_name().unwrap().to_string_lossy()
        );
        self.uploads.lock().unwrap().push(token.clone());
        Ok(token)
    }

    fn batch_create(
        &self,
        _token: &str,
        items: &[NewMediaItem],
    ) -> Result<Vec<CreateResult>, ApiError> {
        self.batches.lock().unwrap().push(items.len());
        Self::next(&self.create_script)?;

        Ok(items
            .iter()
            .map(|item| CreateResult::Created {
                media_item_id: format!("media-{}", item.upload_token),
            })
            .collect())
    }
}

/// A token source that never talks to anybody.
struct FixedToken {
    invalidations: Mutex<u32>,
    /// When set, `access_token` fails with this — the dead-grant case.
    dead: Option<String>,
    calls: Mutex<u32>,
}

impl FixedToken {
    fn working() -> Self {
        Self {
            invalidations: Mutex::new(0),
            dead: None,
            calls: Mutex::new(0),
        }
    }

    fn dead(reason: &str) -> Self {
        Self {
            invalidations: Mutex::new(0),
            dead: Some(reason.into()),
            calls: Mutex::new(0),
        }
    }

    fn calls(&self) -> u32 {
        *self.calls.lock().unwrap()
    }
}

impl AccessTokens for FixedToken {
    fn access_token(&self) -> Result<String, Error> {
        *self.calls.lock().unwrap() += 1;
        match &self.dead {
            Some(reason) => Err(Error::Config(reason.clone())),
            None => Ok("access-token".into()),
        }
    }

    fn invalidate(&self) {
        *self.invalidations.lock().unwrap() += 1;
    }
}

/// A sleeper that records rather than sleeps.
///
/// The 30-second floor is asserted by what the code *asked* for. A test that
/// actually waited thirty seconds is a test that gets deleted.
#[derive(Default)]
struct RecordingSleeper {
    waits: Mutex<Vec<Duration>>,
}

impl RecordingSleeper {
    fn waits(&self) -> Vec<Duration> {
        self.waits.lock().unwrap().clone()
    }
}

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) {
        self.waits.lock().unwrap().push(duration);
    }
}

// ---------------------------------------------------------------------------
// A session, staged and verified, ready to publish
// ---------------------------------------------------------------------------

struct Session {
    _f: Fixtures,
    staging: PathBuf,
    manifest: Manifest,
    plan: SessionPlan,
    report: ArrivalReport,
    ledger: Ledger,
}

fn session_of(count: usize) -> Session {
    let f = Fixtures::new();
    let derived = f.path().join("derived");
    let staging = f.path().join("staging");
    std::fs::create_dir_all(&derived).unwrap();
    std::fs::create_dir_all(&staging).unwrap();

    let items: Vec<HandoffItem> = (0..count)
        .map(|i| {
            let stem = format!("IMG_{i:04}");
            let path = derived.join(format!("{stem}.jpg"));
            std::fs::write(&path, format!("photograph number {i}")).unwrap();
            HandoffItem {
                stem,
                source_sha256: format!("source-{i:04}"),
                derived: path,
                width: 3000,
                height: 2000,
                capture: None,
            }
        })
        .collect();

    let handoff = Handoff::prepare("session-1", "card-1", &items).unwrap();
    let ledger = Ledger::open_in_memory().unwrap();
    let plan =
        phototools_core::ingest::handoff::decide(handoff.manifest(), &ledger, &staging).unwrap();

    // Deliver every file, as the desktop's writer would.
    for entry in &handoff.manifest().entries {
        std::fs::copy(
            handoff.local_path(&entry.file_name).unwrap(),
            staging.join(&entry.file_name),
        )
        .unwrap();
    }

    let report = phototools_core::ingest::handoff::verify_arrivals(
        handoff.manifest(),
        &plan,
        &staging,
        &InMemoryProgress::new(),
    );
    assert!(report.complete(), "the fixture must start verified");

    // The session row has to exist for the dry run to be recorded against it.
    ledger
        .open_session(
            &plan.session_id,
            "card-1",
            &serde_json::to_string(handoff.manifest()).unwrap(),
            &serde_json::to_string(&plan).unwrap(),
        )
        .unwrap();

    Session {
        manifest: handoff.manifest().clone(),
        plan,
        report,
        staging,
        ledger,
        _f: f,
    }
}

impl Session {
    fn plan_after_dry_run(&self) -> PublishPlan {
        dry_run(&self.manifest, &self.plan, Some(&self.report), &self.ledger).unwrap()
    }

    fn publisher<'a>(
        &'a self,
        api: &'a dyn PhotosApi,
        tokens: &'a dyn AccessTokens,
        sleeper: &'a dyn Sleeper,
    ) -> Publisher<'a> {
        Publisher {
            ledger: &self.ledger,
            api,
            tokens,
            sleeper,
            staging_dir: self.staging.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Batching — §6.1's limit of fifty
// ---------------------------------------------------------------------------

/// **Phase 12 acceptance.** 120 items produce exactly 3 `batchCreate` calls.
#[test]
fn a_hundred_and_twenty_items_produce_exactly_three_batch_create_calls() {
    let s = session_of(120);
    let api = FakeApi::default();
    let tokens = FixedToken::working();
    let sleeper = RecordingSleeper::default();

    let plan = s.plan_after_dry_run();
    assert_eq!(plan.batch_create_requests, 3, "the dry run must say so too");

    let outcome = s
        .publisher(&api, &tokens, &sleeper)
        .publish(&plan, &InMemoryProgress::new())
        .unwrap();

    assert_eq!(api.batch_sizes(), vec![50, 50, 20]);
    assert_eq!(outcome.created, 120);
    assert_eq!(api.upload_count(), 120, "one upload per photograph");
    assert!(outcome.complete());
}

#[test]
fn batch_counts_are_right_at_the_boundaries() {
    assert_eq!(batch_count(0), 0);
    assert_eq!(batch_count(1), 1);
    assert_eq!(batch_count(MAX_BATCH), 1);
    assert_eq!(batch_count(MAX_BATCH + 1), 2);
    assert_eq!(batch_count(120), 3);
}

#[test]
fn no_batch_ever_exceeds_the_api_limit() {
    let s = session_of(101);
    let api = FakeApi::default();
    let tokens = FixedToken::working();
    let sleeper = RecordingSleeper::default();

    s.publisher(&api, &tokens, &sleeper)
        .publish(&s.plan_after_dry_run(), &InMemoryProgress::new())
        .unwrap();

    assert!(api.batch_sizes().iter().all(|n| *n <= MAX_BATCH));
    assert_eq!(api.batch_sizes().iter().sum::<usize>(), 101);
}

// ---------------------------------------------------------------------------
// The dry run — §9.2 rule 3
// ---------------------------------------------------------------------------

/// **Phase 12 acceptance.** Publish is refused if no dry run has been performed.
#[test]
fn publishing_without_a_dry_run_is_refused() {
    let s = session_of(3);
    let api = FakeApi::default();
    let tokens = FixedToken::working();
    let sleeper = RecordingSleeper::default();

    // A plan built by hand, as a caller skipping the dry run would.
    let (items, skipped) = publishable(&s.manifest, &s.plan, Some(&s.report));
    let forged = PublishPlan {
        session_id: s.plan.session_id.clone(),
        items,
        skipped,
        ..Default::default()
    };

    let err = s
        .publisher(&api, &tokens, &sleeper)
        .publish(&forged, &InMemoryProgress::new())
        .unwrap_err();

    assert!(err.to_string().contains("no dry run"), "got {err}");
    assert_eq!(api.upload_count(), 0, "nothing may be uploaded");
    assert!(api.batch_sizes().is_empty(), "and nothing created");
}

#[test]
fn a_dry_run_reaches_google_for_nothing_at_all() {
    let s = session_of(120);
    let api = FakeApi::default();

    let plan = s.plan_after_dry_run();

    assert_eq!(plan.items.len(), 120);
    assert_eq!(plan.upload_requests, 120);
    assert_eq!(plan.batch_create_requests, 3);
    assert!(plan.total_bytes > 0);
    assert_eq!(api.upload_count(), 0);
    assert!(api.batch_sizes().is_empty());
}

#[test]
fn the_dry_run_requirement_is_recorded_in_the_database_not_in_memory() {
    // The API cannot delete, so a safeguard a restart forgets is no safeguard.
    let s = session_of(2);
    assert!(s.ledger.dry_run_at(&s.plan.session_id).unwrap().is_none());

    let plan = s.plan_after_dry_run();
    assert!(s.ledger.dry_run_at(&s.plan.session_id).unwrap().is_some());

    // A publisher built fresh — as it would be after a restart — is satisfied.
    let api = FakeApi::default();
    let tokens = FixedToken::working();
    let sleeper = RecordingSleeper::default();
    let outcome = s
        .publisher(&api, &tokens, &sleeper)
        .publish(&plan, &InMemoryProgress::new())
        .unwrap();

    assert_eq!(outcome.created, 2);
}

#[test]
fn a_dry_run_will_not_publish_a_file_that_failed_verification() {
    // §9.2 invariant 6: a file that is not known to be intact is not published.
    let s = session_of(3);
    let mut report = s.report.clone();
    let dropped = report.verified.remove(1);
    report.recopy.push(Recopy {
        stem: "IMG_0001".into(),
        file_name: dropped,
        reason: RecopyReason::ShortFile,
    });

    let plan = dry_run(&s.manifest, &s.plan, Some(&report), &s.ledger).unwrap();

    assert_eq!(plan.items.len(), 2);
    assert_eq!(plan.skipped.len(), 1);
    assert!(
        plan.skipped[0].reason.contains("short_file"),
        "{:?}",
        plan.skipped
    );
}

#[test]
fn a_session_that_was_never_verified_publishes_nothing() {
    let s = session_of(3);
    let plan = dry_run(&s.manifest, &s.plan, None, &s.ledger).unwrap();

    assert!(plan.items.is_empty());
    assert_eq!(plan.skipped.len(), 3);
    assert!(plan.skipped[0].reason.contains("not been verified"));
}

// ---------------------------------------------------------------------------
// The state machine — §6.3
// ---------------------------------------------------------------------------

/// **Phase 12 acceptance.** A simulated timeout after upload but before create
/// resumes without duplicating.
#[test]
fn a_timeout_after_upload_resumes_at_create_without_uploading_again() {
    let s = session_of(4);
    let sleeper = RecordingSleeper::default();
    let tokens = FixedToken::working();
    let plan = s.plan_after_dry_run();

    // First run: uploads succeed, the create is refused outright.
    let first_api = FakeApi::scripted_creates(vec![Err(ApiError::Refused {
        status: 503,
        detail: "backend unavailable".into(),
    })]);
    let first = s
        .publisher(&first_api, &tokens, &sleeper)
        .publish(&plan, &InMemoryProgress::new())
        .unwrap();

    assert_eq!(first_api.upload_count(), 4);
    assert_eq!(first.created, 0);
    assert_eq!(first.uploaded, 4);

    // Second run: the tokens are held, so nothing is uploaded a second time.
    let second_api = FakeApi::default();
    let second = s
        .publisher(&second_api, &tokens, &sleeper)
        .publish(&plan, &InMemoryProgress::new())
        .unwrap();

    assert_eq!(
        second_api.upload_count(),
        0,
        "a held upload token must not be thrown away and re-earned"
    );
    assert_eq!(second_api.batch_sizes(), vec![4]);
    assert_eq!(second.created, 4);
    assert!(second.complete());
}

#[test]
fn a_photograph_already_created_is_not_published_a_second_time() {
    let s = session_of(3);
    let sleeper = RecordingSleeper::default();
    let tokens = FixedToken::working();
    let plan = s.plan_after_dry_run();

    let first = FakeApi::default();
    s.publisher(&first, &tokens, &sleeper)
        .publish(&plan, &InMemoryProgress::new())
        .unwrap();
    assert_eq!(first.batch_sizes(), vec![3]);

    // Running the same publish again must do nothing at all.
    let second = FakeApi::default();
    let outcome = s
        .publisher(&second, &tokens, &sleeper)
        .publish(&plan, &InMemoryProgress::new())
        .unwrap();

    assert_eq!(second.upload_count(), 0);
    assert!(second.batch_sizes().is_empty(), "no second batchCreate");
    assert_eq!(outcome.created, 0);
    assert_eq!(outcome.already_created, 3);
}

/// A create that was sent and never answered is the case `batchCreate`'s lack of
/// idempotency actually bites. It must not be retried.
#[test]
fn a_create_whose_answer_never_arrived_is_left_unconfirmed_not_retried() {
    let s = session_of(3);
    let sleeper = RecordingSleeper::default();
    let tokens = FixedToken::working();
    let plan = s.plan_after_dry_run();

    let api = FakeApi::scripted_creates(vec![Err(ApiError::NoAnswer("timed out".into()))]);
    let first = s
        .publisher(&api, &tokens, &sleeper)
        .publish(&plan, &InMemoryProgress::new())
        .unwrap();

    assert_eq!(api.batch_sizes(), vec![3], "sent exactly once");
    assert_eq!(first.created, 0);
    assert!(first.halted.is_some());
    assert!(!first.complete());

    // And a later run leaves them alone rather than trying again.
    let again = FakeApi::default();
    let second = s
        .publisher(&again, &tokens, &sleeper)
        .publish(&plan, &InMemoryProgress::new())
        .unwrap();

    assert!(
        again.batch_sizes().is_empty(),
        "retrying would duplicate any that Google did create, and it cannot delete"
    );
    assert_eq!(second.unconfirmed.len(), 3);
    assert!(second.unconfirmed[0].reason.contains("check Google Photos"));
}

#[test]
fn a_definite_refusal_leaves_the_shots_safe_to_try_again() {
    // A 503 means Google declined to act, so nothing was created and the held
    // upload tokens make a retry free — the opposite of the lost-answer case.
    let s = session_of(2);
    let sleeper = RecordingSleeper::default();
    let tokens = FixedToken::working();
    let plan = s.plan_after_dry_run();

    let api = FakeApi::scripted_creates(vec![Err(ApiError::Refused {
        status: 503,
        detail: "backend unavailable".into(),
    })]);
    let first = s
        .publisher(&api, &tokens, &sleeper)
        .publish(&plan, &InMemoryProgress::new())
        .unwrap();
    assert_eq!(first.failed.len(), 2);
    assert!(first.unconfirmed.is_empty());

    let retry = FakeApi::default();
    let second = s
        .publisher(&retry, &tokens, &sleeper)
        .publish(&plan, &InMemoryProgress::new())
        .unwrap();

    assert_eq!(retry.batch_sizes(), vec![2]);
    assert_eq!(second.created, 2);
}

#[test]
fn a_created_photograph_lands_in_the_deduplication_ledger() {
    // F16's ledger is what makes the *next* ingest of this card publish nothing.
    let s = session_of(2);
    let api = FakeApi::default();
    let tokens = FixedToken::working();
    let sleeper = RecordingSleeper::default();

    s.publisher(&api, &tokens, &sleeper)
        .publish(&s.plan_after_dry_run(), &InMemoryProgress::new())
        .unwrap();

    assert!(s.ledger.is_published("source-0000").unwrap());
    assert!(s.ledger.is_published("source-0001").unwrap());
}

#[test]
fn a_photograph_that_was_not_created_is_absent_from_the_deduplication_ledger() {
    // The dangerous inverse: a ledger entry without a media item would make the
    // next ingest skip a photograph that was never published.
    let s = session_of(2);
    let api = FakeApi::scripted_creates(vec![Err(ApiError::Refused {
        status: 400,
        detail: "bad request".into(),
    })]);
    let tokens = FixedToken::working();
    let sleeper = RecordingSleeper::default();

    s.publisher(&api, &tokens, &sleeper)
        .publish(&s.plan_after_dry_run(), &InMemoryProgress::new())
        .unwrap();

    assert!(!s.ledger.is_published("source-0000").unwrap());
    assert!(!s.ledger.is_published("source-0001").unwrap());
}

// ---------------------------------------------------------------------------
// Rate limiting — §6.1's thirty-second floor
// ---------------------------------------------------------------------------

/// **Phase 12 acceptance.** A `429` is followed by a wait of at least 30 seconds.
#[test]
fn a_rate_limited_call_waits_at_least_thirty_seconds() {
    let s = session_of(2);
    let sleeper = RecordingSleeper::default();
    let tokens = FixedToken::working();

    let api = FakeApi::scripted_creates(vec![
        Err(ApiError::RateLimited { retry_after: None }),
        Ok(()),
    ]);
    let outcome = s
        .publisher(&api, &tokens, &sleeper)
        .publish(&s.plan_after_dry_run(), &InMemoryProgress::new())
        .unwrap();

    let waits = sleeper.waits();
    assert_eq!(waits.len(), 1);
    assert!(
        waits[0] >= Duration::from_secs(30),
        "§6.1 requires at least thirty seconds; got {:?}",
        waits[0]
    );
    assert_eq!(outcome.created, 2, "and then it succeeds");
}

#[test]
fn a_retry_after_header_shorter_than_the_floor_does_not_shorten_the_wait() {
    let s = session_of(1);
    let sleeper = RecordingSleeper::default();
    let tokens = FixedToken::working();

    let api = FakeApi::scripted_creates(vec![
        Err(ApiError::RateLimited {
            retry_after: Some(Duration::from_secs(1)),
        }),
        Ok(()),
    ]);
    s.publisher(&api, &tokens, &sleeper)
        .publish(&s.plan_after_dry_run(), &InMemoryProgress::new())
        .unwrap();

    assert_eq!(sleeper.waits(), vec![RATE_LIMIT_FLOOR]);
}

#[test]
fn repeated_rate_limits_back_off_and_then_stop() {
    let s = session_of(1);
    let sleeper = RecordingSleeper::default();
    let tokens = FixedToken::working();

    let api = FakeApi::scripted_creates(vec![Err(ApiError::RateLimited { retry_after: None })]);
    let outcome = s
        .publisher(&api, &tokens, &sleeper)
        .publish(&s.plan_after_dry_run(), &InMemoryProgress::new())
        .unwrap();

    let waits = sleeper.waits();
    assert_eq!(waits.len() as u32, MAX_RATE_LIMIT_RETRIES);
    assert!(waits[0] >= Duration::from_secs(30));
    assert!(
        waits.windows(2).all(|w| w[1] > w[0]),
        "the wait must grow: {waits:?}"
    );
    assert!(outcome.halted.is_some(), "it stops rather than hammering");
}

// ---------------------------------------------------------------------------
// The reconnect path — §6.2
// ---------------------------------------------------------------------------

/// **Phase 12 acceptance.** `invalid_grant` marks disconnected and does not
/// retry in a loop.
#[test]
fn a_dead_grant_stops_the_run_instead_of_failing_four_hundred_times() {
    let s = session_of(400);
    let api = FakeApi::default();
    let sleeper = RecordingSleeper::default();
    let tokens = FixedToken::dead("the Google account's authorisation is no longer valid");

    let outcome = s
        .publisher(&api, &tokens, &sleeper)
        .publish(&s.plan_after_dry_run(), &InMemoryProgress::new())
        .unwrap();

    assert_eq!(
        tokens.calls(),
        1,
        "one attempt, not one per photograph — this is the loop the build plan forbids"
    );
    assert_eq!(api.upload_count(), 0);
    assert!(outcome.halted.is_some());
    assert!(outcome.halted.unwrap().contains("resumes where it stopped"));
}

// ---------------------------------------------------------------------------
// OAuth — §6.2
// ---------------------------------------------------------------------------

fn oauth_config() -> OAuthConfig {
    OAuthConfig {
        client_id: "client-id.apps.googleusercontent.com".into(),
        client_secret: "client-secret".into(),
        redirect_uri: "https://nas.local/api/connectors/google/callback".into(),
    }
}

fn cipher() -> TokenCipher {
    TokenCipher::from_hex(&"7f".repeat(32)).unwrap()
}

/// A token endpoint that answers from a script and counts its calls.
struct FakeEndpoint {
    exchanges: RefCell<Vec<Result<TokenResponse, TokenError>>>,
    refreshes: RefCell<Vec<Result<TokenResponse, TokenError>>>,
    refresh_calls: RefCell<u32>,
}

impl FakeEndpoint {
    fn new() -> Self {
        Self {
            exchanges: RefCell::new(Vec::new()),
            refreshes: RefCell::new(Vec::new()),
            refresh_calls: RefCell::new(0),
        }
    }

    fn granting(refresh_token: &str) -> Self {
        let endpoint = Self::new();
        endpoint.exchanges.borrow_mut().push(Ok(TokenResponse {
            access_token: "access-1".into(),
            refresh_token: Some(refresh_token.into()),
            expires_in: 3600,
            scope: None,
        }));
        endpoint
    }
}

// The connector is used from one thread; `Sync` is satisfied by never sharing.
unsafe impl Sync for FakeEndpoint {}
unsafe impl Send for FakeEndpoint {}

impl TokenEndpoint for FakeEndpoint {
    fn exchange_code(
        &self,
        _config: &OAuthConfig,
        _code: &str,
    ) -> Result<TokenResponse, TokenError> {
        self.exchanges
            .borrow_mut()
            .pop()
            .unwrap_or(Err(TokenError::Transport("no scripted answer".into())))
    }

    fn refresh(
        &self,
        _config: &OAuthConfig,
        _refresh_token: &str,
    ) -> Result<TokenResponse, TokenError> {
        *self.refresh_calls.borrow_mut() += 1;
        self.refreshes
            .borrow_mut()
            .pop()
            .unwrap_or(Err(TokenError::Transport("no scripted answer".into())))
    }
}

#[test]
fn the_consent_url_asks_for_offline_access_and_a_fresh_consent() {
    // Without `access_type=offline` there is no refresh token at all, and
    // without `prompt=consent` a reconnect silently returns none.
    let url = oauth_config().consent_url("nonce");

    assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
    assert!(url.contains("access_type=offline"), "{url}");
    assert!(url.contains("prompt=consent"), "{url}");
    assert!(url.contains("response_type=code"), "{url}");
    assert!(url.contains("state=nonce"), "{url}");
    assert!(
        url.contains("photoslibrary.appendonly"),
        "the only scope that still works: {url}"
    );
    assert!(!url.contains("photoslibrary.readonly"), "{url}");
}

#[test]
fn the_consent_url_encodes_its_parameters() {
    let mut config = oauth_config();
    config.redirect_uri = "https://nas.local/cb?a=b&c=d".into();
    let url = config.consent_url("n");

    assert!(
        url.contains("redirect_uri=https%3A%2F%2Fnas.local%2Fcb%3Fa%3Db%26c%3Dd"),
        "{url}"
    );
}

#[test]
fn a_connected_account_stores_its_refresh_token_encrypted() {
    let ledger = Ledger::open_in_memory().unwrap();
    let endpoint = FakeEndpoint::granting("1//0-the-refresh-token");
    let connector = Connector::new(&ledger, oauth_config(), cipher(), &endpoint);

    let url = connector.begin().unwrap();
    let state = url.split("state=").nth(1).unwrap().to_string();
    connector.complete("auth-code", &state).unwrap();

    let stored = ledger.oauth_grant("google").unwrap().unwrap();
    assert!(
        !stored.encrypted_refresh_token.contains("the-refresh-token"),
        "the token is stored in the clear: {}",
        stored.encrypted_refresh_token
    );
    assert_eq!(
        cipher().decrypt(&stored.encrypted_refresh_token).unwrap(),
        "1//0-the-refresh-token"
    );
    assert!(connector.status().unwrap().connected);
}

#[test]
fn a_callback_whose_state_does_not_match_is_refused() {
    // Without this check, anybody who can make the photographer's browser hit
    // the callback can bind their own Google account to this server.
    let ledger = Ledger::open_in_memory().unwrap();
    let endpoint = FakeEndpoint::granting("1//0-token");
    let connector = Connector::new(&ledger, oauth_config(), cipher(), &endpoint);

    connector.begin().unwrap();
    let err = connector
        .complete("auth-code", "not-the-nonce")
        .unwrap_err();

    assert!(matches!(err, Error::AccessDenied(_)), "got {err}");
    assert!(ledger.oauth_grant("google").unwrap().is_none());
}

#[test]
fn a_state_nonce_cannot_be_used_twice() {
    let ledger = Ledger::open_in_memory().unwrap();
    let endpoint = FakeEndpoint::granting("1//0-token");
    let connector = Connector::new(&ledger, oauth_config(), cipher(), &endpoint);

    let url = connector.begin().unwrap();
    let state = url.split("state=").nth(1).unwrap().to_string();

    connector.complete("auth-code", &state).unwrap();
    assert!(connector.complete("auth-code", &state).is_err());
}

#[test]
fn an_exchange_that_returns_no_refresh_token_says_what_to_do_about_it() {
    let ledger = Ledger::open_in_memory().unwrap();
    let endpoint = FakeEndpoint::new();
    endpoint.exchanges.borrow_mut().push(Ok(TokenResponse {
        access_token: "access-1".into(),
        refresh_token: None,
        expires_in: 3600,
        scope: None,
    }));
    let connector = Connector::new(&ledger, oauth_config(), cipher(), &endpoint);

    let url = connector.begin().unwrap();
    let state = url.split("state=").nth(1).unwrap().to_string();
    let err = connector.complete("code", &state).unwrap_err();

    assert!(
        err.to_string().contains("Revoke PhotoTools' access"),
        "got {err}"
    );
}

/// **Phase 12 acceptance.** `invalid_grant` marks disconnected and does not
/// retry in a loop.
#[test]
fn invalid_grant_disconnects_the_connector_and_stops_asking() {
    let ledger = Ledger::open_in_memory().unwrap();
    let endpoint = FakeEndpoint::granting("1//0-token");
    endpoint
        .refreshes
        .borrow_mut()
        .push(Err(TokenError::InvalidGrant(
            "Token has been expired or revoked.".into(),
        )));
    let connector = Connector::new(&ledger, oauth_config(), cipher(), &endpoint);

    let url = connector.begin().unwrap();
    let state = url.split("state=").nth(1).unwrap().to_string();
    connector.complete("code", &state).unwrap();

    // Expire the cached access token so the next call must refresh.
    AccessTokens::invalidate(&connector);

    let err = connector.access_token().unwrap_err();
    assert!(err.to_string().contains("Reconnect"), "got {err}");
    assert_eq!(*endpoint.refresh_calls.borrow(), 1);

    let status = connector.status().unwrap();
    assert!(!status.connected);
    assert!(status.needs_reauthorisation);
    assert!(status.detail.unwrap().contains("Reconnect"));

    // Every later attempt short-circuits without asking Google again.
    for _ in 0..10 {
        assert!(connector.access_token().is_err());
    }
    assert_eq!(
        *endpoint.refresh_calls.borrow(),
        1,
        "a disconnected connector must not keep asking"
    );
}

#[test]
fn a_refreshed_access_token_is_reused_rather_than_fetched_per_photograph() {
    let ledger = Ledger::open_in_memory().unwrap();
    let endpoint = FakeEndpoint::granting("1//0-token");
    let connector = Connector::new(&ledger, oauth_config(), cipher(), &endpoint);

    let url = connector.begin().unwrap();
    let state = url.split("state=").nth(1).unwrap().to_string();
    connector.complete("code", &state).unwrap();

    for _ in 0..100 {
        connector.access_token().unwrap();
    }
    assert_eq!(
        *endpoint.refresh_calls.borrow(),
        0,
        "the exchange's token is still good"
    );
}

#[test]
fn disconnecting_removes_the_stored_refresh_token() {
    // A grant nobody intends to use should not sit in the database.
    let ledger = Ledger::open_in_memory().unwrap();
    let endpoint = FakeEndpoint::granting("1//0-token");
    let connector = Connector::new(&ledger, oauth_config(), cipher(), &endpoint);

    let url = connector.begin().unwrap();
    let state = url.split("state=").nth(1).unwrap().to_string();
    connector.complete("code", &state).unwrap();
    assert!(ledger.oauth_grant("google").unwrap().is_some());

    connector.disconnect().unwrap();

    assert!(ledger.oauth_grant("google").unwrap().is_none());
    assert!(!connector.status().unwrap().connected);
}

// ---------------------------------------------------------------------------
// The real HTTP client, against a socket on localhost
// ---------------------------------------------------------------------------

/// A one-shot HTTP server that answers every request the same way.
///
/// Hand-rolled rather than a dependency: the requirement is that no test reaches
/// Google, and binding `127.0.0.1` proves it more directly than any library
/// could. It also lets the *request* be captured and asserted on.
fn mock_server(
    status_line: &str,
    body: &str,
    expected_requests: usize,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let status_line = status_line.to_string();
    let body = body.to_string();

    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for _ in 0..expected_requests {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut buf = vec![0u8; 64 * 1024];
            let n = stream.read(&mut buf).unwrap_or(0);
            seen.push(String::from_utf8_lossy(&buf[..n]).to_string());

            let response = format!(
                "{status_line}\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
        seen
    });

    (format!("http://127.0.0.1:{port}"), handle)
}

#[test]
fn the_real_client_sends_the_headers_google_requires() {
    let f = Fixtures::new();
    let photo = f.path().join("IMG_0001.jpg");
    std::fs::write(&photo, b"jpeg bytes").unwrap();

    let (base, handle) = mock_server("HTTP/1.1 200 OK", "upload-token-abc", 1);
    let api = HttpPhotosApi::with_endpoints(&format!("{base}/uploads"), &format!("{base}/create"));

    let token = api.upload("access-token", &photo, "IMG_0001.jpg").unwrap();
    assert_eq!(token, "upload-token-abc");

    let request = handle.join().unwrap().remove(0);
    // Header names are case-insensitive and reqwest writes them lowercased, so
    // the assertion folds case rather than pinning one client's spelling.
    let headers = request.to_ascii_lowercase();
    assert!(
        headers.contains("authorization: bearer access-token"),
        "{request}"
    );
    assert!(headers.contains("x-goog-upload-protocol: raw"), "{request}");
    assert!(
        headers.contains("x-goog-upload-content-type: image/jpeg"),
        "{request}"
    );
    assert!(
        headers.contains("x-goog-file-name: img_0001.jpg"),
        "{request}"
    );
    assert!(
        request.contains("jpeg bytes"),
        "the bytes must be the body: {request}"
    );
}

#[test]
fn the_real_client_parses_a_batch_create_response() {
    let body = r#"{"newMediaItemResults":[
        {"status":{"code":0},"mediaItem":{"id":"media-1"}},
        {"status":{"code":3,"message":"Invalid upload token."}}
    ]}"#;
    let (base, handle) = mock_server("HTTP/1.1 200 OK", body, 1);
    let api = HttpPhotosApi::with_endpoints(&format!("{base}/uploads"), &format!("{base}/create"));

    let results = api
        .batch_create(
            "access-token",
            &[
                NewMediaItem {
                    upload_token: "t1".into(),
                    file_name: "IMG_0001.jpg".into(),
                },
                NewMediaItem {
                    upload_token: "t2".into(),
                    file_name: "IMG_0002.jpg".into(),
                },
            ],
        )
        .unwrap();

    assert_eq!(
        results,
        vec![
            CreateResult::Created {
                media_item_id: "media-1".into()
            },
            CreateResult::Failed {
                detail: "Invalid upload token.".into()
            },
        ]
    );

    let request = handle.join().unwrap().remove(0);
    assert!(request.contains("\"uploadToken\":\"t1\""), "{request}");
    assert!(request.contains("\"simpleMediaItem\""), "{request}");
}

#[test]
fn the_real_client_reports_a_429_as_rate_limited_with_its_retry_after() {
    let (base, _handle) = mock_server("HTTP/1.1 429 Too Many Requests\r\nRetry-After: 45", "{}", 1);
    let api = HttpPhotosApi::with_endpoints(&format!("{base}/uploads"), &format!("{base}/create"));

    let err = api.batch_create("access-token", &[]).unwrap_err();

    assert_eq!(
        err,
        ApiError::RateLimited {
            retry_after: Some(Duration::from_secs(45))
        }
    );
    assert!(!err.may_have_been_applied());
}

#[test]
fn the_real_client_treats_a_short_batch_create_answer_as_unconfirmed() {
    // Google answered for fewer items than were sent. Some may exist; there is
    // no way to say which, so it must not be retried.
    let (base, _handle) = mock_server(
        "HTTP/1.1 200 OK",
        r#"{"newMediaItemResults":[{"status":{"code":0},"mediaItem":{"id":"media-1"}}]}"#,
        1,
    );
    let api = HttpPhotosApi::with_endpoints(&format!("{base}/uploads"), &format!("{base}/create"));

    let err = api
        .batch_create(
            "access-token",
            &[
                NewMediaItem {
                    upload_token: "t1".into(),
                    file_name: "a.jpg".into(),
                },
                NewMediaItem {
                    upload_token: "t2".into(),
                    file_name: "b.jpg".into(),
                },
            ],
        )
        .unwrap_err();

    assert!(err.may_have_been_applied(), "got {err}");
}

#[test]
fn an_upload_that_returns_no_token_is_not_reported_as_a_success() {
    // §9.2 invariant 6. A 200 with an empty body is not an upload.
    let (base, _handle) = mock_server("HTTP/1.1 200 OK", "", 1);
    let api = HttpPhotosApi::with_endpoints(&format!("{base}/uploads"), &format!("{base}/create"));

    let f = Fixtures::new();
    let photo = f.path().join("IMG_0001.jpg");
    std::fs::write(&photo, b"bytes").unwrap();

    assert!(api.upload("access-token", &photo, "IMG_0001.jpg").is_err());
}

/// Building a plan must not be what satisfies the dry-run requirement.
///
/// A publish needs the same plan a dry run produces, and the obvious way to get
/// it — call the dry run — would have every publish stamp the session as
/// reviewed on its way past, turning §9.2 rule 3 into a check that cannot fail.
#[test]
fn computing_a_plan_does_not_count_as_having_reviewed_one() {
    let s = session_of(2);

    let plan =
        phototools_core::publish::plan_publish(&s.manifest, &s.plan, Some(&s.report), &s.ledger)
            .unwrap();

    assert_eq!(plan.items.len(), 2, "the plan is still computed");
    assert!(
        s.ledger.dry_run_at(&s.plan.session_id).unwrap().is_none(),
        "planning must not record a dry run"
    );

    let api = FakeApi::default();
    let tokens = FixedToken::working();
    let sleeper = RecordingSleeper::default();
    let err = s
        .publisher(&api, &tokens, &sleeper)
        .publish(&plan, &InMemoryProgress::new())
        .unwrap_err();

    assert!(err.to_string().contains("no dry run"), "got {err}");
    assert_eq!(api.upload_count(), 0);
}
