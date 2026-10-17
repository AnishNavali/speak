use crate::{config::Settings, engine::Core};
use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Weak,
    },
    thread,
    time::{Duration, Instant},
};

pub enum Command {
    Start(u64, Settings),
    Stop,
    Cancel,
}

pub fn microphones() -> Result<Vec<String>> {
    cpal::default_host()
        .input_devices()?
        .map(|device| device.name().map_err(Into::into))
        .collect()
}

struct Capture {
    stream: cpal::Stream,
    receiver: mpsc::Receiver<Vec<f32>>,
    fault: Arc<AtomicBool>,
    id: u64,
    rate: u32,
    segment: Vec<f32>,
    silence: usize,
    threshold: f32,
    started: Instant,
    last_audio: Instant,
}

fn callback<T: cpal::Sample + cpal::SizedSample + Copy + 'static>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sender: mpsc::SyncSender<Vec<f32>>,
    fault: Arc<AtomicBool>,
    convert: fn(T) -> f32,
) -> Result<cpal::Stream> {
    let channels = config.channels as usize;
    let error = fault.clone();
    Ok(device.build_input_stream(
        config,
        move |data: &[T], _| {
            let samples = data
                .chunks_exact(channels)
                .map(|frame| frame.iter().copied().map(convert).sum::<f32>() / channels as f32)
                .collect();
            if sender.try_send(samples).is_err() {
                fault.store(true, Ordering::Relaxed);
            }
        },
        move |_| {
            error.store(true, Ordering::Relaxed);
        },
        None,
    )?)
}

impl Capture {
    fn open(id: u64, settings: &Settings) -> Result<Self> {
        let host = cpal::default_host();
        let device = if settings.microphone.is_empty() { host.default_input_device() }
            else { host.input_devices()?.find(|d| d.name().ok().as_deref() == Some(&settings.microphone)) }
            .context("Microphone unavailable. Select an input device and allow microphone access in system settings.")?;
        let supported = device.default_input_config()?;
        let config: cpal::StreamConfig = supported.clone().into();
        if !(8000..=96000).contains(&config.sample_rate.0) || config.channels == 0 {
            bail!("Select a microphone with an 8–96 kHz recording format");
        }
        let (sender, receiver) = mpsc::sync_channel(128);
        let fault = Arc::new(AtomicBool::new(false));
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => {
                callback(&device, &config, sender, fault.clone(), |v: f32| v)
            }
            cpal::SampleFormat::I16 => {
                callback(&device, &config, sender, fault.clone(), |v: i16| {
                    v as f32 / 32768.
                })
            }
            cpal::SampleFormat::U16 => {
                callback(&device, &config, sender, fault.clone(), |v: u16| {
                    (v as f32 - 32768.) / 32768.
                })
            }
            format => {
                bail!("Microphone format {format:?} is not supported; choose another input device")
            }
        }?;
        stream
            .play()
            .context("Could not start microphone. Check microphone permission.")?;
        Ok(Self {
            stream,
            receiver,
            fault,
            id,
            rate: config.sample_rate.0,
            segment: Vec::new(),
            silence: 0,
            threshold: settings.pause_threshold,
            started: Instant::now(),
            last_audio: Instant::now(),
        })
    }

    fn drain(&mut self, core: &Core) {
        while let Ok(samples) = self.receiver.try_recv() {
            self.last_audio = Instant::now();
            let rms =
                (samples.iter().map(|v| v * v).sum::<f32>() / samples.len().max(1) as f32).sqrt();
            core.level(self.id, rms, self.started.elapsed().as_secs());
            self.silence = if rms < self.threshold {
                self.silence + samples.len()
            } else {
                0
            };
            for segment in bounded_segments(&mut self.segment, &samples, self.rate as usize * 24) {
                core.chunk(self.id, segment, self.rate);
                self.silence = 0;
            }
            // ponytail: energy-based pauses with a 24-second ceiling, not a trained VAD.
            // No quiet speech is discarded; use a VAD if noisy rooms cause frequent hard cuts.
            if should_split(self.segment.len(), self.silence, self.rate) {
                self.flush(core);
            }
        }
    }

    fn flush(&mut self, core: &Core) {
        if !self.segment.is_empty() {
            core.chunk(self.id, std::mem::take(&mut self.segment), self.rate);
        }
        self.silence = 0;
    }

    fn stop(mut self, core: &Core, cancel: bool) {
        let _ = self.stream.pause();
        self.drain(core);
        if !cancel {
            self.flush(core);
            core.finish_capture(self.id);
        }
    }
}

pub fn should_split(samples: usize, silence: usize, rate: u32) -> bool {
    samples >= rate as usize * 24
        || (samples >= rate as usize * 4 && silence as f64 / rate as f64 >= 0.7)
}

fn bounded_segments(pending: &mut Vec<f32>, mut samples: &[f32], limit: usize) -> Vec<Vec<f32>> {
    let mut complete = Vec::new();
    while !samples.is_empty() {
        let count = (limit - pending.len()).min(samples.len());
        pending.extend_from_slice(&samples[..count]);
        samples = &samples[count..];
        if pending.len() == limit {
            complete.push(std::mem::take(pending));
        }
    }
    complete
}

pub fn spawn(core: Weak<Core>) -> mpsc::Sender<Command> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut capture: Option<Capture> = None;
        loop {
            let Some(core) = core.upgrade() else { break };
            match receiver.recv_timeout(Duration::from_millis(15)) {
                Ok(Command::Start(id, settings)) => match Capture::open(id, &settings) {
                    Ok(value) => {
                        capture = Some(value);
                        core.recording(id);
                    }
                    Err(error) => {
                        core.capture_failed(id, format!("{error:#}"));
                        core.finish_capture(id);
                    }
                },
                Ok(Command::Stop) => {
                    if let Some(value) = capture.take() {
                        value.stop(&core, false);
                    }
                }
                Ok(Command::Cancel) => {
                    if let Some(value) = capture.take() {
                        value.stop(&core, true);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Some(value) = capture.as_mut() {
                value.drain(&core);
                if value.fault.load(Ordering::Relaxed)
                    || value.last_audio.elapsed() > Duration::from_secs(2)
                {
                    let value = capture.take().unwrap();
                    let id = value.id;
                    core.capture_failed(id, "Microphone disconnected or audio capture could not keep up. Captured audio is retained; retry or export it.".into());
                    value.stop(&core, false);
                } else if value.started.elapsed() >= Duration::from_secs(600) {
                    // ponytail: bound in-memory dictation to ten minutes; disk-backed sessions for longer capture.
                    capture.take().unwrap().stop(&core, false);
                }
            }
        }
    });
    sender
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn segment_limit_and_pause_floor() {
        assert!(!should_split(16000 * 3, 16000, 16000));
        assert!(should_split(16000 * 4, 11200, 16000));
        assert!(should_split(48000 * 24, 0, 48000));
        assert!(!should_split(48000 * 23, 0, 48000));
        let mut pending = vec![0., 1.];
        let output = bounded_segments(&mut pending, &[2., 3., 4., 5., 6.], 3);
        assert_eq!(output, vec![vec![0., 1., 2.], vec![3., 4., 5.]]);
        assert_eq!(pending, vec![6.]);
    }
}
