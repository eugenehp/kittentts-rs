//! OpenAI-compatible TTS web service for KittenTTS.
//!
//! Provides a `/v1/audio/speech` endpoint compatible with OpenAI's API format.
//!
//! ## Usage
//!
//! ```bash
//! # Using HuggingFace model (auto-download)
//! cargo run --bin kittentts-server -- --model KittenML/kitten-tts-mini-0.8
//!
//! # Using local model files
//! cargo run --bin kittentts-server -- \
//!     --model /path/to/model.onnx \
//!     --voices /path/to/voices.npz
//!
//! # With custom port
//! cargo run --bin kittentts-server -- --port 8080
//! ```

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{sse::{Event, KeepAlive, Sse}, IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use base64::Engine;

use kittentts::{download, encoding, model::KittenTtsOnnx, streaming, SAMPLE_RATE};

// ─────────────────────────────────────────────────────────────────────────────
// OpenAI API Types
// ─────────────────────────────────────────────────────────────────────────────

/// OpenAI-compatible `/v1/audio/speech` request.
#[derive(Debug, Deserialize)]
struct SpeechRequest {
    /// Model identifier (ignored for now, can be used for versioning)
    #[allow(dead_code)]
    model: String,

    /// Input text to synthesize
    input: String,

    /// Voice name (OpenAI voices mapped to KittenTTS voices)
    voice: String,

    /// Audio format (only "wav" for non-streaming, "pcm" for streaming)
    #[serde(default)]
    response_format: String,

    /// Speech speed (0.25 to 4.0, OpenAI default is 1.0)
    #[serde(default = "default_speed")]
    speed: f32,

    /// Enable SSE streaming. When true, response_format must be "pcm".
    #[serde(default)]
    stream: Option<bool>,
}

fn default_speed() -> f32 {
    1.0
}

/// OpenAI-compatible error response.
#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    param: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Voice Mapping
// ─────────────────────────────────────────────────────────────────────────────

/// Map OpenAI voice names to KittenTTS voice names.
fn map_openai_voice(openai_voice: &str) -> String {
    match openai_voice.to_lowercase().as_str() {
        // OpenAI's standard voices → KittenTTS expr-voice voices
        "alloy"   => "expr-voice-2-f".to_string(),
        "echo"    => "expr-voice-4-m".to_string(),
        "fable"   => "expr-voice-3-f".to_string(),
        "onyx"    => "expr-voice-3-m".to_string(),
        "nova"    => "expr-voice-5-f".to_string(),
        "shimmer" => "expr-voice-4-f".to_string(),
        "kiki"    => "expr-voice-5-m".to_string(),
        "leo"     => "expr-voice-2-m".to_string(),

        // Pass through KittenTTS voice names directly (e.g. expr-voice-2-f)
        _ => openai_voice.to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Application State
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    tts: Arc<KittenTtsOnnx>,
    default_format: String,
    default_stream: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Error Handling
// ─────────────────────────────────────────────────────────────────────────────

enum ApiError {
    BadRequest(String),
    InternalError(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::InternalError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        let body = Json(ErrorResponse {
            error: ErrorDetail {
                message,
                r#type: None,
                param: None,
                code: None,
            },
        });

        (status, body).into_response()
    }
}

impl<E: std::error::Error> From<E> for ApiError {
    fn from(err: E) -> Self {
        ApiError::InternalError(err.to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// OpenAI-compatible `/v1/audio/speech` endpoint.
async fn speech_handler(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Json(mut req): Json<SpeechRequest>,
) -> Result<Response, ApiError> {
    // ── Apply server defaults for missing fields ────────────────────────────────
    if req.response_format.is_empty() {
        req.response_format = state.default_format.clone();
    }
    let stream = req.stream.unwrap_or(state.default_stream);

    // ── Validate request ───────────────────────────────────────────────────────
    if req.input.trim().is_empty() {
        return Err(ApiError::BadRequest("input text cannot be empty".into()));
    }

    if !(0.25..=4.0).contains(&req.speed) {
        return Err(ApiError::BadRequest(
            "speed must be between 0.25 and 4.0".into(),
        ));
    }

    // Map OpenAI voice name to KittenTTS voice name
    let mapped_voice = map_openai_voice(&req.voice);

    // Check if voice is available
    if !state.tts.available_voices.contains(&mapped_voice.to_string()) {
        return Err(ApiError::BadRequest(format!(
            "voice '{}' (mapped from '{}') not available. Available voices: {:?}",
            mapped_voice, req.voice, state.tts.available_voices
        )));
    }

    // ── Log request ───────────────────────────────────────────────────────────
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let text_preview: String = req.input.chars().take(50).collect();
    let ellipsis = if req.input.chars().count() > 50 { "..." } else { "" };
    println!("[{}] voice={}, format={}, stream={}, speed={}, text=\"{}{}\"",
        now,
        req.voice, req.response_format, stream, req.speed,
        text_preview, ellipsis
    );

    // ── Route to streaming or non-streaming handler ──────────────────────────
    if stream {
        speech_stream_handler(State(state), req, mapped_voice).await
    } else {
        speech_full_handler(State(state), req, mapped_voice).await
    }
}

/// Non-streaming TTS handler with multi-format support.
async fn speech_full_handler(
    state: State<AppState>,
    req: SpeechRequest,
    mapped_voice: String,
) -> Result<Response<Body>, ApiError> {
    // Parse format and create encoder
    let format = kittentts::AudioFormat::from_string(&req.response_format)
        .map_err(|e| ApiError::BadRequest(format!("Invalid format: {}", e)))?;

    // ── Generate audio ─────────────────────────────────────────────────────────
    let audio = state
        .tts
        .generate(&req.input, &mapped_voice, req.speed, true)
        .map_err(|e| ApiError::InternalError(format!("TTS generation failed: {}", e)))?;

    // ── Encode to target format ───────────────────────────────────────────────────
    let encoder = kittentts::EncoderFactory::create(format);
    let audio_data = encoder.encode(&audio)
        .map_err(|e| ApiError::InternalError(format!("Audio encoding failed: {}", e)))?;

    // ── Return response ──────────────────────────────────────────────────────────
    let mut response = Response::new(Body::from(audio_data));

    // Set headers
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_str(encoder.content_type())
            .unwrap(),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"speech.{}\"", encoder.extension()))
            .unwrap(),
    );

    // Set cache control (prevent caching of generated audio)
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );

    Ok(response)
}

/// Streaming TTS handler using Server-Sent Events (SSE).
async fn speech_stream_handler(
    state: State<AppState>,
    req: SpeechRequest,
    mapped_voice: String,
) -> Result<Response, ApiError> {
    // Validate format for streaming
    if req.response_format != "pcm" {
        return Err(ApiError::BadRequest(
            "streaming only supports response_format 'pcm'".into(),
        ));
    }

    // Create SSE channel
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(8);

    // Clone data for the blocking task
    let input = req.input.clone();
    let voice = mapped_voice.clone();
    let speed = req.speed;
    let tts = state.tts.clone();

    // Spawn blocking task for CPU-intensive audio generation
    tokio::task::spawn_blocking(move || {
        // Step 1: Intelligent text chunking for streaming
        let chunks = streaming::chunk_text_streaming(&input, 100, 400);

        println!("Starting TTS streaming: num_chunks={}, input_len={}", chunks.len(), input.len());

        // Base64 encoder for PCM data
        let b64 = base64::engine::general_purpose::STANDARD;

        // Step 2: Generate and stream audio for each chunk
        for (i, chunk) in chunks.iter().enumerate() {
            // Generate audio for this text chunk
            let audio = match tts.generate_chunk(chunk, &voice, speed) {
                Ok(audio) => audio,
                Err(e) => {
                    // Send error event and terminate
                    let err_msg = serde_json::json!({
                        "type": "error",
                        "error": { "message": format!("Chunk {} generation failed: {}", i, e) }
                    });
                    let _ = tx.blocking_send(Ok(Event::default().data(err_msg.to_string())));
                    return;
                }
            };

            // Encode audio as 16-bit PCM
            let pcm = encoding::encode_pcm(&audio);

            // Base64 encode for JSON transmission
            let delta = b64.encode(&pcm);

            println!("Generated streaming chunk: chunk_index={}, text_len={}, audio_samples={}, pcm_bytes={}",
                i, chunk.len(), audio.len(), pcm.len());

            // Create SSE event with audio data
            let data = serde_json::json!({
                "type": "speech.audio.delta",
                "delta": delta,
            });

            let event = Event::default().data(data.to_string());

            // Send event (check for client disconnect)
            if tx.blocking_send(Ok(event)).is_err() {
                println!("Client disconnected during streaming at chunk {}", i);
                return;
            }
        }

        // Step 3: Send completion event
        let done = serde_json::json!({ "type": "speech.audio.done" });
        if let Err(_) = tx.blocking_send(Ok(Event::default().data(done.to_string()))) {
            println!("Failed to send completion event");
        }

        println!("TTS streaming completed successfully");
    });

    // Return SSE response with keep-alive
    Ok(Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::default())
        .into_response())
}

/// Health check endpoint.
async fn health_handler() -> &'static str {
    "OK"
}

