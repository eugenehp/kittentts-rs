package com.kittenml.kittentts.ui

import androidx.compose.animation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.kittenml.kittentts.EngineState
import com.kittenml.kittentts.PlayState
import com.kittenml.kittentts.R
import com.kittenml.kittentts.TTSEngine

// ─── Root screen ─────────────────────────────────────────────────────────────

/**
 * Root composable — switches between loading / error / TTS screens.
 * Mirrors iOS ContentView.swift.
 */
@Composable
fun MainScreen(engine: TTSEngine) {
    val engineState by engine.engineState.collectAsState()
    val voices      by engine.voices.collectAsState()
    val playState   by engine.playState.collectAsState()
    val playProgress by engine.playProgress.collectAsState()
    val synthError  by engine.synthError.collectAsState()

    AnimatedContent(
        targetState = engineState,
        transitionSpec = { fadeIn() togetherWith fadeOut() },
        label = "engineStateTransition",
    ) { state ->
        when (state) {
            is EngineState.Downloading -> DownloadScreen(state.fraction, state.label)
            is EngineState.Loading     -> LoadingScreen()
            is EngineState.Error       -> ErrorScreen(state.message)
            is EngineState.Ready       -> TTSScreen(
                engine       = engine,
                voices       = voices,
                playState    = playState,
                playProgress = playProgress,
                synthError   = synthError,
            )
        }
    }
}

// ─── Download screen ─────────────────────────────────────────────────────────

