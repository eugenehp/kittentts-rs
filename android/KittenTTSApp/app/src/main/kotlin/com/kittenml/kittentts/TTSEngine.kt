package com.kittenml.kittentts

import android.content.Context
import android.media.AudioAttributes
import android.media.MediaPlayer
import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.util.zip.ZipInputStream

private const val TAG = "TTSEngine"

// ─── State types ─────────────────────────────────────────────────────────────

sealed interface EngineState {
    data class Downloading(val fraction: Float, val label: String) : EngineState
    data object Loading : EngineState
    data object Ready : EngineState
    data class Error(val message: String) : EngineState
}

sealed interface PlayState {
    data object Idle : PlayState
    data object Synthesizing : PlayState
    data class Playing(val durationMs: Int) : PlayState
}

// ─── ViewModel ────────────────────────────────────────────────────────────────

/**
 * Main ViewModel — owns the Rust model handle and drives all state transitions.
 * Mirrors iOS TTSEngine.swift in structure and data-flow.
 */
class TTSEngine(private val appContext: Context) : ViewModel() {

    // ── Public state ──────────────────────────────────────────────────────────
    private val _engineState = MutableStateFlow<EngineState>(
        EngineState.Downloading(0f, "Initialising…")
    )
    val engineState: StateFlow<EngineState> = _engineState.asStateFlow()

    private val _voices = MutableStateFlow<List<String>>(emptyList())
    val voices: StateFlow<List<String>> = _voices.asStateFlow()

    private val _playState = MutableStateFlow<PlayState>(PlayState.Idle)
    val playState: StateFlow<PlayState> = _playState.asStateFlow()

    private val _playProgress = MutableStateFlow(0f)   // 0..1
    val playProgress: StateFlow<Float> = _playProgress.asStateFlow()

    private val _synthError = MutableStateFlow<String?>(null)
    val synthError: StateFlow<String?> = _synthError.asStateFlow()

    // ── Private ───────────────────────────────────────────────────────────────
    private var modelHandle: Long = 0L
    private var player: MediaPlayer? = null
    private var lastWavFile: File? = null
    private var progressJob: Job? = null

    // ── Model file locations ──────────────────────────────────────────────────
    private val filesDir: File = appContext.filesDir
    private val cacheDir: File = appContext.cacheDir

    private val onnxFile   = File(filesDir, "kitten_tts_mini_v0_8.onnx")
    private val voicesFile = File(filesDir, "voices.npz")
    private val configFile = File(filesDir, "config.json")

    private val espeakDataDir = File(filesDir, "espeak-ng-data")

    private companion object {
        const val HF_BASE = "https://huggingface.co/KittenML/kitten-tts-mini-0.8/resolve/main"
    }

    // ─────────────────────────────────────────────────────────────────────────
    init {
        viewModelScope.launch { setUp() }
    }

