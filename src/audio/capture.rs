use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::Mutex;

const SAMPLE_RATE: usize = 16000;
const BUFFER_SECS: usize = 12;
const RING_CAPACITY: usize = SAMPLE_RATE * BUFFER_SECS; // 192,000 samples

#[derive(Clone, Copy)]
pub enum AudioSourceMode {
    Auto,
    Monitor,
    Mic,
}

pub struct RingBuffer {
    data: Vec<i16>,
    write_idx: usize,
    count: usize,
}

impl RingBuffer {
    pub fn new() -> Self {
        Self {
            data: vec![0i16; RING_CAPACITY],
            write_idx: 0,
            count: 0,
        }
    }

    pub fn push_slice(&mut self, samples: &[i16]) {
        for &s in samples {
            self.data[self.write_idx] = s;
            self.write_idx = (self.write_idx + 1) % RING_CAPACITY;
            if self.count < RING_CAPACITY {
                self.count += 1;
            }
        }
    }

    pub fn extract_recent(&self, num_samples: usize) -> Vec<i16> {
        let n = num_samples.min(self.count);
        let mut out = Vec::with_capacity(n);

        if self.count < RING_CAPACITY {
            // Buffer has not wrapped yet
            let start = self.write_idx.saturating_sub(n);
            out.extend_from_slice(&self.data[start..self.write_idx]);
        } else {
            // Buffer is full, read backwards from write_idx
            let start_idx = (self.write_idx + RING_CAPACITY - n) % RING_CAPACITY;
            if start_idx + n <= RING_CAPACITY {
                out.extend_from_slice(&self.data[start_idx..start_idx + n]);
            } else {
                let first_part = RING_CAPACITY - start_idx;
                let second_part = n - first_part;
                out.extend_from_slice(&self.data[start_idx..RING_CAPACITY]);
                out.extend_from_slice(&self.data[0..second_part]);
            }
        }

        out
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn clear(&mut self) {
        self.write_idx = 0;
        self.count = 0;
    }
}

pub struct AudioCapture {
    is_running: Arc<AtomicBool>,
    ring_buffer: Arc<Mutex<RingBuffer>>,
    shutdown: Arc<AtomicBool>,
}

impl AudioCapture {
    pub fn new(mode: AudioSourceMode) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = is_running.clone();
        let ring_buffer = Arc::new(Mutex::new(RingBuffer::new()));
        let ring_buffer_clone = ring_buffer.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        tokio::spawn(async move {
            while !shutdown_clone.load(Ordering::Relaxed) {
                let mut cmd = Command::new("pw-record");
                match mode {
                    AudioSourceMode::Monitor | AudioSourceMode::Auto => {
                        cmd.args(["-P", "stream.capture.sink=true"]);
                    }
                    AudioSourceMode::Mic => {
                        cmd.args(["--target", "@DEFAULT_AUDIO_SOURCE@"]);
                    }
                }

                cmd.args(["--format", "s16", "--rate", "16000", "--channels", "1", "-"]);
                cmd.kill_on_drop(true);
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
                        let mut lock = ring_buffer_clone.lock().await;
                        lock.push_slice(&samples);
                    }
                }

                let _ = child.start_kill();
                if !shutdown_clone.load(Ordering::Relaxed) {
                    // Quick recovery delay on PipeWire device change or stream reconnect
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
            }
        });

        Ok(Self {
            is_running,
            ring_buffer,
            shutdown,
        })
    }

    pub fn pause(&self) {
        self.is_running.store(false, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.is_running.store(true, Ordering::Relaxed);
    }

    pub async fn extract_chunk(&self, duration_secs: usize) -> Vec<i16> {
        let lock = self.ring_buffer.lock().await;
        let samples_wanted = duration_secs * SAMPLE_RATE;
        lock.extract_recent(samples_wanted)
    }

    pub async fn sample_count(&self) -> usize {
        let lock = self.ring_buffer.lock().await;
        lock.count()
    }

    pub async fn clear_buffer(&self) {
        let mut lock = self.ring_buffer.lock().await;
        lock.clear();
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}