/// List available voices.
async fn voices_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let openai_voices = vec![
        "alloy", "echo", "fable", "onyx", "nova", "shimmer", "kiki", "leo",
    ];

    let available: Vec<serde_json::Value> = state
        .tts
        .available_voices
        .iter()
        .map(|v| {
            serde_json::json!({
                "name": v,
                "supported": true,
            })
        })
        .collect();

    // Map OpenAI voice names to available expr-voice-* names
    let voice_mapping: std::collections::HashMap<&str, &str> = [
        ("alloy", "expr-voice-2-f"),
        ("echo", "expr-voice-3-f"),
        ("fable", "expr-voice-4-m"),
        ("onyx", "expr-voice-5-m"),
        ("nova", "expr-voice-3-m"),
        ("shimmer", "expr-voice-4-f"),
        ("kiki", "expr-voice-2-m"),
        ("leo", "expr-voice-5-f"),
    ]
    .into_iter()
    .filter(|(_, available_voice): &(&str, &str)| state.tts.available_voices.contains(&available_voice.to_string()))
    .collect();

    Json(serde_json::json!({
        "openai_voices": openai_voices,
        "available_voices": available,
        "voice_mapping": voice_mapping
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// WAV Encoding
// ─────────────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
fn audio_to_wav(audio: &[f32]) -> Vec<u8> {
    let mut buffer = Vec::new();

    // RIFF header
    buffer.extend_from_slice(b"RIFF");

    // File size - 8 (will update later)
    let file_size_pos = buffer.len();
    buffer.extend_from_slice(&[0u8; 4]);

    // WAVE format
    buffer.extend_from_slice(b"WAVE");

    // fmt chunk
    buffer.extend_from_slice(b"fmt ");

    // Chunk size (16 for PCM)
    buffer.extend_from_slice(&16u32.to_le_bytes());

    // Audio format (1 = PCM)
    buffer.extend_from_slice(&1u16.to_le_bytes());

    // Number of channels (1 = mono)
    buffer.extend_from_slice(&1u16.to_le_bytes());

    // Sample rate
    buffer.extend_from_slice(&(SAMPLE_RATE).to_le_bytes());

    // Byte rate = SampleRate * NumChannels * BitsPerSample/8
    let byte_rate = SAMPLE_RATE * 1 * 16 / 8;
    buffer.extend_from_slice(&byte_rate.to_le_bytes());

    // Block align = NumChannels * BitsPerSample/8
    buffer.extend_from_slice(&2u16.to_le_bytes());

    // Bits per sample
    buffer.extend_from_slice(&16u16.to_le_bytes());

    // data chunk
    buffer.extend_from_slice(b"data");

    // Data size (will update later)
    let data_size_pos = buffer.len();
    buffer.extend_from_slice(&[0u8; 4]);

    // Audio data (convert f32 [-1.0, 1.0] to i16)
    let data_start = buffer.len();
    for &sample in audio {
        let clamped = sample.clamp(-1.0, 1.0);
        let i16_sample = (clamped * 32767.0) as i16;
        buffer.extend_from_slice(&i16_sample.to_le_bytes());
    }

    // Update sizes
    let data_size = (buffer.len() - data_start) as u32;
    let file_size = (buffer.len() - 8) as u32;

    buffer[data_size_pos..data_size_pos + 4].copy_from_slice(&data_size.to_le_bytes());
    buffer[file_size_pos..file_size_pos + 4].copy_from_slice(&file_size.to_le_bytes());

    buffer
}

// ─────────────────────────────────────────────────────────────────────────────
// Server Configuration
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ServerConfig {
    model_source: ModelSource,
    port: u16,
    host: String,
    default_format: String,
    default_stream: bool,
}

#[derive(Debug, Clone)]
enum ModelSource {
    HuggingFace(String),
    LocalPath { model: PathBuf, voices: PathBuf },
}

// ─────────────────────────────────────────────────────────────────────────────
// Main
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // ── Parse CLI arguments ─────────────────────────────────────────────────────
    let args = parse_args()?;

    // ── Load model ─────────────────────────────────────────────────────────────
    println!("🎤 KittenTTS Server - OpenAI-Compatible TTS API");
    println!();

    let tts = match &args.model_source {
        ModelSource::HuggingFace(repo_id) => {
            println!("📥 Loading model from HuggingFace: {}", repo_id);
            download::load_from_hub_cb(repo_id, |progress| {
                match progress {
                    download::LoadProgress::Fetching { step, total, file } => {
                        println!("  [{}/{}] Fetching: {}", step, total, file);
                    }
                    download::LoadProgress::Loading => {
                        println!("  [4/4] Loading ONNX session...");
                    }
                }
            })
            .context("Failed to load model from HuggingFace")?
        }
        ModelSource::LocalPath { model, voices } => {
            println!("📂 Loading model from local files:");
            println!("  Model: {}", model.display());
            println!("  Voices: {}", voices.display());

            KittenTtsOnnx::load(
                model,
                voices,
                HashMap::new(),
                {
                    let mut aliases = HashMap::new();
                    // Add OpenAI voice aliases
                    aliases.insert("alloy".to_string(), "expr-voice-2-f".to_string());
                    aliases.insert("echo".to_string(), "expr-voice-4-m".to_string());
                    aliases.insert("fable".to_string(), "expr-voice-3-f".to_string());
                    aliases.insert("onyx".to_string(), "expr-voice-3-m".to_string());
                    aliases.insert("nova".to_string(), "expr-voice-5-f".to_string());
                    aliases.insert("shimmer".to_string(), "expr-voice-4-f".to_string());
                    aliases.insert("kiki".to_string(), "expr-voice-5-m".to_string());
                    aliases.insert("leo".to_string(), "expr-voice-2-m".to_string());
                    aliases
                },
            )
            .context("Failed to load model from local files")?
        }
    };

    println!("✅ Model loaded successfully!");
    println!("   Available voices: {:?}", tts.available_voices);
    println!();

    // ── Build router ───────────────────────────────────────────────────────────
    let state = AppState {
        tts: Arc::new(tts),
        default_format: args.default_format.clone(),
        default_stream: args.default_stream,
    };

    let app = Router::new()
        .route("/v1/audio/speech", post(speech_handler))
        .route("/health", axum::routing::get(health_handler))
        .route("/v1/voices", axum::routing::get(voices_handler))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // ── Start server ──────────────────────────────────────────────────────────
    let addr = format!("{}:{}", args.host, args.port);
    let socket_addr: SocketAddr = addr
        .parse()
        .with_context(|| format!("Invalid address: {}", addr))?;

    println!("🚀 Server starting on http://{}", addr);
    println!("   Endpoints:");
    println!("     POST /v1/audio/speech - Generate speech from text");
    println!("     GET  /v1/voices       - List available voices");
    println!("     GET  /health          - Health check");
    println!();
    println!("📖 Example usage:");
    println!(
        "   curl -X POST http://{}:{}/v1/audio/speech -H 'Content-Type: application/json' \\",
        args.host, args.port
    );
    println!("        -d '{{\"model\":\"tts-1\",\"input\":\"Hello world\",\"voice\":\"alloy\"}}' \\");
    println!("        --output speech.wav");
    println!();

    let listener = TcpListener::bind(&socket_addr)
        .await
        .context("Failed to bind to address")?;

    axum::serve(listener, app)
        .await
        .context("Server error")?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// CLI Argument Parsing
// ─────────────────────────────────────────────────────────────────────────────

fn parse_args() -> Result<ServerConfig> {
    let mut model = None;
    let mut voices = None;
    let mut port = 3000u16;
    let mut host = "127.0.0.1".to_string();
    let mut default_format = "wav".to_string();
    let mut default_stream = false;

    let mut args = std::env::args().skip(1).peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--model" | "-m" => {
                if let Some(value) = args.next() {
                    model = Some(value);
                }
            }
            "--voices" | "-v" => {
                if let Some(value) = args.next() {
                    voices = Some(PathBuf::from(value));
                }
            }
            "--port" | "-p" => {
                if let Some(value) = args.next() {
                    port = value.parse().context("Invalid port number")?;
                }
            }
            "--host" | "-h" => {
                if let Some(value) = args.next() {
                    host = value;
                }
            }
            "--default-format" => {
                if let Some(value) = args.next() {
                    default_format = value;
                }
            }
            "--default-stream" => {
                default_stream = true;
            }
            "--no-default-stream" => {
                default_stream = false;
            }
            "--help" => {
                print_usage();
                std::process::exit(0);
            }
            _ if arg.starts_with('-') => {
                anyhow::bail!("Unknown option: {}", arg);
            }
            _ => {
                // Positional argument: model
                if model.is_none() {
                    model = Some(arg);
                } else {
                    anyhow::bail!("Unexpected argument: {}", arg);
                }
            }
        }
    }

    // Determine model source
    let model_source = if let Some(voices_path) = voices {
        // Local mode: both --model and --voices specified
        let model_path = model.ok_or_else(|| {
            anyhow::anyhow!("--model required when --voices is specified")
        })?;

        ModelSource::LocalPath {
            model: PathBuf::from(model_path),
            voices: voices_path,
        }
    } else {
        // HuggingFace mode (or implied local if model is a file path)
        let model_id = model.unwrap_or_else(|| "KittenML/kitten-tts-mini-0.8".to_string());

        if Path::new(&model_id).exists() {
            // It's a file path - infer voices path
            anyhow::bail!(
                "Model path detected but --voices not specified. \
                 Please provide both --model and --voices for local model loading."
            );
        } else {
            ModelSource::HuggingFace(model_id)
        }
    };

    Ok(ServerConfig {
        model_source,
        port,
        host,
        default_format,
        default_stream,
    })
}

