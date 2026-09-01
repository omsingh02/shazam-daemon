use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

use super::models::{RecognizedSong, ShazamResponse};

pub struct ShazamClient {
    client: Client,
}

impl ShazamClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client }
    }

    pub async fn recognize(
        &self,
        signature_uri: &str,
        sample_ms: u32,
    ) -> Result<Option<RecognizedSong>, Box<dyn std::error::Error + Send + Sync>> {
        let now_sec = chrono::Utc::now().timestamp();

        let payload = json!({
            "geolocation": {
                "altitude": 200.0,
                "latitude": 37.7749,
                "longitude": -122.4194
            },
            "signature": {
                "samplems": sample_ms,
                "timestamp": now_sec,
                "uri": signature_uri
            },
            "timestamp": now_sec,
            "timezone": "America/Los_Angeles"
        });

        let uuid1 = Uuid::new_v4().to_string().to_uppercase();
        let uuid2 = Uuid::new_v4().to_string();

        let url = format!(
            "https://amp.shazam.com/discovery/v5/en/US/android/-/tag/{}/{}?sync=true&webv3=true&sampling=true&connected=&shazamapiversion=v3&sharehub=true&video=v3",
            uuid1, uuid2
        );

        let response = self
            .client
            .post(&url)
            .header("User-Agent", "Dalvik/2.1.0 (Linux; U; Android 10; SM-G960F Build/QP1A.190711.020)")
            .header("Content-Type", "application/json")
            .header("Content-Language", "en_US")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let shazam_resp: ShazamResponse = response.json().await?;
        Ok(RecognizedSong::from_shazam_response(shazam_resp))
    }
}
