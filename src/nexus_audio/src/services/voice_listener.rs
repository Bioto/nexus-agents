use crate::error::{Result, VoiceError};
use crate::services::{AudioRecorder, RecordingConfig};
use cpal::traits::StreamTrait;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Calculate zero-crossing rate of audio signal
fn calculate_zero_crossing_rate(samples: &[f32]) -> f32 {
    if samples.len() < 2 {
        return 0.0;
    }

    let mut crossings = 0;
    let mut prev_sign = samples[0] >= 0.0;

    for &sample in &samples[1..] {
        let sign = sample >= 0.0;
        if sign != prev_sign {
            crossings += 1;
        }
        prev_sign = sign;
    }

    crossings as f32 / samples.len() as f32
}

/// Estimate pitch using simple autocorrelation method
fn estimate_pitch(samples: &[f32], sample_rate: u32) -> f32 {
    let min_period = (sample_rate as f32 / 400.0) as usize; // 400 Hz max
    let max_period = (sample_rate as f32 / 50.0) as usize; // 50 Hz min

    if samples.len() < max_period * 2 {
        return 0.0; // Not enough samples
    }

    let mut best_period = 0;
    let mut best_correlation = 0.0;

    // Simple autocorrelation
    for period in min_period..max_period.min(samples.len() / 2) {
        let mut correlation = 0.0;
        let mut count = 0;

        for i in 0..(samples.len() - period) {
            correlation += samples[i] * samples[i + period];
            count += 1;
        }

        if count > 0 {
            correlation /= count as f32;
            if correlation > best_correlation {
                best_correlation = correlation;
                best_period = period;
            }
        }
    }

    if best_period > 0 && best_correlation > 0.3 {
        sample_rate as f32 / best_period as f32
    } else {
        0.0 // No clear pitch detected
    }
}

/// Configuration for voice listening
#[derive(Debug, Clone)]
pub struct VoiceListenerConfig {
    /// Audio sample rate in Hz
    pub sample_rate: u32,
    /// Number of audio channels
    pub channels: u16,
    /// Optional device name
    pub device_name: Option<String>,
    /// RMS energy threshold for voice detection
    pub energy_threshold: f32,
    /// Zero-crossing rate threshold
    pub zcr_threshold: f32,
    /// Minimum voice frequency in Hz
    pub min_voice_freq: f32,
    /// Maximum voice frequency in Hz
    pub max_voice_freq: f32,
    /// Silence duration in milliseconds before ending speech
    pub silence_duration_ms: u32,
    /// Minimum speech duration in milliseconds
    pub min_speech_ms: u32,
    /// Path to Whisper model
    pub model_path: PathBuf,
    /// Enable verbose output
    pub verbose: bool,
}

impl Default for VoiceListenerConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            channels: 1,
            device_name: None,
            energy_threshold: 0.02,
            zcr_threshold: 0.25,
            min_voice_freq: 85.0,
            max_voice_freq: 255.0,
            silence_duration_ms: 5000,
            min_speech_ms: 1000,
            model_path: PathBuf::from(".models/ggml-small-fp16.bin"),
            verbose: false,
        }
    }
}

/// Voice detection metrics for verbose output
#[derive(Debug, Clone)]
pub struct VoiceMetrics {
    pub energy: f32,
    pub zero_crossing_rate: f32,
    pub dominant_frequency: f32,
    pub is_voice: bool,
}

/// Transcription result containing the text and metadata
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TranscriptionResult {
    /// The transcribed text
    pub text: String,
    /// Duration of the audio segment in seconds
    pub duration_seconds: f32,
    /// Timestamp when transcription was completed
    pub timestamp: std::time::SystemTime,
}

/// Trait for handling transcription results
#[allow(dead_code)]
pub trait TranscriptionHandler: Send + 'static {
    /// Called when a new transcription is available
    /// Return false to stop listening
    fn on_transcription(&mut self, result: TranscriptionResult) -> bool;

    /// Called when an error occurs during transcription
    fn on_error(&mut self, error: String) {
        log::error!("Transcription error: {}", error);
    }
}

/// Voice listener that handles continuous speech detection and transcription
pub struct VoiceListener {
    config: VoiceListenerConfig,
    recorder: AudioRecorder,
    running: Arc<AtomicBool>,
    transcription_tx: Option<mpsc::SyncSender<(Vec<f32>, f32)>>, // Added duration
    transcription_handle: Option<thread::JoinHandle<()>>,
}

impl VoiceListener {
    /// Create a new voice listener with the given configuration
    pub fn new(config: VoiceListenerConfig) -> Result<Self> {
        // Validate model path
        if !config.model_path.exists() {
            return Err(VoiceError::Configuration(format!(
                "Whisper model not found at: {}",
                config.model_path.display()
            )));
        }

        let recorder = AudioRecorder::new()?;
        let running = Arc::new(AtomicBool::new(false));

        Ok(Self {
            config,
            recorder,
            running,
            transcription_tx: None,
            transcription_handle: None,
        })
    }

