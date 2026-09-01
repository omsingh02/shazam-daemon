use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

pub enum AudioSourceMode {
    Auto,
    Monitor,
    Mic,
}

pub struct AudioCapture {
    child: Arc<Mutex<Option<Child>>>,
    is_running: Arc<AtomicBool>,
    buffer: Arc<Mutex<Vec<i16>>>,
}

impl AudioCapture {
    pub fn new(mode: AudioSourceMode) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = is_running.clone();
        let buffer = Arc::new(Mutex::new(Vec::with_capacity(16000 * 16)));
        let buffer_clone = buffer.clone();

        let mut cmd = Command::new("pw-record");
        match mode {
            AudioSourceMode::Monitor | AudioSourceMode::Auto => {
                cmd.args(["--target", "@DEFAULT_AUDIO_SINK@.monitor"]);
            }
            AudioSourceMode::Mic => {
                cmd.args(["--target", "@DEFAULT_AUDIO_SOURCE@"]);
            }
        }

        cmd.args(["--format", "s16", "--rate", "16000", "--channels", "1", "-"]);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(_) => {
                // Fallback to generic pw-record without target
                let mut fallback_cmd = Command::new("pw-record");
                fallback_cmd.args(["--format", "s16", "--rate", "16000", "--channels", "1", "-"]);
                fallback_cmd.stdout(Stdio::piped());
                fallback_cmd.stderr(Stdio::null());
                fallback_cmd.spawn()?
            }
        };

        let mut stdout = child.stdout.take().ok_or("Failed to open pw-record stdout")?;

        tokio::spawn(async move {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut chunk).await {
                if n == 0 { break; }
                if is_running_clone.load(Ordering::Relaxed) {
                    let num_samples = n / 2;
                    let mut samples = Vec::with_capacity(num_samples);
                    for i in 0..num_samples {
                        let sample = i16::from_le_bytes([chunk[i * 2], chunk[i * 2 + 1]]);
                        samples.push(sample);
                    }
                    let mut lock = buffer_clone.lock().await;
                    lock.extend_from_slice(&samples);
                    // Cap buffer at 16 seconds (16000 * 16 samples)
                    if lock.len() > 16000 * 16 {
                        let excess = lock.len() - 16000 * 16;
                        lock.drain(0..excess);
                    }
                }
            }
        });

        Ok(Self {
            child: Arc::new(Mutex::new(Some(child))),
            is_running,
            buffer,
        })
    }

    pub fn pause(&self) {
        self.is_running.store(false, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.is_running.store(true, Ordering::Relaxed);
    }

    pub async fn read_available(&self) -> Vec<i16> {
        let mut lock = self.buffer.lock().await;
        let data = lock.clone();
        data
    }

    pub async fn clear_buffer(&self) {
        let mut lock = self.buffer.lock().await;
        lock.clear();
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        if let Ok(mut lock) = self.child.try_lock() {
            if let Some(mut c) = lock.take() {
                let _ = c.start_kill();
            }
        }
    }
}
