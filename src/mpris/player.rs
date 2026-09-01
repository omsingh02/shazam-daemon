use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use zbus::interface;
use zbus::zvariant::Value;

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
    current_song: Arc<RwLock<Option<RecognizedSong>>>,
}

impl ShazamPlayer {
    pub fn new(
        is_listening: Arc<AtomicBool>,
        current_song: Arc<RwLock<Option<RecognizedSong>>>,
    ) -> Self {
        Self {
            is_listening,
            current_song,
        }
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl ShazamPlayer {
    #[zbus(property)]
    async fn playback_status(&self) -> String {
        if self.is_listening.load(Ordering::Relaxed) {
            "Playing".to_string()
        } else {
            "Paused".to_string()
        }
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
        } else {
            map.insert(
                "mpris:trackid".into(),
                Value::from("/org/mpris/MediaPlayer2/Track/none".to_string()),
            );
            map.insert("xesam:title".into(), Value::from("Shazam Active".to_string()));
            map.insert(
                "xesam:artist".into(),
                Value::from(vec!["Listening...".to_string()]),
            );
        }

        map
    }

    async fn play(&self) {
        self.is_listening.store(true, Ordering::Relaxed);
    }

    async fn pause(&self) {
        self.is_listening.store(false, Ordering::Relaxed);
    }

    async fn play_pause(&self) {
        let current = self.is_listening.load(Ordering::Relaxed);
        self.is_listening.store(!current, Ordering::Relaxed);
    }

    async fn stop(&self) {
        self.is_listening.store(false, Ordering::Relaxed);
    }
}
