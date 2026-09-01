use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub enum AudioSourceMode {
    Auto,      // Attempts default sink monitor first (desktop audio), falls back to default mic
    Monitor,   // Force desktop audio output monitor
    Mic,       // Force microphone input
}

pub struct AudioCapture {
    _stream: Option<Stream>,
    consumer: Consumer<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    is_running: Arc<AtomicBool>,
}

impl AudioCapture {
    pub fn new(mode: AudioSourceMode) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let host = cpal::default_host();

        let device = match mode {
            AudioSourceMode::Mic => host
                .default_input_device()
                .ok_or("No default input device (microphone) found")?,
            AudioSourceMode::Monitor => Self::find_monitor_device(&host)?,
            AudioSourceMode::Auto => {
                Self::find_monitor_device(&host).unwrap_or_else(|_| {
                    host.default_input_device().expect("No audio input device found")
                })
            }
        };

        let default_config = device.default_input_config()?;
        let sample_rate = default_config.sample_rate().0;
        let channels = default_config.channels();
        let sample_format = default_config.sample_format();

        let config: StreamConfig = default_config.into();

        // 16 seconds buffer capacity at hardware rate
        let buffer_capacity = (sample_rate as usize) * (channels as usize) * 16;
        let (mut producer, consumer) = RingBuffer::<f32>::new(buffer_capacity);

        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = is_running.clone();

        let err_fn = |err| eprintln!("Audio stream error: {}", err);

        let stream = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                &config,
                move |data: &[f32], _| {
                    if is_running_clone.load(Ordering::Relaxed) {
                        for &sample in data {
                            let _ = producer.push(sample);
                        }
                    }
                },
                err_fn,
                None,
            )?,
            SampleFormat::I16 => device.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    if is_running_clone.load(Ordering::Relaxed) {
                        for &sample in data {
                            let f = (sample as f32) / 32768.0;
                            let _ = producer.push(f);
                        }
                    }
                },
                err_fn,
                None,
            )?,
            SampleFormat::U16 => device.build_input_stream(
                &config,
                move |data: &[u16], _| {
                    if is_running_clone.load(Ordering::Relaxed) {
                        for &sample in data {
                            let f = ((sample as f32) - 32768.0) / 32768.0;
                            let _ = producer.push(f);
                        }
                    }
                },
                err_fn,
                None,
            )?,
            _ => return Err("Unsupported audio sample format".into()),
        };

        stream.play()?;

        Ok(Self {
            _stream: Some(stream),
            consumer,
            sample_rate,
            channels,
            is_running,
        })
    }

    fn find_monitor_device(host: &cpal::Host) -> Result<Device, Box<dyn std::error::Error + Send + Sync>> {
        let input_devices = host.input_devices()?;
        for dev in input_devices {
            if let Ok(name) = dev.name() {
                let name_lower = name.to_lowercase();
                if name_lower.contains("monitor") || name_lower.contains("stereo mix") || name_lower.contains("what u hear") {
                    return Ok(dev);
                }
            }
        }
        // Fallback to default input
        host.default_input_device().ok_or("No input device found".into())
    }

    pub fn pause(&self) {
        self.is_running.store(false, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.is_running.store(true, Ordering::Relaxed);
    }

    pub fn read_available(&mut self) -> Vec<f32> {
        let count = self.consumer.slots();
        let mut samples = Vec::with_capacity(count);
        while let Ok(sample) = self.consumer.pop() {
            samples.push(sample);
        }
        samples
    }
}
