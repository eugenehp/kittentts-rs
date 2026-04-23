//! OpenAI-compatible TTS server.
//!
//! Implements the `POST /v1/audio/speech` endpoint with voice mapping,
//! multi-format audio output, and request validation.
//!
//! # Usage
//!
//! ```bash
//! cargo run --bin kittentts-server --features server
//! cargo run --bin kittentts-server --features server -- --port 9090 --model KittenML/kitten-tts-nano-0.8-int8
//! ```
//!
//! # Example request
//!
//! ```bash
//! curl -X POST http://localhost:8080/v1/audio/speech \
//!   -H "Content-Type: application/json" \
//!   -d '{"model":"tts-1","input":"Hello!","voice":"alloy"}' \
//!   --output output.mp3
//! ```

use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

use kittentts::{download, AudioFormat, EncoderFactory, KittenTTS, SAMPLE_RATE};

// ─── CLI ────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "kittentts-server")]
#[command(about = "OpenAI-compatible TTS server powered by KittenTTS")]
struct Args {
    /// Host to bind to
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    /// Port to listen on
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// HuggingFace model repository ID
    #[arg(long, default_value = "KittenML/kitten-tts-mini-0.8")]
    model: String,

    /// Default audio output format (mp3, wav, opus, flac, pcm)
    #[arg(long, default_value = "mp3")]
    default_format: String,
}

// ─── Shared state ───────────────────────────────────────────────────────────

struct AppState {
    tts: KittenTTS,
    model_id: String,
    default_format: String,
}

// ─── OpenAI API types ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SpeechRequest {
    /// Model identifier (accepted for compatibility, not used for selection).
    #[allow(dead_code)]
    model: String,

    /// Text to synthesise. Maximum 4096 characters.
    input: String,

    /// Voice name. Accepts OpenAI names (alloy, echo, ...) or KittenTTS
    /// names (Bella, Jasper, ...).
    voice: String,

    /// Output format: mp3, opus, flac, wav, pcm.
    response_format: Option<String>,

    /// Speed multiplier (0.25 to 4.0). Default 1.0.
    speed: Option<f32>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
    code: Option<String>,
}

#[derive(Serialize)]
struct ModelsResponse {
    object: &'static str,
    data: Vec<ModelObject>,
}

#[derive(Serialize)]
struct ModelObject {
    id: String,
    object: &'static str,
    owned_by: &'static str,
}

// ─── Voice mapping ──────────────────────────────────────────────────────────

/// Map OpenAI voice names to KittenTTS voice names.
/// Returns the KittenTTS name, or `None` if the name is not a known
/// OpenAI alias (caller should try it as a direct KittenTTS name).
fn openai_voice_to_kittentts(name: &str) -> Option<&'static str> {
    match name {
        "alloy" => Some("Luna"),
        "echo" => Some("Hugo"),
        "fable" => Some("Kiki"),
        "onyx" => Some("Bruno"),
        "nova" => Some("Bella"),
        "shimmer" => Some("Rosie"),
        "ash" => Some("Jasper"),
        "sage" => Some("Leo"),
        "coral" => Some("Rosie"),
        _ => None,
    }
}

/// Resolve a voice name: try OpenAI mapping first, then check if it's a valid
/// KittenTTS voice name directly.
fn resolve_voice(name: &str, available: &[String]) -> Option<String> {
    // Try OpenAI mapping
    if let Some(mapped) = openai_voice_to_kittentts(name) {
        if available.iter().any(|v| v == mapped) {
            return Some(mapped.to_string());
        }
    }

    // Try as direct KittenTTS name (case-insensitive match)
    for v in available {
        if v.eq_ignore_ascii_case(name) {
            return Some(v.clone());
        }
    }

    None
}

// ─── Error helpers ──────────────────────────────────────────────────────────

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: ErrorDetail {
                message: msg.into(),
                error_type: "invalid_request_error".to_string(),
                code: None,
            },
        }),
    )
}

fn server_error(msg: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: ErrorDetail {
                message: msg.into(),
                error_type: "server_error".to_string(),
                code: None,
            },
        }),
    )
}

// ─── Handlers ───────────────────────────────────────────────────────────────

