use crate::error::{Result, VoiceError};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::StreamConfig;
use rodio::{Decoder, OutputStream, Sink};
use std::io::Cursor;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::{Message, client::IntoClientRequest}};
use futures_util::{SinkExt, StreamExt};
use url::Url;
use rmp_serde::{Deserializer, Serializer};
use serde::{Deserialize, Serialize};

// Text-to-speech service using Kyutai TTS.
//
// # Setup
//
// This service supports three modes:
//
// 1. Local Python execution:
//    - Install Kyutai TTS: `pip install kyutai-tts` or follow Kyutai setup instructions
//    - Set KYUTAI_TTS_MODE=local (or use --local flag)
//    - The service will call Python directly
//
// 2. HTTP REST API mode:
//    - Start a Kyutai TTS server (e.g., using Unmute framework)
//    - Set KYUTAI_TTS_URL environment variable or use --endpoint
//    - Default endpoint: http://localhost:8089/api/tts_streaming
//
// 3. WebSocket RPC mode (recommended for better performance):
//    - Start moshi-server: `moshi-server worker --config configs/config-tts.toml`
//    - Set KYUTAI_TTS_MODE=websocket or use --websocket flag
//    - Default WebSocket URL: ws://localhost:8089/api/tts_streaming
//    - Uses MessagePack encoding for efficient streaming

/// Configuration for text-to-speech synthesis
#[derive(Debug, Clone)]
pub struct TtsConfig {
    /// Kyutai TTS server endpoint URL (None = use local Python execution)
    /// For WebSocket mode, use ws:// or wss:// protocol
    pub endpoint: Option<String>,
    /// Use local Python execution instead of HTTP/WebSocket server
    pub local: bool,
    /// Use WebSocket RPC mode instead of HTTP REST (faster, streaming)
    pub websocket: bool,
    /// Python command/path (default: "python3")
    pub python_cmd: Option<String>,
    /// Voice to use (None = default voice)
    pub voice: Option<String>,
    /// Speech rate/speed (0.0 to 1.0, where 0.5 is normal speed)
    pub rate: Option<f32>,
    /// Speech volume (0.0 to 1.0)
    pub volume: Option<f32>,
    /// Language code (e.g., "en", "fr")
    pub language: Option<String>,
}

impl Default for TtsConfig {
    fn default() -> Self {
        // Check mode via environment variable
        let mode = std::env::var("KYUTAI_TTS_MODE")
            .unwrap_or_else(|_| "websocket".to_string()); // Default to WebSocket for better performance
        
        let local = mode == "local";
        let websocket = mode == "websocket" || mode == "ws";

        // Default endpoint based on mode
        let endpoint = if local {
            None
        } else if websocket {
            Some(
                std::env::var("KYUTAI_TTS_URL")
                    .unwrap_or_else(|_| "ws://localhost:8089/api/tts_streaming".to_string()),
            )
        } else {
            // HTTP mode
            Some(
                std::env::var("KYUTAI_TTS_URL")
                    .unwrap_or_else(|_| "http://localhost:8089/api/tts_streaming".to_string()),
            )
        };

        Self {
            endpoint,
            local,
            websocket,
            python_cmd: None,
            voice: None,
            rate: Some(0.5),
            volume: Some(1.0),
            language: None,
        }
    }
}

/// Message types for WebSocket RPC protocol
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum TtsMessage {
    #[serde(rename = "Text")]
    Text { text: String },
    #[serde(rename = "Eos")]
    Eos,
    #[serde(rename = "Audio")]
    Audio { pcm: Vec<f32> },
    #[serde(rename = "Ready")]
    Ready,
}

/// Text-to-speech service using Kyutai TTS
pub struct TextToSpeech {
    http_client: Option<reqwest::Client>,
    endpoint: Option<String>,
    python_cmd: String,
    local_mode: bool,
    websocket_mode: bool,
    current_sink: Arc<Mutex<Option<Sink>>>,
}

