use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Copy)]
pub enum AudioSourceMode {
    Auto,
    Monitor,
    Mic,
}

pub struct AudioCapture {
    is_running: Arc<AtomicBool>,
    buffer: Arc<Mutex<Vec<i16>>>,
    shutdown: Arc<AtomicBool>,
}

impl AudioCapture {
    pub fn new(mode: AudioSourceMode) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = is_running.clone();
        let buffer = Arc::new(Mutex::new(Vec::with_capacity(16000 * 16)));
        let buffer_clone = buffer.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        tokio::spawn(async move {
            while !shutdown_clone.load(Ordering::Relaxed) {
                let mut cmd = Command::new("pw-record");
                match mode {
                    AudioSourceMode::Monitor => {
                        cmd.args(["--target", "@DEFAULT_AUDIO_SINK@.monitor"]);
                    }
                    AudioSourceMode::Mic => {
                        cmd.args(["--target", "@DEFAULT_AUDIO_SOURCE@"]);
                    }
                    AudioSourceMode::Auto => {
                        // Default system recording stream
                    }
                }

                cmd.args(["--format", "s16", "--rate", "16000", "--channels", "1", "-"]);
                cmd.stdout(Stdio::piped());
                cmd.stderr(Stdio::null());

                let Ok(mut child) = cmd.spawn() else {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                };

                let Some(mut stdout) = child.stdout.take() else {
                    let _ = child.start_kill();
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                };

                let mut chunk = [0u8; 4096];
                while let Ok(n) = stdout.read(&mut chunk).await {
                    if n == 0 || shutdown_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    if is_running_clone.load(Ordering::Relaxed) {
                        let num_samples = n / 2;
                        let mut samples = Vec::with_capacity(num_samples);
                        for i in 0..num_samples {
                            let sample = i16::from_le_bytes([chunk[i * 2], chunk[i * 2 + 1]]);
                            samples.push(sample);
                        }
                        let mut lock = buffer_clone.lock().await;
                        lock.extend_from_slice(&samples);
                        // Keep last 16 seconds
                        if lock.len() > 16000 * 16 {
                            let excess = lock.len() - 16000 * 16;
                            lock.drain(0..excess);
                        }
                    }
                }

                let _ = child.start_kill();
                if !shutdown_clone.load(Ordering::Relaxed) {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
            }
        });

        Ok(Self {
            is_running,
            buffer,
            shutdown,
        })
    }

    pub fn pause(&self) {
        self.is_running.store(false, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.is_running.store(true, Ordering::Relaxed);
    }

    pub async fn read_available(&self) -> Vec<i16> {
        let lock = self.buffer.lock().await;
        lock.clone()
    }

    pub async fn clear_buffer(&self) {
        let mut lock = self.buffer.lock().await;
        lock.clear();
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}