async fn speech_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SpeechRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    // Validate input
    if req.input.is_empty() {
        return Err(bad_request("Input text must not be empty."));
    }
    if req.input.len() > 4096 {
        return Err(bad_request("Input text must be at most 4096 characters."));
    }

    // Validate speed
    let speed = req.speed.unwrap_or(1.0);
    if !(0.25..=4.0).contains(&speed) {
        return Err(bad_request("Speed must be between 0.25 and 4.0."));
    }

    // Resolve voice
    let voice = resolve_voice(&req.voice, &state.tts.available_voices)
        .ok_or_else(|| {
            bad_request(format!(
                "Unknown voice '{}'. Available voices: {} (or OpenAI names: alloy, echo, fable, onyx, nova, shimmer, ash, sage, coral).",
                req.voice,
                state.tts.available_voices.join(", ")
            ))
        })?;

    // Parse format
    let format_str = req
        .response_format
        .as_deref()
        .unwrap_or(&state.default_format);
    let format = AudioFormat::from_str_openai(format_str)
        .ok_or_else(|| bad_request(format!("Unsupported format '{format_str}'. Supported: mp3, wav, opus, flac, pcm.")))?;

    // Create encoder (validates feature availability)
    let encoder = EncoderFactory::create(format).map_err(|e| bad_request(e.to_string()))?;

    // Log request (truncate input for readability)
    let display_input: String = req.input.chars().take(80).collect();
    let truncated = if req.input.chars().count() > 80 {
        "..."
    } else {
        ""
    };
    eprintln!(
        "POST /v1/audio/speech voice={voice} format={format_str} speed={speed} input=\"{display_input}{truncated}\""
    );

    // Run inference on a blocking thread (CPU-bound ONNX work).
    // Clone the Arc so the blocking task owns a reference to AppState.
    let state_clone = Arc::clone(&state);
    let input = req.input.clone();
    let voice_clone = voice.clone();

    let audio = tokio::task::spawn_blocking(move || {
        state_clone.tts.generate(&input, &voice_clone, speed, true)
    })
    .await
    .map_err(|e| server_error(format!("TTS task panicked: {e}")))?
    .map_err(|e| server_error(format!("TTS generation failed: {e}")))?;

    // Encode
    let bytes = encoder
        .encode(&audio, SAMPLE_RATE)
        .map_err(|e| server_error(format!("Encoding failed: {e}")))?;

    // Build response with correct content-type
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        encoder.content_type().parse().unwrap(),
    );

    Ok((headers, bytes))
}

async fn list_models(State(state): State<Arc<AppState>>) -> Json<ModelsResponse> {
    Json(ModelsResponse {
        object: "list",
        data: vec![ModelObject {
            id: state.model_id.clone(),
            object: "model",
            owned_by: "kittentts",
        }],
    })
}

async fn list_voices(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "voices": state.tts.available_voices,
        "openai_mapping": {
            "alloy": "Luna",
            "echo": "Hugo",
            "fable": "Kiki",
            "onyx": "Bruno",
            "nova": "Bella",
            "shimmer": "Rosie",
            "ash": "Jasper",
            "sage": "Leo",
            "coral": "Rosie",
        }
    }))
}

async fn health() -> &'static str {
    "ok"
}

// ─── Main ───────────────────────────────────────────────────────────────────

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
    eprintln!("\nShutting down...");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Validate default format
    if AudioFormat::from_str_openai(&args.default_format).is_none() {
        anyhow::bail!(
            "Invalid default format '{}'. Supported: mp3, wav, opus, flac, pcm.",
            args.default_format
        );
    }

    eprintln!("Loading model {}...", args.model);
    let tts = download::load_from_hub(&args.model)?;
    eprintln!(
        "Model loaded. Available voices: {:?}",
        tts.available_voices
    );

    let state = Arc::new(AppState {
        tts,
        model_id: args.model,
        default_format: args.default_format,
    });

    let app = Router::new()
        .route("/v1/audio/speech", post(speech_handler))
        .route("/v1/models", get(list_models))
        .route("/v1/voices", get(list_voices))
        .route("/health", get(health))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("{}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    eprintln!("Listening on http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}