@Composable
private fun DownloadScreen(fraction: Float, label: String) {
    Box(
        modifier = Modifier.fillMaxSize(),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(24.dp),
            modifier = Modifier.padding(40.dp),
        ) {
            Icon(
                painter = painterResource(R.drawable.ic_download),
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
                modifier = Modifier.size(64.dp),
            )

            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(
                    text = "Setting up KittenTTS",
                    style = MaterialTheme.typography.titleLarge,
                )
                Text(
                    text = "Downloading model files from HuggingFace.\nThis only happens once (~40 MB).",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
            }

            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.widthIn(max = 280.dp),
            ) {
                LinearProgressIndicator(
                    progress = { fraction.coerceIn(0f, 1f) },
                    modifier = Modifier.fillMaxWidth(),
                )
                Text(
                    text = label,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

// ─── Loading screen ───────────────────────────────────────────────────────────

@Composable
private fun LoadingScreen() {
    Box(
        modifier = Modifier.fillMaxSize(),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            CircularProgressIndicator()
            Text(
                text = "Loading model…",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

// ─── Error screen ─────────────────────────────────────────────────────────────

@Composable
private fun ErrorScreen(message: String) {
    Box(
        modifier = Modifier.fillMaxSize().padding(32.dp),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Icon(
                painter = painterResource(R.drawable.ic_warning),
                contentDescription = null,
                tint = MaterialTheme.colorScheme.error,
                modifier = Modifier.size(56.dp),
            )
            Text(
                text = "Something went wrong",
                style = MaterialTheme.typography.titleMedium,
            )
            Text(
                text = message,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
        }
    }
}

// ─── TTS screen ───────────────────────────────────────────────────────────────

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TTSScreen(
    engine: TTSEngine,
    voices: List<String>,
    playState: PlayState,
    playProgress: Float,
    synthError: String?,
) {
    var inputText     by remember { mutableStateOf("Hello! I'm KittenTTS, a fast on-device speech engine.") }
    var selectedVoice by remember { mutableStateOf("") }
    var speed         by remember { mutableFloatStateOf(1.0f) }
    var voiceMenuOpen by remember { mutableStateOf(false) }

    // Default voice
    LaunchedEffect(voices) {
        if (selectedVoice.isEmpty()) selectedVoice = voices.firstOrNull() ?: ""
    }

    val showPlayerBar = playState is PlayState.Playing ||
            (playState == PlayState.Idle && playProgress > 0f)
    val isSynthesizing = playState == PlayState.Synthesizing

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("🐱 KittenTTS") },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                ),
            )
        },
        bottomBar = {
            AnimatedVisibility(
                visible = showPlayerBar,
                enter = slideInVertically(initialOffsetY = { it }) + fadeIn(),
                exit  = slideOutVertically(targetOffsetY = { it }) + fadeOut(),
            ) {
                AudioPlayerBar(
                    engine       = engine,
                    playState    = playState,
                    playProgress = playProgress,
                )
            }
        },
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding)
                .verticalScroll(rememberScrollState())
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(24.dp),
        ) {

            // ── Text input ────────────────────────────────────────────────
            SectionLabel("Text")
            OutlinedTextField(
                value = inputText,
                onValueChange = { inputText = it },
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 120.dp),
                placeholder = { Text("Enter text to synthesise…") },
                keyboardOptions = KeyboardOptions(
                    capitalization = KeyboardCapitalization.Sentences,
                    imeAction = ImeAction.Default,
                ),
                shape = RoundedCornerShape(12.dp),
            )

            // ── Voice picker ──────────────────────────────────────────────
            SectionLabel("Voice")
            ExposedDropdownMenuBox(
                expanded = voiceMenuOpen,
                onExpandedChange = { voiceMenuOpen = it },
            ) {
                OutlinedTextField(
                    value = selectedVoice.ifEmpty { "Select voice" },
                    onValueChange = {},
                    readOnly = true,
                    trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(voiceMenuOpen) },
                    shape = RoundedCornerShape(12.dp),
                    modifier = Modifier
                        .fillMaxWidth()
                        .menuAnchor(),
                )
                ExposedDropdownMenu(
                    expanded = voiceMenuOpen,
                    onDismissRequest = { voiceMenuOpen = false },
                ) {
                    engine.displayVoices.forEach { voice ->
                        DropdownMenuItem(
                            text = { Text(voice) },
                            onClick = {
                                selectedVoice = voice
                                voiceMenuOpen = false
                            },
                        )
                    }
                }
            }

            // ── Speed slider ──────────────────────────────────────────────
            SectionLabel("Speed — %.1f×".format(speed))
            Slider(
                value = speed,
                onValueChange = { speed = it },
                valueRange = 0.5f..2.0f,
                steps = 14,        // 0.5, 0.6, … 2.0 → 15 values, 14 gaps
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    "0.5×",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    "2×",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            // ── Speak button ──────────────────────────────────────────────
            Button(
                onClick = {
                    if (selectedVoice.isNotEmpty()) {
                        engine.synthesize(inputText, selectedVoice, speed)
                    }
                },
                enabled = engine.isReady && !isSynthesizing &&
                          inputText.isNotBlank() && selectedVoice.isNotEmpty(),
                modifier = Modifier
                    .fillMaxWidth()
                    .height(52.dp),
            ) {
                if (isSynthesizing) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(18.dp),
                        strokeWidth = 2.dp,
                        color = MaterialTheme.colorScheme.onPrimary,
                    )
                    Spacer(Modifier.width(8.dp))
                    Text("Synthesizing…", style = MaterialTheme.typography.labelLarge)
                } else {
                    Text("🔊  Speak", style = MaterialTheme.typography.labelLarge)
                }
            }

            // ── Error banner ──────────────────────────────────────────────
            if (synthError != null) {
                Surface(
                    color = MaterialTheme.colorScheme.errorContainer,
                    shape = RoundedCornerShape(10.dp),
                ) {
                    Row(
                        modifier = Modifier.padding(12.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        verticalAlignment = Alignment.Top,
                    ) {
                        Icon(
                            painter = painterResource(R.drawable.ic_warning),
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.onErrorContainer,
                            modifier = Modifier.size(16.dp).padding(top = 2.dp),
                        )
                        Text(
                            text = synthError,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onErrorContainer,
                        )
                    }
                }
            }

            // Bottom spacing so content clears the player bar
            Spacer(Modifier.height(8.dp))
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

@Composable
private fun SectionLabel(text: String) {
    Text(
        text = text,
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}
