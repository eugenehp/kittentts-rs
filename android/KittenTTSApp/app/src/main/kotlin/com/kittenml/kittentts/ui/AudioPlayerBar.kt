package com.kittenml.kittentts.ui

import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import com.kittenml.kittentts.PlayState
import com.kittenml.kittentts.R
import com.kittenml.kittentts.TTSEngine
import kotlinx.coroutines.delay
import kotlin.random.Random

// ─── AudioPlayerBar ───────────────────────────────────────────────────────────

/**
 * Sticky bottom bar shown while audio is playing or after it finishes.
 * Mirrors iOS AudioPlayerBar.swift in structure and behaviour.
 */
@Composable
fun AudioPlayerBar(
    engine: TTSEngine,
    playState: PlayState,
    playProgress: Float,
    modifier: Modifier = Modifier,
) {
    val isPlaying  = playState is PlayState.Playing
    val durationMs = (playState as? PlayState.Playing)?.durationMs ?: 0

    Surface(
        modifier = modifier.fillMaxWidth(),
        tonalElevation = 4.dp,
        shadowElevation = 8.dp,
    ) {
        Column {
            HorizontalDivider()

            Row(
                modifier = Modifier
                    .padding(horizontal = 20.dp, vertical = 12.dp)
                    .fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(16.dp),
            ) {

                // ── Play / Pause / Replay button ──────────────────────────
                IconButton(onClick = { engine.togglePlay() }) {
                    when {
                        isPlaying -> Icon(
                            painter = painterResource(R.drawable.ic_pause),
                            contentDescription = "Pause",
                            tint = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.size(28.dp),
                        )
                        playProgress > 0f -> Icon(
                            painter = painterResource(R.drawable.ic_replay),
                            contentDescription = "Replay",
                            tint = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.size(28.dp),
                        )
                        else -> Icon(
                            painter = painterResource(R.drawable.ic_play_arrow),
                            contentDescription = "Play",
                            tint = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.size(28.dp),
                        )
                    }
                }

                // ── Scrubber + timestamps ─────────────────────────────────
                Column(modifier = Modifier.weight(1f)) {
                    LinearProgressIndicator(
                        progress = { playProgress.coerceIn(0f, 1f) },
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(4.dp)
                            .clip(RoundedCornerShape(2.dp)),
                        color = MaterialTheme.colorScheme.primary,
                        trackColor = MaterialTheme.colorScheme.surfaceVariant,
                    )
                    Spacer(Modifier.height(4.dp))
                    Row {
                        Text(
                            text = formatMs((playProgress * durationMs).toLong()),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.weight(1f))
                        if (durationMs > 0) {
                            Text(
                                text = formatMs(durationMs.toLong()),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }

                // ── Animated waveform ─────────────────────────────────────
                AnimatedWaveform(playing = isPlaying)
            }
        }
    }
}

// ─── AnimatedWaveform ─────────────────────────────────────────────────────────

private const val BAR_COUNT = 20

@Composable
private fun AnimatedWaveform(playing: Boolean) {
    // Snapshot state: each bar height drives an independent animated value
    val targets = remember { mutableStateListOf<Float>().also { list ->
        repeat(BAR_COUNT) { list.add(4f) }
    }}

    LaunchedEffect(playing) {
        if (!playing) {
            targets.replaceAll { 4f }
            return@LaunchedEffect
        }
        while (true) {
            for (i in 0 until BAR_COUNT) targets[i] = Random.nextFloat() * 22f + 4f
            delay(250)
        }
    }

    Row(
        modifier = Modifier
            .width(72.dp)
            .height(28.dp),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for (i in 0 until BAR_COUNT) {
            val h by animateFloatAsState(
                targetValue = targets[i],
                animationSpec = tween(200, easing = EaseInOutQuad),
                label = "bar$i",
            )
            Box(
                modifier = Modifier
                    .width(2.5.dp)
                    .height(h.dp)
                    .clip(RoundedCornerShape(1.5.dp))
                    .background(MaterialTheme.colorScheme.primary.copy(alpha = 0.85f)),
            )
        }
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

private fun formatMs(ms: Long): String {
    val s = (ms / 1000).toInt()
    return "%d:%02d".format(s / 60, s % 60)
}