    override fun onCleared() {
        super.onCleared()
        releasePlayer()
        if (modelHandle != 0L) {
            KittenTtsLib.nativeModelFree(modelHandle)
            modelHandle = 0L
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // MARK: - Setup

    private suspend fun setUp() {
        // Check for missing native libraries first (build script not run).
        KittenTtsLib.loadError?.let {
            _engineState.value = EngineState.Error(it)
            return
        }

        // Detect missing assets early and give an actionable message.
        val hasEspeakZip = try {
            appContext.assets.open("espeak-ng-data.zip").close(); true
        } catch (_: Exception) { false }
        if (!hasEspeakZip) {
            _engineState.value = EngineState.Error(
                "espeak-ng-data.zip is missing from the app's assets.\n\n" +
                "Run the native build script first:\n" +
                "  bash android/build_rust_android.sh\n\n" +
                "Then rebuild and reinstall the app."
            )
            return
        }

        try {
            extractEspeakData()
            ensureModels()
        } catch (e: Exception) {
            _engineState.value = EngineState.Error("Setup failed: ${e.message}")
            return
        }
        _engineState.value = EngineState.Loading
        loadModel()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // MARK: - espeak-ng data extraction

    /**
     * Extract espeak-ng-data.zip from assets into internal storage.
     * Runs once; skipped if the phontab sentinel file already exists.
     */
    private suspend fun extractEspeakData() = withContext(Dispatchers.IO) {
        val sentinel = File(espeakDataDir, "phontab")
        if (sentinel.exists()) {
            Log.i(TAG, "espeak-ng-data already extracted")
            return@withContext
        }
        Log.i(TAG, "Extracting espeak-ng-data.zip -> ${espeakDataDir.absolutePath}")
        withContext(Dispatchers.Main) {
            _engineState.value = EngineState.Downloading(0f, "Extracting phoneme data…")
        }

        espeakDataDir.deleteRecursively()
        espeakDataDir.mkdirs()

        appContext.assets.open("espeak-ng-data.zip").use { assetStream ->
            ZipInputStream(assetStream).use { zip ->
                var entry = zip.nextEntry
                while (entry != null) {
                    // Strip the leading "espeak-ng-data/" component so files
                    // land directly under espeakDataDir.
                    val name = entry.name.removePrefix("espeak-ng-data/")
                    if (name.isBlank()) { entry = zip.nextEntry; continue }

                    val dest = File(espeakDataDir, name)
                    if (entry.isDirectory) {
                        dest.mkdirs()
                    } else {
                        dest.parentFile?.mkdirs()
                        FileOutputStream(dest).use { out -> zip.copyTo(out) }
                    }
                    zip.closeEntry()
                    entry = zip.nextEntry
                }
            }
        }

        if (!sentinel.exists()) {
            throw IllegalStateException(
                "espeak-ng-data extracted but phontab is missing — " +
                "re-run build_rust_android.sh to regenerate espeak-ng-data.zip"
            )
        }
        Log.i(TAG, "espeak-ng-data extraction complete")
    }

    // ─────────────────────────────────────────────────────────────────────────
    // MARK: - Model download

    private suspend fun ensureModels() = withContext(Dispatchers.IO) {
        val needed = listOf(
            onnxFile   to "kitten_tts_mini_v0_8.onnx",
            voicesFile to "voices.npz",
            configFile to "config.json",
        )

        // Check if all bundled in assets first (built by build_rust_android.sh)
        for ((dest, assetName) in needed) {
            if (!dest.exists()) {
                try {
                    appContext.assets.open("models/$assetName").use { src ->
                        dest.outputStream().use { dst -> src.copyTo(dst) }
                    }
                    Log.i(TAG, "Copied $assetName from assets")
                } catch (_: Exception) {
                    // Not in assets — will download below
                }
            }
        }

        val missing = needed.filter { (dest, _) -> !dest.exists() }
        if (missing.isEmpty()) return@withContext

        // Download remaining files from HuggingFace
        missing.forEachIndexed { idx, (dest, filename) ->
            val label = "Downloading $filename…"
            val baseF = idx.toFloat() / missing.size

            withContext(Dispatchers.Main) {
                _engineState.value = EngineState.Downloading(baseF, label)
            }

            Log.i(TAG, "Downloading $filename from HuggingFace...")
            val tmp = File(cacheDir, "$filename.tmp")
            downloadFile("$HF_BASE/$filename", tmp) { downloaded, total ->
                if (total > 0) {
                    val frac = baseF + (downloaded.toFloat() / total) / missing.size
                    // Post progress back to main thread
                    viewModelScope.launch(Dispatchers.Main) {
                        _engineState.value =
                            EngineState.Downloading(frac.coerceIn(0f, 1f), label)
                    }
                }
            }
            tmp.renameTo(dest)
            Log.i(TAG, "Downloaded $filename (${dest.length() / 1024} KB)")
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // MARK: - Download helper

    /**
     * Download [urlStr] to [dest], following HTTP redirects (including
     * cross-origin redirects that HuggingFace CDN issues).
     * [onProgress] is called on the IO thread with (bytesDownloaded, totalBytes).
     * totalBytes is -1 when the server does not send Content-Length.
     */
    private fun downloadFile(
        urlStr: String,
        dest: File,
        onProgress: (Long, Long) -> Unit,
    ) {
        var location = urlStr
        var redirects = 0
        var finalConn: HttpURLConnection? = null

        // Follow redirects manually so cross-origin (HuggingFace → CDN) hops work.
        while (redirects < 10) {
            val conn = URL(location).openConnection() as HttpURLConnection
            conn.connectTimeout          = 15_000
            conn.readTimeout             = 60_000
            conn.instanceFollowRedirects = false          // manual redirect loop
            conn.setRequestProperty("User-Agent", "KittenTTS-Android/1.0")
            conn.connect()

            val code = conn.responseCode
            when {
                code in 300..399 -> {
                    location = conn.getHeaderField("Location")
                        ?: error("Redirect with no Location header")
                    conn.disconnect()
                    redirects++
                }
                code == 200 -> { finalConn = conn; break }
                else        -> { conn.disconnect(); error("HTTP $code downloading $urlStr") }
            }
        }

        val conn = finalConn ?: error("Too many redirects for $urlStr")
        val total = conn.contentLengthLong          // -1 when server omits Content-Length
        var downloaded = 0L

        conn.inputStream.use { src ->
            FileOutputStream(dest).use { dst ->
                val buf = ByteArray(32_768)
                var n: Int
                while (src.read(buf).also { n = it } != -1) {
                    dst.write(buf, 0, n)
                    downloaded += n
                    onProgress(downloaded, total)
                }
            }
        }
        conn.disconnect()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // MARK: - Model loading

    private suspend fun loadModel() = withContext(Dispatchers.IO) {
        val dataPath = espeakDataDir.absolutePath
        Log.i(TAG, "Setting espeak data path: $dataPath")
        KittenTtsLib.nativeSetEspeakDataPath(dataPath)

        Log.i(TAG, "Loading model: ${onnxFile.absolutePath}")
        val handle = KittenTtsLib.nativeModelLoad(
            onnxFile.absolutePath,
            voicesFile.absolutePath,
        )

        if (handle == 0L) {
            withContext(Dispatchers.Main) {
                _engineState.value =
                    EngineState.Error("Failed to load model — check logcat for details.")
            }
            return@withContext
        }

        modelHandle = handle
        Log.i(TAG, "Model loaded (handle=$handle)")

        // Decode voice list from JSON array
        val voiceJson = KittenTtsLib.nativeModelVoices(handle)
        val voiceList = parseJsonStringArray(voiceJson)
        Log.i(TAG, "Available voices: $voiceList")

        withContext(Dispatchers.Main) {
            _voices.value = voiceList
            _engineState.value = EngineState.Ready
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // MARK: - Synthesis

    fun synthesize(text: String, voice: String, speed: Float) {
        if (_engineState.value != EngineState.Ready || modelHandle == 0L) return
        stop()
        _playState.value = PlayState.Synthesizing
        _synthError.value = null

        val outFile = File(cacheDir, "kittentts_${System.currentTimeMillis()}.wav")

        viewModelScope.launch {
            val errMsg = withContext(Dispatchers.Default) {
                KittenTtsLib.nativeSynthesizeToFile(
                    modelHandle, text, voice, speed, outFile.absolutePath
                )
            }

            if (errMsg != null) {
                Log.e(TAG, "Synthesis error: $errMsg")
                _synthError.value = errMsg
                _playState.value = PlayState.Idle
                return@launch
            }

            lastWavFile = outFile
            playFile(outFile)
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // MARK: - Audio playback

    private fun playFile(file: File) {
        releasePlayer()

        val mp = MediaPlayer().also { player = it }
        try {
            // AudioAttributes must be set before setDataSource on API 21+.
            // Without this the emulator may route audio to a virtual sink that
            // produces no audible output.
            mp.setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build()
            )
            mp.setDataSource(file.absolutePath)
            mp.prepare()
            val durationMs = mp.duration

            mp.setOnCompletionListener {
                _playState.value = PlayState.Idle
                _playProgress.value = 1f
                progressJob?.cancel()
            }
            mp.start()

            _playState.value = PlayState.Playing(durationMs)
            _playProgress.value = 0f

            // Progress polling — ~30 fps
            progressJob?.cancel()
            progressJob = viewModelScope.launch {
                while (true) {
                    delay(33)
                    val ps = _playState.value
                    if (ps is PlayState.Playing) {
                        val pos = mp.currentPosition.toFloat()
                        _playProgress.value = if (ps.durationMs > 0)
                            (pos / ps.durationMs).coerceIn(0f, 1f)
                        else 0f
                    } else {
                        break
                    }
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "MediaPlayer error: ${e.message}")
            _synthError.value = e.message
            _playState.value = PlayState.Idle
            releasePlayer()
        }
    }

    fun stop() {
        progressJob?.cancel()
        releasePlayer()
        _playState.value = PlayState.Idle
        _playProgress.value = 0f
    }

    fun togglePlay() {
        when (val ps = _playState.value) {
            is PlayState.Playing -> {
                player?.pause()
                progressJob?.cancel()
                _playState.value = PlayState.Idle
            }
            PlayState.Idle -> {
                lastWavFile?.let { f ->
                    if (f.exists()) playFile(f)
                }
            }
            else -> Unit
        }
    }

    private fun releasePlayer() {
        progressJob?.cancel()
        player?.apply {
            try { if (isPlaying) stop() } catch (_: Exception) {}
            release()
        }
        player = null
    }

    // ─────────────────────────────────────────────────────────────────────────
    // MARK: - Helpers

    val isReady: Boolean get() = _engineState.value == EngineState.Ready

    val displayVoices: List<String>
        get() = _voices.value.ifEmpty { listOf("(no model)") }

    /** Parse a compact JSON string array without pulling in a JSON library. */
    private fun parseJsonStringArray(json: String?): List<String> {
        if (json.isNullOrBlank()) return emptyList()
        return try {
            json.trim().removePrefix("[").removeSuffix("]")
                .split(",")
                .map { it.trim().removeSurrounding("\"") }
                .filter { it.isNotBlank() }
        } catch (e: Exception) {
            Log.w(TAG, "Failed to parse voice JSON: $json")
            emptyList()
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // MARK: - Factory

    class Factory(private val context: Context) : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T =
            TTSEngine(context.applicationContext) as T
    }
}
