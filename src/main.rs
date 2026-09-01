mod audio;
mod downloader;
mod dsp;
mod history;
mod mpris;
mod network;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::RwLock;
use zbus::connection::Builder;

use crate::audio::{AudioCapture, AudioSourceMode};
use crate::downloader::JioSaavnDownloader;
use crate::dsp::SignatureGenerator;
use crate::history::HistoryStorage;
use crate::mpris::{ShazamPlayer, ShazamRoot};
use crate::network::{RecognizedSong, ShazamClient};

const PID_FILE: &str = "/tmp/shazam-scanner.pid";
const STATE_FILE: &str = "/tmp/waybar-shazam-state";
const CURRENT_FILE: &str = "/tmp/waybar-shazam-current";
const JSON_FILE: &str = "/tmp/waybar-shazam-json";

#[derive(Parser, Debug)]
#[command(name = "shazam-daemon", about = "High-performance Shazam audio recognition daemon and 320kbps downloader")]
struct Cli {
    #[arg(long, help = "Run as background daemon with JSON output for Waybar / Quickshell")]
    waybar: bool,

    #[arg(long, help = "Toggle listening state of running daemon")]
    toggle: bool,

    #[arg(long, help = "Print running daemon status")]
    status: bool,

    #[arg(long, help = "Audio capture source: auto, monitor, or mic", default_value = "auto")]
    source: String,

    #[arg(long, help = "Download a song by title and artist directly from JioSaavn 320kbps", num_args = 2, value_names = ["TITLE", "ARTIST"])]
    download: Option<Vec<String>>,

    #[arg(long, help = "Download the currently recognized song from running daemon")]
    download_current: bool,
}

fn read_pid() -> Option<i32> {
    if Path::new(PID_FILE).exists() {
        let content = fs::read_to_string(PID_FILE).ok()?;
        if let Ok(pid) = content.trim().parse::<i32>() {
            // Verify process is actually running in OS
            if unsafe { libc::kill(pid, 0) == 0 } {
                return Some(pid);
            }
        }
    }
    None
}

fn write_pid() {
    let pid = std::process::id();
    let _ = fs::write(PID_FILE, pid.to_string());
}

fn cleanup_files() {
    let _ = fs::remove_file(PID_FILE);
    let _ = fs::remove_file(CURRENT_FILE);
    let _ = fs::remove_file(STATE_FILE);
    let _ = fs::remove_file(JSON_FILE);
}

fn emit_waybar_state(text: &str, tooltip: &str, class: &str) {
    let data = serde_json::json!({
        "text": text,
        "tooltip": tooltip,
        "class": class
    });
    let _ = fs::write(JSON_FILE, data.to_string());
}

fn emit_paused() {
    let _ = fs::remove_file(STATE_FILE);
    let _ = fs::remove_file(CURRENT_FILE);
    emit_waybar_state("󰏤", "Shazam is paused. Click to listen.", "paused");
}

fn emit_listening() {
    let _ = fs::write(STATE_FILE, "active");
    emit_waybar_state("󰓅", "Shazam is listening (ambient)...", "ambient");
}

