use std::fs::{create_dir_all, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use crate::network::models::RecognizedSong;

pub struct HistoryStorage {
    txt_path: PathBuf,
    jsonl_path: PathBuf,
}

impl HistoryStorage {
    pub fn new() -> Self {
        let base_dir = dirs_or_fallback();
        let txt_path = base_dir.join("shazam_history.txt");
        let jsonl_path = base_dir.join("shazam_history.jsonl");

        let _ = create_dir_all(&base_dir);

        Self {
            txt_path,
            jsonl_path,
        }
    }

    pub fn log_song(&self, song: &RecognizedSong) {
        let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        // 1. Append to text log
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.txt_path)
        {
            let _ = writeln!(file, "[{}] {} - {}", now_str, song.artist, song.title);
        }

        // 2. Append to JSONL log
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.jsonl_path)
        {
            let json_entry = serde_json::json!({
                "timestamp": now_str,
                "title": song.title,
                "artist": song.artist,
                "album": song.album.as_deref().unwrap_or(""),
                "genre": song.genre.as_deref().unwrap_or(""),
                "isrc": song.isrc.as_deref().unwrap_or(""),
                "shazam_key": song.shazam_key.as_deref().unwrap_or(""),
                "cover_art": song.cover_art_hq_url.as_deref().or(song.cover_art_url.as_deref()).unwrap_or(""),
                "offset": song.offset_seconds.unwrap_or(0.0),
                "preview_url": song.preview_audio_url.as_deref().unwrap_or(""),
                "youtube_url": song.youtube_url.as_deref().unwrap_or(""),
                "share_url": song.share_url.as_deref().unwrap_or(""),
                "lyrics": song.lyrics.as_deref().unwrap_or(&[])
            });
            if let Ok(serialized) = serde_json::to_string(&json_entry) {
                let _ = writeln!(file, "{}", serialized);
            }
        }
    }

    pub fn get_recent(&self, limit: usize) -> Vec<serde_json::Value> {
        if !self.jsonl_path.exists() {
            return Vec::new();
        }

        let Ok(file) = OpenOptions::new().read(true).open(&self.jsonl_path) else {
            return Vec::new();
        };

        let reader = BufReader::new(file);
        let mut lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
        lines.reverse();

        lines
            .into_iter()
            .take(limit)
            .filter_map(|l| serde_json::from_str(&l).ok())
            .collect()
    }

    pub fn search(&self, query: &str) -> Vec<serde_json::Value> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.get_recent(50);
        }

        if !self.jsonl_path.exists() {
            return Vec::new();
        }

        let Ok(file) = OpenOptions::new().read(true).open(&self.jsonl_path) else {
            return Vec::new();
        };

        let reader = BufReader::new(file);
        let mut lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
        lines.reverse();

        lines
            .into_iter()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(&l).ok())
            .filter(|item| {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let artist = item.get("artist").and_then(|v| v.as_str()).unwrap_or("");
                let album = item.get("album").and_then(|v| v.as_str()).unwrap_or("");
                let genre = item.get("genre").and_then(|v| v.as_str()).unwrap_or("");
                title.to_lowercase().contains(&q)
                    || artist.to_lowercase().contains(&q)
                    || album.to_lowercase().contains(&q)
                    || genre.to_lowercase().contains(&q)
            })
            .take(50)
            .collect()
    }
}

fn dirs_or_fallback() -> PathBuf {
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(data_home)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/share")
    } else {
        PathBuf::from("/tmp")
    }
}