fn print_usage() {
    println!(
        r#"
KittenTTS Server - OpenAI-Compatible TTS Web Service

USAGE:
    kittentts-server [OPTIONS] [MODEL]

OPTIONS:
    -m, --model <MODEL>      Model identifier or path
                             - HuggingFace repo (e.g., KittenML/kitten-tts-mini-0.8)
                             - Local .onnx file path (requires --voices)
    -v, --voices <PATH>      Voices .npz file path (required for local models)
    -p, --port <PORT>            Server port [default: 3000]
    -h, --host <HOST>            Server host [default: 127.0.0.1]
    --default-format <FORMAT>    Default response_format for /v1/audio/speech [default: wav]
    --default-stream             Default streaming mode for /v1/audio/speech [default: false]
    --no-default-stream          Explicitly disable default streaming
    --help                       Print this help message

ARGUMENTS:
    [MODEL]                  Shorthand for --model

EXAMPLES:
    # Using HuggingFace model (default: KittenML/kitten-tts-mini-0.8)
    kittentts-server

    # Specify a different HuggingFace model
    kittentts-server --model KittenML/kitten-tts-micro-0.8

    # Using local model files
    kittentts-server --model /path/to/model.onnx --voices /path/to/voices.npz

    # Custom host and port
    kittentts-server --port 8080 --host 0.0.0.0

OPENAI API COMPATIBILITY:
    The server implements the /v1/audio/speech endpoint with voice mapping:
    - alloy  → expr-voice-2-f
    - echo   → expr-voice-4-m
    - fable  → expr-voice-3-f
    - onyx   → expr-voice-3-m
    - nova   → expr-voice-5-f
    - shimmer→ expr-voice-4-f
    - kiki   → expr-voice-5-m
    - leo    → expr-voice-2-m
"#
    );
}