fn emit_found(song: &RecognizedSong) {
    let _ = fs::write(STATE_FILE, "active");
    let clean_title = song.title.split('(').next().unwrap_or(&song.title).trim();
    let text = format!("󰓅 {}", clean_title);

    let mut tooltip = format!(
        "<span size='13000' weight='bold'>{}</span>\n<span size='11000' color='#cccccc'><i>by</i> {}</span>",
        html_escape(&song.title),
        html_escape(&song.artist)
    );
    if let Some(album) = &song.album {
        tooltip.push_str(&format!("\n<b>Album:</b> {}", html_escape(album)));
    }
    if let Some(genre) = &song.genre {
        tooltip.push_str(&format!("\n<b>Genre:</b> {}", html_escape(genre)));
    }

    emit_waybar_state(&text, &tooltip, "found");
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn calculate_rms(samples: &[i16]) -> f32 {
    if samples.is_empty() { return 0.0; }
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    (sum_sq / (samples.len() as f64)).sqrt() as f32
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = Cli::parse();

    if let Some(args) = cli.download {
        let title = &args[0];
        let artist = &args[1];
        let downloader = JioSaavnDownloader::new();
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let download_dir = PathBuf::from(home).join("Music").join("ShazamLive");

        println!("Downloading: {} - {} (320kbps AAC)...", title, artist);
        match downloader.download_song(title, artist, download_dir).await {
            Ok(p) => {
                println!("Saved to: {}", p.display());
                return Ok(());
            }
            Err(e) => {
                eprintln!("Download error: {}", e);
                std::process::exit(1);
            }
        }
    }

    if cli.download_current {
        let current_text = fs::read_to_string(CURRENT_FILE).unwrap_or_default();
        if let Some((title, artist)) = current_text.split_once(" - ") {
            let downloader = JioSaavnDownloader::new();
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            let download_dir = PathBuf::from(home).join("Music").join("ShazamLive");

            println!("Downloading current track: {} - {} (320kbps AAC)...", title, artist);
            match downloader.download_song(title.trim(), artist.trim(), download_dir).await {
                Ok(p) => {
                    println!("Saved to: {}", p.display());
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("Download error: {}", e);
                    std::process::exit(1);
                }
            }
        } else {
            eprintln!("No song is currently recognized.");
            std::process::exit(1);
        }
    }

    if cli.toggle {
        if let Some(pid) = read_pid() {
            unsafe {
                libc::kill(pid, libc::SIGUSR1);
            }
            println!("Toggled daemon (PID {})", pid);
        } else {
            eprintln!("shazam-daemon is not running");
            std::process::exit(1);
        }
        return Ok(());
    }

    if cli.status {
        let running = read_pid().is_some();
        println!("{}", serde_json::json!({ "running": running }));
        return Ok(());
    }

    // Check single-instance
    if let Some(existing_pid) = read_pid() {
        if existing_pid != std::process::id() as i32 {
            eprintln!("Another instance of shazam-daemon is running (PID {})", existing_pid);
            std::process::exit(1);
        }
    }

    write_pid();

    let source_mode = match cli.source.to_lowercase().as_str() {
        "monitor" => AudioSourceMode::Monitor,
        "mic" => AudioSourceMode::Mic,
        _ => AudioSourceMode::Auto,
    };

    let audio_capture = AudioCapture::new(source_mode)?;
    let sig_gen = SignatureGenerator::new();
    let shazam_client = ShazamClient::new();
    let history_storage = HistoryStorage::new();

    let is_listening = Arc::new(AtomicBool::new(true)); // Start active listening
    let current_song = Arc::new(RwLock::new(None::<RecognizedSong>));

    // Register D-Bus MPRIS server
    let player_service = ShazamPlayer::new(is_listening.clone(), current_song.clone());
    let dbus_conn = Builder::session()?
        .name("org.mpris.MediaPlayer2.Shazam")?
        .serve_at("/org/mpris/MediaPlayer2", ShazamRoot)?
        .serve_at("/org/mpris/MediaPlayer2", player_service)?
        .build()
        .await?;

    // Setup UNIX signals
    let mut sigusr1 = signal(SignalKind::user_defined1())?;
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sighup = signal(SignalKind::hangup())?;

    emit_listening();

    let mut last_detected_id = String::new();
    let mut miss_count = 0;

    println!("Shazam high-performance daemon started (12s Ring Buffer Engine).");

    loop {
        tokio::select! {
            _ = sighup.recv() => {
                // Ignore SIGHUP
            }
            _ = sigusr1.recv() => {
                let current_state = is_listening.load(Ordering::Relaxed);
                let new_state = !current_state;
                is_listening.store(new_state, Ordering::Relaxed);

                if new_state {
                    audio_capture.clear_buffer().await;
                    last_detected_id.clear();
                    audio_capture.resume();
                    emit_listening();
                } else {
                    audio_capture.pause();
                    audio_capture.clear_buffer().await;
                    last_detected_id.clear();
                    *current_song.write().await = None;
                    emit_paused();
                }

                if let Ok(iface_ref) = dbus_conn.object_server().interface::<_, ShazamPlayer>("/org/mpris/MediaPlayer2").await {
                    let _ = iface_ref.get().await.playback_status_changed(iface_ref.signal_emitter()).await;
                    let _ = iface_ref.get().await.metadata_changed(iface_ref.signal_emitter()).await;
                }
            }
            _ = sigterm.recv() => {
                break;
            }
            _ = sigint.recv() => {
                break;
            }
            _ = tokio::time::sleep(Duration::from_millis(1000)) => {
                if !is_listening.load(Ordering::Relaxed) {
                    continue;
                }

                // Require at least 3.0s in the ring buffer (48,000 samples)
                if audio_capture.sample_count().await < 48000 {
                    continue;
                }

                // Extract the most recent 5-second chunk from the 12-second ring buffer
                let samples = audio_capture.extract_chunk(5).await;
                if samples.is_empty() {
                    continue;
                }

                // RMS Energy Gating: Skip DSP if audio is near absolute silence
                let rms = calculate_rms(&samples);
                if rms < 30.0 { // Below ~ -60 dBFS
                    continue;
                }

                // Generate signature from contiguous PCM samples
                let Some(sig_uri) = sig_gen.generate_from_i16(&samples) else {
                    continue;
                };

                let sample_ms = (samples.len() as f32 / 16.0) as u32;

                // Query Cloud API
                match shazam_client.recognize(&sig_uri, sample_ms).await {
                    Ok(Some(song)) => {
                        let song_id = song.display_id();
                        miss_count = 0;

                        if song_id != last_detected_id {
                            last_detected_id = song_id.clone();
                            history_storage.log_song(&song);
                            emit_found(&song);
                            let _ = fs::write(CURRENT_FILE, format!("{} - {}", song.title, song.artist));
                            *current_song.write().await = Some(song);

                            if let Ok(iface_ref) = dbus_conn.object_server().interface::<_, ShazamPlayer>("/org/mpris/MediaPlayer2").await {
                                let _ = iface_ref.get().await.metadata_changed(iface_ref.signal_emitter()).await;
                                let _ = iface_ref.get().await.playback_status_changed(iface_ref.signal_emitter()).await;
                            }
                        }

                        // Match Cooldown
                        tokio::time::sleep(Duration::from_secs(3)).await;
                    }
                    Ok(None) => {
                        miss_count += 1;
                        if miss_count >= 3 && !last_detected_id.is_empty() {
                            last_detected_id.clear();
                            *current_song.write().await = None;
                            let _ = fs::remove_file(CURRENT_FILE);
                            emit_listening();

                            if let Ok(iface_ref) = dbus_conn.object_server().interface::<_, ShazamPlayer>("/org/mpris/MediaPlayer2").await {
                                let _ = iface_ref.get().await.metadata_changed(iface_ref.signal_emitter()).await;
                            }
                        }
                    }
                    Err(_) => {
                        // Rate limit / network error backoff
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
        }
    }

    cleanup_files();
    Ok(())
}
