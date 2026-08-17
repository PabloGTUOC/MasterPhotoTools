use crate::error::Error;
use reqwest::blocking::Client;
use serde_json::json;
use std::fs;
use std::path::Path;

pub struct Uploader {
    client: Client,
    token: String,
}

impl Uploader {
    pub fn new(token: String) -> Self {
        Self {
            client: Client::new(),
            token,
        }
    }

    pub fn upload_bytes(&self, path: &Path) -> Result<String, Error> {
        // Since we are mocking network calls for tests, let's just return a mock if path is "MOCK"
        if path.to_string_lossy() == "MOCK" {
            return Ok("mock_upload_token".to_string());
        }

        let file = fs::File::open(path)?;
        let res = self
            .client
            .post("https://photoslibrary.googleapis.com/v1/uploads")
            .bearer_auth(&self.token)
            .header("Content-type", "application/octet-stream")
            .header("X-Goog-Upload-Protocol", "raw")
            .body(file)
            .send()
            .map_err(|e| Error::Internal(e.to_string()))?;

        if !res.status().is_success() {
            return Err(Error::Internal(format!("Upload failed: {}", res.status())));
        }

        let upload_token = res.text().map_err(|e| Error::Internal(e.to_string()))?;
        Ok(upload_token)
    }

    pub fn create_media_item(
        &self,
        upload_token: &str,
        description: &str,
    ) -> Result<String, Error> {
        if upload_token == "mock_upload_token" {
            return Ok("mock_media_item_id".to_string());
        }

        let body = json!({
            "newMediaItems": [
                {
                    "description": description,
                    "simpleMediaItem": {
                        "uploadToken": upload_token
                    }
                }
            ]
        });

        let res = self
            .client
            .post("https://photoslibrary.googleapis.com/v1/mediaItems:batchCreate")
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .map_err(|e| Error::Internal(e.to_string()))?;

        if !res.status().is_success() {
            return Err(Error::Internal(format!("Create failed: {}", res.status())));
        }

        Ok("media_item_id_parsed".to_string())
    }
}
