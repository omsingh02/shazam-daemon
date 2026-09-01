use std::path::PathBuf;
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SearchResultsResponse {
    results: Option<Vec<JioSaavnSong>>,
}

#[derive(Debug, Deserialize)]
pub struct JioSaavnSong {
    pub id: Option<String>,
    pub song: Option<String>,
    pub primary_artists: Option<String>,
    pub singers: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub image: Option<String>,
    pub encrypted_media_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthTokenResponse {
    auth_url: Option<String>,
    status: Option<String>,
}

pub struct JioSaavnDownloader {
    client: Client,
}

impl JioSaavnDownloader {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_default();
        Self { client }
    }

    pub async fn search(&self, title: &str, artist: &str) -> Option<JioSaavnSong> {
        let query = format!("{} {}", title, artist);
        let resp = self.client.get("https://www.jiosaavn.com/api.php")
            .query(&[
                ("__call", "search.getResults"),
                ("_format", "json"),
                ("_marker", "0"),
                ("cc", "in"),
                ("includeMetaTags", "1"),
                ("q", &query),
                ("n", "5"),
            ])
            .send()
            .await
            .ok()?;
        let json_text = resp.text().await.ok()?;
        
        let parsed: SearchResultsResponse = serde_json::from_str(&json_text).ok()?;
        let mut songs = parsed.results?;
        if songs.is_empty() { return None; }
        Some(songs.remove(0))
    }

    pub async fn get_direct_stream_url(&self, enc_url: &str) -> Result<String, String> {
        let resp = self.client.post("https://www.jiosaavn.com/api.php")
            .form(&[
                ("__call", "song.generateAuthToken"),
                ("url", enc_url),
                ("bitrate", "320"),
                ("api_version", "4"),
                ("_format", "json"),
                ("ctx", "web6dot0"),
                ("_marker", "0"),
            ])
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;
        
        let status = resp.status();
        let body_text = resp.text().await.map_err(|e| format!("Failed to read body: {}", e))?;
        
        if let Ok(auth_resp) = serde_json::from_str::<AuthTokenResponse>(&body_text) {
            if let Some(auth_url) = auth_resp.auth_url {
                return Ok(auth_url);
            }
        }
        
        // Also try JSON Value lookup in case structure has data.auth_url
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body_text) {
            if let Some(u) = val.get("auth_url").and_then(|v| v.as_str()) {
                return Ok(u.to_string());
            }
            if let Some(u) = val.pointer("/data/auth_url").and_then(|v| v.as_str()) {
                return Ok(u.to_string());
            }
        }
        
        Err(format!("HTTP {} - Server returned: {}", status, body_text))
    }

    pub async fn download_song(&self, title: &str, artist: &str, download_dir: PathBuf) -> Result<PathBuf, String> {
        let song = self.search(title, artist).await.ok_or_else(|| "No match found on JioSaavn".to_string())?;
        
        let enc_url = song.encrypted_media_url.as_ref()
            .ok_or_else(|| "No media URL in JioSaavn metadata".to_string())?;

        let media_url = self.get_direct_stream_url(enc_url).await?;

        tokio::fs::create_dir_all(&download_dir).await.map_err(|e| e.to_string())?;

        let clean_title = song.song.as_deref().unwrap_or(title);
        let clean_artist = song.primary_artists.as_deref().or(song.singers.as_deref()).unwrap_or(artist);

        let safe_title = clean_title.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        let safe_artist = clean_artist.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        let filename = format!("{} - {}.m4a", safe_title, safe_artist);
        let dest_path = download_dir.join(&filename);

        // Download audio payload
        let audio_bytes = self.client.get(&media_url)
            .header("Referer", "https://www.jiosaavn.com/")
            .send()
            .await
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;

        tokio::fs::write(&dest_path, &audio_bytes).await.map_err(|e| e.to_string())?;

        // Download High-Res Album Art if present
        let mut cover_bytes = None;
        if let Some(ref img_url) = song.image {
            let hq_url = img_url.replace("150x150.jpg", "500x500.jpg").replace("50x50.jpg", "500x500.jpg");
            if let Ok(resp) = self.client.get(&hq_url).send().await {
                if let Ok(bytes) = resp.bytes().await {
                    cover_bytes = Some(bytes.to_vec());
                }
            }
        }

        // Tag the downloaded file with standard MP4/M4A metadata
        if let Ok(mut tag) = mp4ameta::Tag::read_from_path(&dest_path) {
            tag.set_title(clean_title);
            tag.set_artist(clean_artist);
            if let Some(ref alb) = song.album {
                tag.set_album(alb);
            }
            if let Some(ref yr) = song.year {
                tag.set_year(yr);
            }
            if let Some(bytes) = cover_bytes {
                tag.add_artwork(mp4ameta::Img::jpeg(bytes));
            }
            let _ = tag.write_to_path(&dest_path);
        }

        Ok(dest_path)
    }
}
