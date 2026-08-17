use phototools_core::ledger::Ledger;
use phototools_core::publish::{OAuth2Manager, Uploader};
use std::path::Path;

#[test]
fn test_publish_flow() {
    let ledger = Ledger::open_in_memory().unwrap();
    let auth = OAuth2Manager::new(&ledger);

    // Save token
    auth.save_token("google", "secret_refresh", "photoslibrary", 9999999999)
        .unwrap();

    // Check DB
    let token: String = ledger
        .inner()
        .query_row(
            "SELECT encrypted_refresh_token FROM oauth WHERE provider = 'google'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(token, "secret_refresh");

    // Publish states
    ledger.add_publish("shot1").unwrap();

    // Update to uploading
    ledger
        .update_publish_state("shot1", "uploading", Some("token1"), None, None)
        .unwrap();

    // Check state
    let state: String = ledger
        .inner()
        .query_row(
            "SELECT state FROM publishes WHERE shot_id = 'shot1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "uploading");

    // Mock Upload
    let bearer = auth.get_bearer_token().unwrap();
    let uploader = Uploader::new(bearer);

    let upload_token = uploader.upload_bytes(Path::new("MOCK")).unwrap();
    assert_eq!(upload_token, "mock_upload_token");

    let media_id = uploader.create_media_item(&upload_token, "Test").unwrap();
    assert_eq!(media_id, "mock_media_item_id");

    // Update to published
    ledger
        .update_publish_state(
            "shot1",
            "published",
            Some(&upload_token),
            Some(&media_id),
            None,
        )
        .unwrap();

    let final_state: String = ledger
        .inner()
        .query_row(
            "SELECT state FROM publishes WHERE shot_id = 'shot1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(final_state, "published");
}
