use rand::seq::SliceRandom;
use rand::Rng;
use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

use super::models::{RecognizedSong, ShazamResponse};

const USER_AGENTS: &[&str] = &[
    "Dalvik/2.1.0 (Linux; U; Android 14; Pixel 8 Pro Build/UQ1A.240205.004)",
    "Dalvik/2.1.0 (Linux; U; Android 13; SM-S918B Build/TP1A.220624.014)",
    "Dalvik/2.1.0 (Linux; U; Android 12; SM-G998B Build/SP1A.210812.016)",
    "Dalvik/2.1.0 (Linux; U; Android 11; OnePlus 9 Pro Build/RP1A.201005.001)",
    "Dalvik/2.1.0 (Linux; U; Android 10; SM-G960F Build/QP1A.190711.020)",
];

const TIMEZONES: &[&str] = &[
    "America/New_York",
    "America/Chicago",
    "America/Los_Angeles",
    "Europe/London",
    "Europe/Paris",
    "Europe/Berlin",
    "Asia/Tokyo",
    "Asia/Kolkata",
];

pub struct ShazamClient {
    client: Client,
}

impl ShazamClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(8))
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client }
    }

    pub async fn recognize(
        &self,
        signature_uri: &str,
        sample_ms: u32,
    ) -> Result<Option<RecognizedSong>, Box<dyn std::error::Error + Send + Sync>> {
        let mut rng = rand::thread_rng();

        let user_agent = USER_AGENTS.choose(&mut rng).unwrap_or(&USER_AGENTS[0]);
        let timezone = TIMEZONES.choose(&mut rng).unwrap_or(&TIMEZONES[0]);

        // Jitter coordinates within realistic bounds
        let lat: f64 = rng.gen_range(-60.0..60.0);
        let lon: f64 = rng.gen_range(-150.0..150.0);
        let alt: f64 = rng.gen_range(50.0..400.0);

        let now_sec = chrono::Utc::now().timestamp();

        let payload = json!({
            "geolocation": {
                "altitude": alt,
                "latitude": lat,
                "longitude": lon
            },
            "signature": {
                "samplems": sample_ms,
                "timestamp": now_sec,
                "uri": signature_uri
            },
            "timestamp": now_sec,
            "timezone": timezone
        });

        let uuid1 = Uuid::new_v4().to_string().to_uppercase();
        let uuid2 = Uuid::new_v4().to_string();

        let url = format!(
            "https://amp.shazam.com/discovery/v5/en/US/android/-/tag/{}/{}?sync=true&webv3=true&sampling=16000&shazamapiversion=v3&sharehub=true&video=v3",
            uuid1, uuid2
        );

        let response = self
            .client
            .post(&url)
            .header("User-Agent", *user_agent)
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