    /// Start listening for voice and transcribing with a simple callback
    /// This is a convenience method that wraps start_with_channel
    pub fn start<F>(&mut self, on_transcription: F) -> Result<()>
    where
        F: Fn(&str) + Send + 'static,
    {
        let rx = self.start_with_channel()?;

        // Spawn a thread to handle the receiver and call the callback
        thread::spawn(move || {
            while let Ok(result) = rx.recv() {
                on_transcription(&result.text);
            }
        });

        Ok(())
    }

    /// Start listening and return a receiver for transcription results
    pub fn start_with_channel(&mut self) -> Result<mpsc::Receiver<TranscriptionResult>> {
        if self.running.load(Ordering::Relaxed) {
            return Err(VoiceError::Other(
                "Voice listener is already running".into(),
            ));
        }

        self.running.store(true, Ordering::Relaxed);

        // Create channels
        // Use a larger buffer to handle longer transcriptions without blocking
        // This allows multiple audio segments to queue while transcription is in progress
        let (tx_transcribe, rx_transcribe) = mpsc::sync_channel::<(Vec<f32>, f32)>(10);
        let (tx_results, rx_results) = mpsc::channel::<TranscriptionResult>();
        self.transcription_tx = Some(tx_transcribe);

        // Spawn transcription thread
        let model_path = self.config.model_path.clone();
        let running = Arc::clone(&self.running);

        let transcription_handle = thread::spawn(move || {
            // Create Whisper context in this thread
            let ctx = match WhisperContext::new_with_params(
                &model_path.to_string_lossy(),
                WhisperContextParameters::default(),
            ) {
                Ok(ctx) => ctx,
                Err(e) => {
                    log::error!("Failed to create Whisper context: {}", e);
                    return;
                }
            };

            // Process transcription requests
            log::debug!("Transcription thread started, waiting for audio data...");
            while running.load(Ordering::Relaxed) {
                match rx_transcribe.recv_timeout(Duration::from_millis(100)) {
                    Ok((audio_data, duration_seconds)) => {
                        log::debug!(
                            "Received audio data for transcription: {} samples, {:.2}s",
                            audio_data.len(),
                            duration_seconds
                        );
                        match ctx.create_state() {
                            Ok(mut state) => {
                                let mut params =
                                    FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
                                params.set_language(Some("en"));
                                params.set_translate(false);
                                params.set_print_progress(false);
                                params.set_print_special(false);

                                log::debug!("Running Whisper transcription...");
                                if let Err(e) = state.full(params, &audio_data) {
                                    log::error!("Transcription error: {}", e);
                                    continue;
                                }

                                let num_segments = state.full_n_segments();
                                let mut transcription = String::new();

                                for i in 0..num_segments {
                                    if let Some(segment) = state.get_segment(i) {
                                        transcription.push_str(&format!("{}", segment));
                                        transcription.push(' ');
                                    }
                                }

                                let text = transcription.trim().to_string();
                                log::debug!(
                                    "Transcription result: \"{}\" ({} segments)",
                                    text,
                                    num_segments
                                );
                                if !text.is_empty() {
                                    let result = TranscriptionResult {
                                        text,
                                        duration_seconds,
                                        timestamp: std::time::SystemTime::now(),
                                    };

                                    log::debug!("Sending transcription result to channel...");
                                    if tx_results.send(result).is_err() {
                                        log::warn!("Failed to send transcription result - receiver dropped");
                                        // Receiver dropped, exit
                                        break;
                                    } else {
                                        log::debug!("Successfully sent transcription result");
                                    }
                                } else {
                                    log::debug!("Transcription is empty, skipping");
                                }
                            }
                            Err(e) => {
                                log::error!("Failed to create Whisper state: {}", e);
                            }
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        log::warn!("Transcription channel disconnected");
                        break;
                    }
                }
            }
            log::debug!("Transcription thread exiting");
        });

        self.transcription_handle = Some(transcription_handle);
        Ok(rx_results)
    }

    /// Start listening with a custom transcription handler
    #[allow(dead_code)]
    pub fn start_with_handler<H>(&mut self, mut handler: H) -> Result<()>
    where
        H: TranscriptionHandler,
    {
        let rx = self.start_with_channel()?;

        // Spawn a thread to handle the receiver with the handler
        let running = Arc::clone(&self.running);
        thread::spawn(move || {
            while let Ok(result) = rx.recv() {
                if !handler.on_transcription(result) {
                    // Handler requested to stop
                    running.store(false, Ordering::Relaxed);
                    break;
                }
            }
        });

        Ok(())
    }

    /// Process audio chunks and detect voice
    pub fn listen<F>(&self, mut on_metrics: F) -> Result<()>
    where
        F: FnMut(VoiceMetrics),
    {
        // Build recording configuration
        let config = RecordingConfig {
            sample_rate: self.config.sample_rate,
            channels: self.config.channels,
            duration: None,
            device_name: self.config.device_name.clone(),
        };

        // Start audio stream
        let (stream, rx, _actual_sr, _actual_ch) = self.recorder.stream_audio_chunks(config)?;
        stream
            .play()
            .map_err(|e| VoiceError::Audio(format!("Failed to start stream: {}", e)))?;

        let mut audio_buffer = Vec::new();
        let mut speech_buffer = Vec::new();
        let mut is_speaking = false;
        let mut silence_duration = 0;

        while self.running.load(Ordering::Relaxed) {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(chunk) => {
                    audio_buffer.extend_from_slice(&chunk);

                    // Voice detection
                    let energy: f32 =
                        chunk.iter().map(|&s| s * s).sum::<f32>() / chunk.len() as f32;
                    let energy_sqrt = energy.sqrt();
                    let zcr = calculate_zero_crossing_rate(&chunk);
                    let dominant_freq = estimate_pitch(&chunk, self.config.sample_rate);

                    let is_voice = energy_sqrt > self.config.energy_threshold
                        && zcr < self.config.zcr_threshold
                        && (dominant_freq == 0.0
                            || (dominant_freq >= self.config.min_voice_freq
                                && dominant_freq <= self.config.max_voice_freq));

                    // Report metrics if verbose
                    if self.config.verbose && energy_sqrt > 0.001 {
                        on_metrics(VoiceMetrics {
                            energy: energy_sqrt,
                            zero_crossing_rate: zcr,
                            dominant_frequency: dominant_freq,
                            is_voice,
                        });
                    }

                    if is_voice {
                        if !is_speaking {
                            log::debug!("Voice detected, starting speech capture");
                            is_speaking = true;
                        }
                        silence_duration = 0;
                        speech_buffer.extend_from_slice(&chunk);
                    } else if is_speaking {
                        silence_duration += 100; // We check every 100ms

                        // Continue recording during short pauses
                        if silence_duration <= self.config.silence_duration_ms / 2 {
                            speech_buffer.extend_from_slice(&chunk);
                        }

                        if silence_duration >= self.config.silence_duration_ms {
                            log::debug!("Silence detected ({}ms), ending speech capture. Buffer size: {} samples", 
                                silence_duration, speech_buffer.len());
                            is_speaking = false;

                            // Only transcribe if we have enough audio
                            let speech_duration_ms =
                                (speech_buffer.len() as f32 / self.config.sample_rate as f32
                                    * 1000.0) as u32;

                            log::debug!(
                                "Speech duration: {}ms (min required: {}ms)",
                                speech_duration_ms,
                                self.config.min_speech_ms
                            );
                            if speech_duration_ms >= self.config.min_speech_ms {
                                // Send to transcription thread with duration
                                let duration_seconds =
                                    speech_buffer.len() as f32 / self.config.sample_rate as f32;
                                log::debug!(
                                    "Sending {} samples ({:.2}s) to transcription thread",
                                    speech_buffer.len(),
                                    duration_seconds
                                );
                                if let Some(ref tx) = self.transcription_tx {
                                    // Use blocking send instead of try_send to ensure audio segments
                                    // are not dropped. This will wait if the queue is full, ensuring
                                    // continuous listening even during long transcriptions.
                                    match tx.send((speech_buffer.clone(), duration_seconds)) {
                                        Ok(()) => {
                                            log::debug!(
                                                "Successfully sent audio to transcription thread"
                                            );
                                        }
                                        Err(mpsc::SendError(_)) => {
                                            log::error!("Transcription thread disconnected");
                                            break;
                                        }
                                    }
                                } else {
                                    log::warn!(
                                        "transcription_tx is None - transcription not initialized?"
                                    );
                                }
                            }

                            speech_buffer.clear();
                            silence_duration = 0;
                        }
                    }

                    // Keep buffer size reasonable
                    if audio_buffer.len() > self.config.sample_rate as usize * 30 {
                        let keep_samples = self.config.sample_rate as usize * 10;
                        audio_buffer.drain(0..audio_buffer.len() - keep_samples);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    log::error!("Audio stream disconnected");
                    break;
                }
            }
        }

        stream
            .pause()
            .map_err(|e| VoiceError::Audio(format!("Failed to stop stream: {}", e)))?;

        Ok(())
    }

    /// Stop listening
    #[allow(dead_code)]
    pub fn stop(&mut self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);

        // Close transcription channel
        if let Some(tx) = self.transcription_tx.take() {
            drop(tx);
        }

        // Wait for transcription thread
        if let Some(handle) = self.transcription_handle.take() {
            if let Err(e) = handle.join() {
                log::error!("Transcription thread panicked: {:?}", e);
            }
        }

        Ok(())
    }

    /// Check if the listener is currently running
    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}
