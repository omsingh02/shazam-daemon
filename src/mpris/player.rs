use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use zbus::interface;
use zbus::zvariant::Value;

use crate::downloader::JioSaavnDownloader;
use crate::network::models::RecognizedSong;

pub struct ShazamRoot;

#[interface(name = "org.mpris.MediaPlayer2")]
impl ShazamRoot {
    #[zbus(property)]
    async fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    async fn can_raise(&self) -> bool {
        false
    }

    #[zbus(property)]
    async fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    async fn identity(&self) -> String {
        "Shazam".to_string()
    }

    #[zbus(property)]
    async fn supported_uri_schemes(&self) -> Vec<String> {
        vec!["http".into(), "https".into()]
    }

    #[zbus(property)]
    async fn supported_mime_types(&self) -> Vec<String> {
        vec!["audio/vnd.shazam.sig".into()]
    }

    async fn raise(&self) {}
    async fn quit(&self) {}
}

pub struct ShazamPlayer {
    is_listening: Arc<AtomicBool>,
    engine_status: Arc<RwLock<String>>,
    current_song: Arc<RwLock<Option<RecognizedSong>>>,
    downloader: Arc<JioSaavnDownloader>,
}

impl ShazamPlayer {
    pub fn new(
        is_listening: Arc<AtomicBool>,
        engine_status: Arc<RwLock<String>>,
        current_song: Arc<RwLock<Option<RecognizedSong>>>,
    ) -> Self {
        Self {
            is_listening,
            engine_status,
            current_song,
            downloader: Arc::new(JioSaavnDownloader::new()),
        }
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl ShazamPlayer {
    #[zbus(property)]
    async fn playback_status(&self) -> String {
        let song_guard = self.current_song.read().await;
        if song_guard.is_some() {
            "Playing".to_string()
        } else {
            "Paused".to_string()
        }
    }

    #[zbus(property)]
    async fn engine_status(&self) -> String {
        self.engine_status.read().await.clone()
    }

    #[zbus(property)]
    async fn can_control(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn can_seek(&self) -> bool {
        false
    }

    #[zbus(property)]
    async fn metadata(&self) -> HashMap<String, Value<'static>> {
        let mut map = HashMap::new();
        let song_guard = self.current_song.read().await;

        if let Some(song) = song_guard.as_ref() {
            let track_id = format!(
                "/org/mpris/MediaPlayer2/Track/{}",
                song.shazam_key.as_deref().unwrap_or("0")
            );
            map.insert("mpris:trackid".into(), Value::from(track_id));
            map.insert("shazam:engineStatus".into(), Value::from(self.engine_status.read().await.clone()));
            map.insert("xesam:title".into(), Value::from(song.title.clone()));
            map.insert("xesam:artist".into(), Value::from(vec![song.artist.clone()]));

            if let Some(album) = &song.album {
                map.insert("xesam:album".into(), Value::from(album.clone()));
            }
            if let Some(genre) = &song.genre {
                map.insert("xesam:genre".into(), Value::from(vec![genre.clone()]));
            }
            if let Some(art_url) = song.cover_art_hq_url.as_ref().or(song.cover_art_url.as_ref()) {
                map.insert("mpris:artUrl".into(), Value::from(art_url.clone()));
                map.insert("xesam:artUrl".into(), Value::from(art_url.clone()));
            }
            if let Some(isrc) = &song.isrc {
                map.insert("shazam:isrc".into(), Value::from(isrc.clone()));
            }
            if let Some(offset) = song.offset_seconds {
                map.insert("shazam:offset".into(), Value::from(offset));
            }
            if let Some(preview) = &song.preview_audio_url {
                map.insert("shazam:previewUrl".into(), Value::from(preview.clone()));
            }
            if let Some(yt) = &song.youtube_url {
                map.insert("shazam:youtubeUrl".into(), Value::from(yt.clone()));
            }
            if let Some(share) = &song.share_url {
                map.insert("shazam:shareUrl".into(), Value::from(share.clone()));
            }
            if let Some(lyrics) = &song.lyrics {
                map.insert("shazam:lyrics".into(), Value::from(lyrics.join("\n")));
            }
        } else {
            map.insert("shazam:engineStatus".into(), Value::from(self.engine_status.read().await.clone()));
            map.insert(
                "mpris:trackid".into(),
                Value::from("/org/mpris/MediaPlayer2/Track/none".to_string()),
            );
        }

        map
    }

    async fn play(&self) {
        if !self.is_listening.load(Ordering::Relaxed) {
            unsafe { libc::kill(libc::getpid(), libc::SIGUSR1); }
        }
    }

    async fn pause(&self) {
        if self.is_listening.load(Ordering::Relaxed) {
            unsafe { libc::kill(libc::getpid(), libc::SIGUSR1); }
        }
    }

    async fn play_pause(&self) {
        unsafe {
            libc::kill(libc::getpid(), libc::SIGUSR1);
        }
    }

    async fn stop(&self) {
        if self.is_listening.load(Ordering::Relaxed) {
            unsafe { libc::kill(libc::getpid(), libc::SIGUSR1); }
        }
    }

    async fn download_current(&self) -> String {
        let (title, artist) = {
            let guard = self.current_song.read().await;
            match guard.as_ref() {
                Some(s) => (s.title.clone(), s.artist.clone()),
                None => return "Error: No song currently recognized".to_string(),
            }
        };

        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let download_dir = PathBuf::from(home).join("Music").join("ShazamLive");

        match self.downloader.download_song(&title, &artist, download_dir).await {
            Ok(p) => format!("Success: Downloaded to {}", p.display()),
            Err(e) => format!("Error: {}", e),
        }
    }

    async fn download_track(&self, title: String, artist: String) -> String {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let download_dir = PathBuf::from(home).join("Music").join("ShazamLive");

        match self.downloader.download_song(&title, &artist, download_dir).await {
            Ok(p) => format!("Success: Downloaded to {}", p.display()),
            Err(e) => format!("Error: {}", e),
        }
    }

    async fn get_preview_url(&self) -> String {
        let guard = self.current_song.read().await;
        guard.as_ref().and_then(|s| s.preview_audio_url.clone()).unwrap_or_default()
    }

    async fn get_recent_history(&self, limit: u32) -> String {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let hist_path = PathBuf::from(home).join(".local/share/shazam_history.jsonl");
        if !hist_path.exists() {
            return "[]".to_string();
        }
        let content = match tokio::fs::read_to_string(&hist_path).await {
            Ok(c) => c,
            Err(_) => return "[]".to_string(),
        };
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        let take_count = (limit as usize).min(lines.len());
        let slice = &lines[lines.len() - take_count..];
        let mut items = Vec::new();
        for l in slice.iter().rev() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(l) {
                items.push(v);
            }
        }
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
    }

    async fn clear_history(&self) -> bool {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let hist_jsonl = PathBuf::from(home.clone()).join(".local/share/shazam_history.jsonl");
        let hist_txt = PathBuf::from(home).join(".local/share/shazam_history.txt");
        let _ = tokio::fs::remove_file(hist_jsonl).await;
        let _ = tokio::fs::remove_file(hist_txt).await;
        true
    }

    async fn search_history(&self, query: String) -> String {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let hist_path = PathBuf::from(home).join(".local/share/shazam_history.jsonl");
        if !hist_path.exists() {
            return "[]".to_string();
        }
        let content = match tokio::fs::read_to_string(&hist_path).await {
            Ok(c) => c,
            Err(_) => return "[]".to_string(),
        };
        let q = query.trim().to_lowercase();
        let items: Vec<serde_json::Value> = content
            .lines()
            .rev()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|item| {
                if q.is_empty() {
                    return true;
                }
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let artist = item.get("artist").and_then(|v| v.as_str()).unwrap_or("");
                let album = item.get("album").and_then(|v| v.as_str()).unwrap_or("");
                let genre = item.get("genre").and_then(|v| v.as_str()).unwrap_or("");
                title.to_lowercase().contains(&q)
                    || artist.to_lowercase().contains(&q)
                    || album.to_lowercase().contains(&q)
                    || genre.to_lowercase().contains(&q)
            })
            .take(100)
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
    }
}