impl TextToSpeech {
    /// Create a new TTS instance with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(TtsConfig::default())
    }

    /// Create a new TTS instance with configuration
    pub fn with_config(config: TtsConfig) -> Result<Self> {
        let local_mode = config.local;
        let websocket_mode = config.websocket && !local_mode;

        let (http_client, endpoint) = if local_mode {
            (None, None)
        } else if websocket_mode {
            // WebSocket mode - no HTTP client needed
            (None, config.endpoint)
        } else {
            // HTTP mode
            let endpoint = config.endpoint.or_else(|| {
                std::env::var("KYUTAI_TTS_URL").ok()
            }).unwrap_or_else(|| "http://localhost:8089/api/tts_streaming".to_string());

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| VoiceError::Api(format!("Failed to create HTTP client: {}", e)))?;

            (Some(client), Some(endpoint))
        };

        let python_cmd = config.python_cmd
            .or_else(|| std::env::var("PYTHON").ok())
            .unwrap_or_else(|| "python3".to_string());

        Ok(Self {
            http_client,
            endpoint,
            python_cmd,
            local_mode,
            websocket_mode,
            current_sink: Arc::new(Mutex::new(None)),
        })
    }

    /// Generate speech from text using Kyutai TTS
    async fn synthesize(&self, text: &str, config: &TtsConfig) -> Result<Vec<u8>> {
        if self.local_mode {
            self.synthesize_local(text, config).await
        } else if self.websocket_mode {
            self.synthesize_websocket(text, config).await
        } else {
            self.synthesize_http(text, config).await
        }
    }

    /// Generate speech using local Python subprocess
    async fn synthesize_local(&self, text: &str, config: &TtsConfig) -> Result<Vec<u8>> {
        // Create a Python script to call Kyutai TTS
        let script = format!(
            r#"
import sys
import json
import base64
from pathlib import Path

try:
    # Try to import Kyutai TTS - adjust import based on actual package
    # This is a template - you may need to adjust based on actual Kyutai API
    try:
        from kyutai import TTS
        tts = TTS()
    except ImportError:
        # Fallback: try other common import paths
        try:
            from unmute.tts import TTS
            tts = TTS()
        except ImportError:
            print(json.dumps({{"error": "Kyutai TTS not installed. Install with: pip install kyutai-tts"}}), file=sys.stderr)
            sys.exit(1)
    
    # Generate speech
    text = sys.argv[1]
    voice = sys.argv[2] if len(sys.argv) > 2 and sys.argv[2] != "None" else None
    speed = float(sys.argv[3]) if len(sys.argv) > 3 and sys.argv[3] != "None" else 0.5
    language = sys.argv[4] if len(sys.argv) > 4 and sys.argv[4] != "None" else None
    
    # Generate audio (adjust API call based on actual Kyutai TTS API)
    audio = tts.synthesize(text, voice=voice, speed=speed, language=language)
    
    # Output as base64 encoded WAV
    import io
    import wave
    buffer = io.BytesIO()
    with wave.open(buffer, 'wb') as wav_file:
        wav_file.setnchannels(1)  # Mono
        wav_file.setsampwidth(2)   # 16-bit
        wav_file.setframerate(24000)  # 24kHz (adjust as needed)
        wav_file.writeframes(audio.tobytes())
    
    print(base64.b64encode(buffer.getvalue()).decode())
    
except Exception as e:
    print(json.dumps({{"error": str(e)}}), file=sys.stderr)
    sys.exit(1)
"#
        );

        // Write script to temp file
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("kyutai_tts_{}.py", std::process::id()));
        std::fs::write(&script_path, script)
            .map_err(|e| VoiceError::Io(e))?;

        // Build command
        let mut cmd = Command::new(&self.python_cmd);
        cmd.arg(&script_path);
        cmd.arg(text);
        cmd.arg(config.voice.as_ref().map(|v| v.as_str()).unwrap_or("None"));
        cmd.arg(config.rate.map(|r| r.to_string()).unwrap_or_else(|| "0.5".to_string()));
        cmd.arg(config.language.as_ref().map(|l| l.as_str()).unwrap_or("None"));

        // Execute and capture output
        let output = tokio::process::Command::from(cmd)
            .output()
            .await
            .map_err(|e| VoiceError::Api(format!("Failed to execute Python: {}", e)))?;

        // Clean up temp script
        let _ = std::fs::remove_file(&script_path);

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(VoiceError::Api(format!(
                "Kyutai TTS Python error: {}. Make sure Kyutai TTS is installed: pip install kyutai-tts",
                error
            )));
        }

        // Decode base64 audio
        let audio_b64 = String::from_utf8(output.stdout)
            .map_err(|e| VoiceError::Api(format!("Invalid output from Python: {}", e)))?;
        
        use base64::Engine;
        let audio_data = base64::engine::general_purpose::STANDARD
            .decode(audio_b64.trim())
            .map_err(|e| VoiceError::Api(format!("Failed to decode audio: {}", e)))?;

        Ok(audio_data)
    }

    /// Generate speech using HTTP API
    async fn synthesize_http(&self, text: &str, config: &TtsConfig) -> Result<Vec<u8>> {
        let endpoint = self.endpoint.as_ref().ok_or_else(|| {
            VoiceError::Configuration("HTTP endpoint not configured".to_string())
        })?;

        let http_client = self.http_client.as_ref().ok_or_else(|| {
            VoiceError::Configuration("HTTP client not initialized".to_string())
        })?;

        log::info!("Calling TTS endpoint: {}", endpoint);
        log::info!("Text to synthesize: {}", text);

        // Build request payload for Moshi/Kyutai TTS
        // Moshi server requires: { "text": "...", "voice": "..." }
        // Default voice from config or use a common default
        let voice = config.voice.clone().unwrap_or_else(|| {
            std::env::var("KYUTAI_TTS_VOICE")
                .unwrap_or_else(|_| "expresso/ex03-ex01_happy_001_channel1_334s.wav".to_string())
        });

        let mut payload = serde_json::json!({
            "text": text,
            "voice": voice,
        });

        if let Some(rate) = config.rate {
            payload["speed"] = serde_json::Value::Number(serde_json::Number::from_f64(rate as f64).unwrap());
        }

        if let Some(ref language) = config.language {
            payload["language"] = serde_json::Value::String(language.clone());
        }

        log::debug!("Request payload: {}", serde_json::to_string_pretty(&payload).unwrap_or_default());

        // Make HTTP request to Moshi/Kyutai TTS
        // Check if API key is needed (from config or env)
        let api_key = std::env::var("KYUTAI_API_KEY")
            .unwrap_or_else(|_| "public_token".to_string());

        let request = http_client
            .post(endpoint)
            .header("Content-Type", "application/json")
            .header("kyutai-api-key", &api_key);

        let response = request
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                let err_msg = format!(
                    "Failed to connect to TTS server at {}: {}. Make sure the server is running.",
                    endpoint, e
                );
                log::error!("{}", err_msg);
                VoiceError::Api(err_msg)
            })?;

        let status = response.status();
        log::info!("TTS server response status: {}", status);

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            let err_msg = format!("TTS server error ({}): {}", status, error_text);
            log::error!("{}", err_msg);
            return Err(VoiceError::Api(err_msg));
        }

        // Check content type to determine audio format
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_string();
        log::info!("Response content-type: {}", content_type);

        // Get audio data
        let audio_data = response
            .bytes()
            .await
            .map_err(|e| {
                let err_msg = format!("Failed to read audio data: {}", e);
                log::error!("{}", err_msg);
                VoiceError::Api(err_msg)
            })?;

        log::info!("Received {} bytes of audio data", audio_data.len());

        // If it's WAV format, ensure it's properly formatted
        // The server might return raw WAV, so we'll let rodio try to decode it
        // If that fails, we'll try to handle it as raw PCM
        Ok(audio_data.to_vec())
    }

    /// Generate speech using WebSocket RPC (faster, streaming)
    async fn synthesize_websocket(&self, text: &str, config: &TtsConfig) -> Result<Vec<u8>> {
        let endpoint = self.endpoint.as_ref().ok_or_else(|| {
            VoiceError::Configuration("WebSocket endpoint not configured".to_string())
        })?;

        log::info!("Connecting to TTS WebSocket: {}", endpoint);
        log::info!("Text to synthesize: {}", text);

        // Parse URL and add query parameters
        let mut url = Url::parse(endpoint)
            .map_err(|e| VoiceError::Api(format!("Invalid WebSocket URL: {}", e)))?;

        // Get voice from config or environment
        let voice = config.voice.clone().unwrap_or_else(|| {
            std::env::var("KYUTAI_TTS_VOICE")
                .unwrap_or_else(|_| "expresso/ex03-ex01_happy_001_channel1_334s.wav".to_string())
        });

        // Add query parameters
        url.query_pairs_mut()
            .append_pair("voice", &voice)
            .append_pair("format", "PcmMessagePack");

        log::debug!("WebSocket URL with params: {}", url);

        // Get API key
        let api_key = std::env::var("KYUTAI_API_KEY")
            .unwrap_or_else(|_| "public_token".to_string());

        // Connect to WebSocket - use tungstenite's client request builder which handles handshake
        // The IntoClientRequest trait creates a request with proper WebSocket handshake headers,
        // then we add our custom header
        let mut request = url.as_str()
            .into_client_request()
            .map_err(|e| VoiceError::Api(format!("Failed to create WebSocket request: {}", e)))?;
        
        // Add custom API key header
        use http::HeaderValue;
        request.headers_mut().insert(
            "kyutai-api-key",
            HeaderValue::from_str(&api_key)
                .map_err(|e| VoiceError::Api(format!("Failed to create header value: {}", e)))?
        );

        let (ws_stream, _) = connect_async(request)
            .await
            .map_err(|e| {
                let err_msg = format!(
                    "Failed to connect to TTS WebSocket at {}: {}. Make sure moshi-server is running.",
                    endpoint, e
                );
                log::error!("{}", err_msg);
                VoiceError::Api(err_msg)
            })?;

        log::info!("Connected to TTS WebSocket");

        let (mut write, mut read) = ws_stream.split();

        // Spawn task to send text
        let text_to_send = text.to_string();
        let send_handle = tokio::spawn(async move {
            // Split text into words and send each as a Text message
            for word in text_to_send.split_whitespace() {
                let msg = TtsMessage::Text {
                    text: word.to_string(),
                };
                let mut buf = Vec::new();
                msg.serialize(&mut Serializer::new(&mut buf))
                    .map_err(|e| VoiceError::Api(format!("Failed to serialize message: {}", e)))?;

                write.send(Message::Binary(buf)).await
                    .map_err(|e| VoiceError::Api(format!("Failed to send text: {}", e)))?;
            }

            // Send EOS message
            let eos_msg = TtsMessage::Eos;
            let mut buf = Vec::new();
            eos_msg.serialize(&mut Serializer::new(&mut buf))
                .map_err(|e| VoiceError::Api(format!("Failed to serialize EOS: {}", e)))?;

            write.send(Message::Binary(buf)).await
                .map_err(|e| VoiceError::Api(format!("Failed to send EOS: {}", e)))?;

            Ok::<(), VoiceError>(())
        });

        // Set up streaming audio playback
        const SAMPLE_RATE: u32 = 24000; // Kyutai TTS uses 24kHz
        let volume = config.volume.unwrap_or(1.0);
        
        // Create a channel for streaming audio chunks
        let (audio_tx, mut audio_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<f32>>();
        let playback_finished = Arc::new(tokio::sync::Notify::new());
        let playback_finished_clone = Arc::clone(&playback_finished);

        // Use a ring buffer for streaming audio
        // Use std::sync::Mutex because the audio callback is blocking
        use std::collections::VecDeque;
        use std::sync::Mutex as StdMutex;
        let audio_queue = Arc::new(StdMutex::new(VecDeque::<f32>::new()));
        let audio_queue_for_callback = Arc::clone(&audio_queue);
        let audio_queue_for_feeder = Arc::clone(&audio_queue);
        let finished_flag = Arc::new(StdMutex::new(false));

        // Start a blocking thread to manage the audio stream
        // The stream must stay in the same thread
        let finished_flag_for_thread = Arc::clone(&finished_flag);
        std::thread::spawn(move || {
            let host = cpal::default_host();
            let device = match host.default_output_device() {
                Some(d) => d,
                None => {
                    log::error!("No default output device available");
                    return;
                }
            };

            // Create stream config
            let config = StreamConfig {
                channels: 1,
                sample_rate: cpal::SampleRate(SAMPLE_RATE),
                buffer_size: cpal::BufferSize::Default,
            };

            // Build output stream
            let stream = match device.build_output_stream(
                &config,
                {
                    let audio_queue = Arc::clone(&audio_queue_for_callback);
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        let mut queue = audio_queue.lock().unwrap();
                        for sample in data.iter_mut() {
                            if let Some(s) = queue.pop_front() {
                                *sample = (s * volume).clamp(-1.0, 1.0);
                            } else {
                                *sample = 0.0; // Silence if no data available
                            }
                        }
                    }
                },
                |err| {
                    log::error!("Audio stream error: {}", err);
                },
                None,
            ) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Failed to build output stream: {}", e);
                    return;
                }
            };

            // Play the stream
            if let Err(e) = stream.play() {
                log::error!("Failed to play audio stream: {}", e);
                return;
            }

            log::info!("Streaming audio playback started");

            // Keep the stream alive until finished flag is set and queue is empty
            loop {
                let is_finished = *finished_flag_for_thread.lock().unwrap();
                let queue_len = {
                    let queue = audio_queue_for_callback.lock().unwrap();
                    queue.len()
                };
                
                if is_finished && queue_len == 0 {
                    // Wait a bit more to ensure last samples play
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    break;
                }
                
                std::thread::sleep(std::time::Duration::from_millis(50));
            }

            // Stop the stream
            let _ = stream.pause();
            log::info!("Streaming audio playback completed");
        });

        // Spawn task to feed audio chunks from async context
        let playback_handle = tokio::spawn(async move {
            while let Some(chunk) = audio_rx.recv().await {
                let chunk_len = chunk.len();
                let mut queue = audio_queue_for_feeder.lock().unwrap();
                queue.extend(chunk);
                log::debug!("Added {} samples to playback queue (queue size: {})", 
                    chunk_len, queue.len());
            }
            // Signal that we're done receiving
            *finished_flag.lock().unwrap() = true;
            playback_finished_clone.notify_one();
        });

        // Receive audio chunks and stream them to playback
        let mut total_samples = 0;
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Binary(data)) => {
                    let mut de = Deserializer::new(&data[..]);
                    match TtsMessage::deserialize(&mut de) {
                        Ok(TtsMessage::Audio { pcm }) => {
                            total_samples += pcm.len();
                            log::debug!("Received {} PCM samples (total: {})", pcm.len(), total_samples);
                            // Send chunk to playback immediately
                            if let Err(e) = audio_tx.send(pcm) {
                                log::error!("Failed to send audio chunk to playback: {}", e);
                                break;
                            }
                        }
                        Ok(TtsMessage::Ready) => {
                            log::debug!("Server sent Ready message - connection established");
                        }
                        Ok(_) => {
                            // Other message types (Text, Eos) - ignore on receive
                        }
                        Err(e) => {
                            log::warn!("Failed to deserialize message: {}", e);
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    log::info!("WebSocket closed by server");
                    break;
                }
                Ok(_) => {
                    // Other message types - ignore
                }
                Err(e) => {
                    log::error!("WebSocket error: {}", e);
                    // Close the audio channel to signal end
                    drop(audio_tx);
                    return Err(VoiceError::Api(format!("WebSocket error: {}", e)));
                }
            }
        }

        // Wait for send task to complete
        if let Err(e) = send_handle.await {
            log::error!("Send task error: {:?}", e);
        }

        // Close the audio channel to signal end of streaming
        drop(audio_tx);

        if total_samples == 0 {
            return Err(VoiceError::Api("No audio data received from TTS server".to_string()));
        }

        log::info!("Received {} PCM samples ({} seconds) - streaming complete", 
            total_samples, 
            total_samples as f32 / SAMPLE_RATE as f32);

        // Wait for playback to finish
        playback_finished.notified().await;
        let _ = playback_handle.await;

        // Return empty buffer since we streamed directly
        Ok(Vec::new())
    }

    /// Fix malformed WAV file by correcting data chunk size
    fn fix_wav_file(&self, audio_data: &[u8]) -> Result<Vec<u8>> {
        // Try to parse and fix the WAV file
        // Look for "data" chunk and fix its size if needed
        if audio_data.len() < 12 {
            return Ok(audio_data.to_vec());
        }

        // Check RIFF header
        if &audio_data[0..4] != b"RIFF" {
            return Ok(audio_data.to_vec());
        }

        let mut fixed = audio_data.to_vec();
        
        // Find the "data" chunk (usually after "fmt " chunk)
        // Search for "data" chunk marker
        let mut data_pos = None;
        for i in 0..fixed.len().saturating_sub(8) {
            if &fixed[i..i+4] == b"data" {
                data_pos = Some(i);
                break;
            }
        }

        if let Some(pos) = data_pos {
            // Read the data chunk size (4 bytes after "data")
            if pos + 8 <= fixed.len() {
                let chunk_size = u32::from_le_bytes([
                    fixed[pos + 4],
                    fixed[pos + 5],
                    fixed[pos + 6],
                    fixed[pos + 7],
                ]) as usize;

                // Calculate actual data size (from data start to end of file, minus 8 for "data" + size)
                let actual_data_size = fixed.len().saturating_sub(pos + 8);
                
                // If chunk size doesn't match, fix it
                if chunk_size != actual_data_size {
                    log::warn!("Fixing WAV data chunk size: {} -> {}", chunk_size, actual_data_size);
                    let new_size = actual_data_size as u32;
                    fixed[pos + 4..pos + 8].copy_from_slice(&new_size.to_le_bytes());
                }
            }
        }

        Ok(fixed)
    }

    /// Play audio data using rodio
    async fn play_audio(&self, audio_data: Vec<u8>, volume: f32) -> Result<()> {
        log::info!("Playing audio: {} bytes, volume: {}", audio_data.len(), volume);

        // Stop any currently playing audio
        self.stop().await?;

        // Create output stream
        let (_stream, stream_handle) = OutputStream::try_default()
            .map_err(|e| {
                let err_msg = format!("Failed to create audio output stream: {}", e);
                log::error!("{}", err_msg);
                VoiceError::Audio(err_msg)
            })?;

        log::debug!("Audio output stream created");

        // Decode audio - check if it's WAV format first (server returns audio/wav)
        use rodio::Source;
        
        let source: Box<dyn Source<Item = f32> + Send> = {
            // Check if it's a WAV file (starts with "RIFF" or content-type is audio/wav)
            if audio_data.len() >= 4 && &audio_data[0..4] == b"RIFF" {
                log::info!("Detected WAV format, decoding with hound");
                
                // Try to fix WAV file if it's malformed
                let fixed_audio = self.fix_wav_file(&audio_data)?;
                
                // Use hound to decode WAV directly
                let mut reader = hound::WavReader::new(Cursor::new(fixed_audio))
                    .map_err(|e| {
                        let err_msg = format!("Failed to decode WAV with hound: {}", e);
                        log::error!("{}", err_msg);
                        VoiceError::Audio(err_msg)
                    })?;
                
                let spec = reader.spec();
                log::info!("WAV spec: {} Hz, {} channels, {} bits", 
                    spec.sample_rate, spec.channels, spec.bits_per_sample);
                
                // Read all samples based on bit depth
                let samples: Vec<f32> = match spec.bits_per_sample {
                    16 => {
                        reader.samples::<i16>()
                            .map(|s| {
                                s.map(|sample| sample as f32 / 32768.0)
                                    .map_err(|e| VoiceError::Audio(format!("Failed to read sample: {}", e)))
                            })
                            .collect::<std::result::Result<Vec<_>, _>>()?
                    }
                    24 => {
                        // 24-bit samples - read as i32 and shift
                        reader.samples::<i32>()
                            .map(|s| {
                                s.map(|sample| (sample >> 8) as f32 / 8388608.0)
                                    .map_err(|e| VoiceError::Audio(format!("Failed to read sample: {}", e)))
                            })
                            .collect::<std::result::Result<Vec<_>, _>>()?
                    }
                    32 => {
                        reader.samples::<i32>()
                            .map(|s| {
                                s.map(|sample| sample as f32 / 2147483648.0)
                                    .map_err(|e| VoiceError::Audio(format!("Failed to read sample: {}", e)))
                            })
                            .collect::<std::result::Result<Vec<_>, _>>()?
                    }
                    _ => {
                        return Err(VoiceError::Audio(format!(
                            "Unsupported bit depth: {} bits",
                            spec.bits_per_sample
                        )));
                    }
                };
                
                log::info!("Decoded {} samples from WAV", samples.len());
                
                // Play audio using cpal directly (more reliable for raw PCM)
                return self.play_pcm_samples(samples, spec.sample_rate, spec.channels, volume).await;
            } else {
                // Try rodio's auto-detection for other formats
                log::info!("Trying rodio auto-detection for audio format");
                let cursor = Cursor::new(audio_data);
                let decoder = Decoder::new(cursor)
                    .map_err(|e| {
                        let err_msg = format!("Failed to decode audio: {}. Audio format may not be supported.", e);
                        log::error!("{}", err_msg);
                        VoiceError::Audio(err_msg)
                    })?;
                Box::new(decoder.convert_samples::<f32>())
            }
        };

        log::debug!("Audio decoded successfully");

        // Apply volume
        let source = source.amplify(volume.max(0.0).min(1.0));

        // Create sink and play
        let sink = Sink::try_new(&stream_handle)
            .map_err(|e| {
                let err_msg = format!("Failed to create audio sink: {}", e);
                log::error!("{}", err_msg);
                VoiceError::Audio(err_msg)
            })?;

        sink.append(source);
        sink.play();

        log::info!("Audio playback started");

        // Store sink for later control
        *self.current_sink.lock().await = Some(sink);

        Ok(())
    }

    /// Play PCM samples directly using cpal
    async fn play_pcm_samples(
        &self,
        samples: Vec<f32>,
        sample_rate: u32,
        channels: u16,
        volume: f32,
    ) -> Result<()> {
        log::info!("Playing {} PCM samples at {} Hz, {} channels", 
            samples.len(), sample_rate, channels);

        // Stop any currently playing audio
        self.stop().await?;

        // Get default output device
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| VoiceError::Audio("No default output device available".to_string()))?;

        // Create stream config
        let config = StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        // Apply volume to samples
        let samples: Vec<f32> = samples
            .into_iter()
            .map(|s| (s * volume).clamp(-1.0, 1.0))
            .collect();

        // Create a shared buffer for the samples
        let samples_arc = Arc::new(samples);
        let samples_clone = Arc::clone(&samples_arc);
        let sample_index = Arc::new(Mutex::new(0usize));

        // Build output stream
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut idx = sample_index.blocking_lock();
                    for sample in data.iter_mut() {
                        if *idx < samples_clone.len() {
                            *sample = samples_clone[*idx];
                            *idx += 1;
                        } else {
                            *sample = 0.0; // Silence after samples end
                        }
                    }
                },
                |err| {
                    log::error!("Audio stream error: {}", err);
                },
                None,
            )
            .map_err(|e| VoiceError::Audio(format!("Failed to build output stream: {}", e)))?;

        // Play the stream
        stream.play().map_err(|e| {
            VoiceError::Audio(format!("Failed to play audio stream: {}", e))
        })?;

        log::info!("Audio playback started with cpal");

        // Wait for playback to complete
        let duration_ms = (samples_arc.len() as f32 / sample_rate as f32 * 1000.0) as u64;
        tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms + 100)).await;

        // Stop the stream
        stream.pause().map_err(|e| {
            VoiceError::Audio(format!("Failed to pause audio stream: {}", e))
        })?;

        log::info!("Audio playback completed");

        Ok(())
    }

    /// Speak the given text
    pub async fn speak(&mut self, text: &str, interrupt: bool, config: &TtsConfig) -> Result<()> {
        if interrupt {
            self.stop().await?;
        }

        let audio_data = self.synthesize(text, config).await?;
        let volume = config.volume.unwrap_or(1.0);
        self.play_audio(audio_data, volume).await?;
        Ok(())
    }

    /// Speak the given text and wait for completion
    pub async fn speak_sync(&mut self, text: &str, interrupt: bool, config: &TtsConfig) -> Result<()> {
        self.speak(text, interrupt, config).await?;
        self.wait().await?;
        Ok(())
    }

    /// Wait for current speech to complete
    pub async fn wait(&self) -> Result<()> {
        let sink_guard = self.current_sink.lock().await;
        if let Some(ref sink) = *sink_guard {
            sink.sleep_until_end();
        }
        Ok(())
    }

    /// Check if currently speaking
    pub async fn is_speaking(&self) -> Result<bool> {
        let sink_guard = self.current_sink.lock().await;
        Ok(sink_guard
            .as_ref()
            .map(|sink| !sink.empty())
            .unwrap_or(false))
    }

    /// Stop current speech
    pub async fn stop(&self) -> Result<()> {
        let mut sink_guard = self.current_sink.lock().await;
        if let Some(sink) = sink_guard.take() {
            sink.stop();
        }
        Ok(())
    }

    /// Get list of available voices
    pub async fn list_voices(&self) -> Result<Vec<VoiceInfo>> {
        if self.local_mode {
            // For local mode, return empty list (voices would need to be queried from Python)
            Ok(vec![])
        } else {
            // HTTP mode: try to get voices from server
            let endpoint = self.endpoint.as_ref().ok_or_else(|| {
                VoiceError::Configuration("HTTP endpoint not configured".to_string())
            })?;

            let http_client = self.http_client.as_ref().ok_or_else(|| {
                VoiceError::Configuration("HTTP client not initialized".to_string())
            })?;

            let voices_endpoint = endpoint.replace("/tts", "/voices");
            
            let response = http_client
                .get(&voices_endpoint)
                .send()
                .await
                .map_err(|e| {
                    VoiceError::Api(format!(
                        "Failed to get voices from Kyutai TTS server: {}. The server may not support the /voices endpoint.",
                        e
                    ))
                })?;

            if !response.status().is_success() {
                return Ok(vec![]);
            }

            let voices: Vec<serde_json::Value> = response
                .json()
                .await
                .map_err(|e| VoiceError::Api(format!("Failed to parse voices response: {}", e)))?;

            Ok(voices
                .into_iter()
                .map(|v| VoiceInfo {
                    name: v["name"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string(),
                    language: v["language"]
                        .as_str()
                        .unwrap_or("en")
                        .to_string(),
                    gender: v["gender"].as_str().map(|s| s.to_string()),
                })
                .collect())
        }
    }

    /// Get the current voice (not applicable for Kyutai - returns None)
    pub fn current_voice(&self) -> Result<Option<VoiceInfo>> {
        Ok(None)
    }

    /// Get current rate (returns configured rate)
    pub fn rate(&self) -> Result<f32> {
        Ok(0.5) // Default, actual rate is in config
    }

    /// Get current volume (returns configured volume)
    pub fn volume(&self) -> Result<f32> {
        Ok(1.0) // Default, actual volume is in config
    }
}

/// Information about a TTS voice
#[derive(Debug, Clone)]
pub struct VoiceInfo {
    pub name: String,
    pub language: String,
    pub gender: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tts_config_default() {
        let config = TtsConfig::default();
        assert!(config.endpoint.is_none());
        assert!(config.voice.is_none());
        assert_eq!(config.rate, Some(0.5));
        assert_eq!(config.volume, Some(1.0));
        assert!(config.language.is_none());
    }
}
