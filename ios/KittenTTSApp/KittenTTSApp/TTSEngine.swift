import Foundation
import AVFoundation
import Combine

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - Engine state

enum EngineState: Equatable {
    case downloading(fraction: Double, label: String)
    case loading
    case ready
    case error(String)
}

enum PlayState: Equatable {
    case idle
    case synthesizing
    case playing(duration: TimeInterval)
}

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - TTSEngine

/// Main ViewModel — owns the Rust model handle and drives all state transitions.
@MainActor
final class TTSEngine: ObservableObject {

    // ── Published state ────────────────────────────────────────────────────
    @Published private(set) var engineState: EngineState = .downloading(fraction: 0, label: "Initialising…")
    @Published private(set) var voices: [String] = []
    @Published private(set) var playState: PlayState = .idle
    @Published private(set) var playProgress: Double = 0   // 0…1 while playing
    @Published              var synthError: String?

    // ── Private ────────────────────────────────────────────────────────────
    private var modelHandle: OpaquePointer?   // KittenTtsHandle *
    private var audioEngine  = AVAudioEngine()
    private var playerNode   = AVAudioPlayerNode()
    private var progressTimer: Timer?
    private var audioFile: AVAudioFile?
    private var audioFileURL: URL?

    // ── Model file locations ───────────────────────────────────────────────
    private static let appSupport: URL = {
        let fm = FileManager.default
        let base = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("KittenTTS", isDirectory: true)
        try? fm.createDirectory(at: base, withIntermediateDirectories: true)
        return base
    }()

    private static let onnxURL   = appSupport.appendingPathComponent("kitten_tts_mini_v0_8.onnx")
    private static let voicesURL = appSupport.appendingPathComponent("voices.npz")
    private static let configURL = appSupport.appendingPathComponent("config.json")

    private static let hfBase = "https://huggingface.co/KittenML/kitten-tts-mini-0.8/resolve/main"

    // ─────────────────────────────────────────────────────────────────────
    init() {
        audioEngine.attach(playerNode)
        Task { await setUp() }
    }

    deinit {
        progressTimer?.invalidate()
        if let h = modelHandle { kittentts_model_free(h) }
    }

    // ─────────────────────────────────────────────────────────────────────
    // MARK: - Setup

    private func setUp() async {
        // 1. Ensure model files are on disk
        do {
            try await ensureModels()
        } catch {
            engineState = .error("Download failed: \(error.localizedDescription)")
            return
        }

        // 2. Load the Rust model
        engineState = .loading
        await loadModel()
    }

    // ─────────────────────────────────────────────────────────────────────
    // MARK: - Model download

    private func ensureModels() async throws {
        let needed: [(URL, String)] = [
            (Self.onnxURL,   "kitten_tts_mini_v0_8.onnx"),
            (Self.voicesURL, "voices.npz"),
            (Self.configURL, "config.json"),
        ]

        let missing = needed.filter { !FileManager.default.fileExists(atPath: $0.0.path) }
        guard !missing.isEmpty else { return }

        for (idx, (dest, filename)) in missing.enumerated() {
            let label = "Downloading \(filename)…"
            let baseFraction = Double(idx) / Double(missing.count)
            engineState = .downloading(fraction: baseFraction, label: label)

            let remote = URL(string: "\(Self.hfBase)/\(filename)")!
            try await download(url: remote, to: dest) { fraction in
                Task { @MainActor in
                    let overall = baseFraction + fraction / Double(missing.count)
                    self.engineState = .downloading(fraction: overall, label: label)
                }
            }
        }
    }

    private func download(url: URL, to dest: URL, progress: @escaping (Double) -> Void) async throws {
        return try await withCheckedThrowingContinuation { cont in
            let task = URLSession.shared.downloadTask(with: url) { tmpURL, _, error in
                if let error {
                    cont.resume(throwing: error)
                    return
                }
                guard let tmpURL else {
                    cont.resume(throwing: URLError(.unknown))
                    return
                }
                do {
                    let fm = FileManager.default
                    if fm.fileExists(atPath: dest.path) { try fm.removeItem(at: dest) }
                    try fm.moveItem(at: tmpURL, to: dest)
                    cont.resume()
                } catch {
                    cont.resume(throwing: error)
                }
            }
            // Wire progress reporting via KVO
            let obs = task.progress.observe(\.fractionCompleted, options: .new) { p, _ in
                progress(p.fractionCompleted)
            }
            task.resume()
            // Retain observer for the task lifetime
            objc_setAssociatedObject(task, &AssocKey.obs, obs, .OBJC_ASSOCIATION_RETAIN)
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // MARK: - Model loading

    private func loadModel() async {
        // Locate espeak-ng-data inside the app bundle.
        //
        // Bundle.main.path(forResource:ofType:) works for individual files
        // but is unreliable for *directories* (folder references): it can
        // silently return nil even when the folder is present.
        // The correct approach for a bundled directory is to append to
        // resourceURL, then verify the path actually exists on disk.
        //
        // espeak_ng_InitializePath() expects the espeak-ng-data directory
        // itself (it then opens espeak-ng-data/phontab, espeak-ng-data/en/,
        // etc.). If this path is wrong, espeak_ng_Initialize() returns
        // errno 2 (ENOENT = "No such file or directory").
        let espeakDataPath: String? = {
            let fm = FileManager.default
            // Primary: resourceURL + directory name (works for folder refs)
            if let url = Bundle.main.resourceURL?
                    .appendingPathComponent("espeak-ng-data"),
               fm.fileExists(atPath: url.path) {
                return url.path
            }
            // Fallback: old path(forResource:ofType:) API
            if let p = Bundle.main.path(forResource: "espeak-ng-data",
                                        ofType: nil),
               fm.fileExists(atPath: p) {
                return p
            }
            return nil
        }()

        guard let dataPath = espeakDataPath else {
            engineState = .error(
                "espeak-ng-data not found in the app bundle.\n\n" +
                "In Xcode: drag ios/espeak-ng-data/ into the Project " +
                "Navigator and choose \"Create folder references\" " +
                "(blue folder icon), then rebuild."
            )
            return
        }

        // Verify the phoneme table exists so we get a clear error message
        // instead of a cryptic errno from inside the Rust library.
        let phontab = dataPath + "/phontab"
        guard FileManager.default.fileExists(atPath: phontab) else {
            engineState = .error(
                "espeak-ng-data found at \(dataPath) but phontab is missing.\n\n" +
                "Re-run ios/build_rust_ios.sh to regenerate the data folder, " +
                "then drag it into Xcode again."
            )
            return
        }

        kittentts_set_espeak_data_path(dataPath)

        let handle = await Task.detached(priority: .userInitiated) {
            kittentts_model_load(
                Self.onnxURL.path,
                Self.voicesURL.path
            )
        }.value

        guard let handle else {
            engineState = .error("Failed to load model — check console for details.")
            return
        }
        modelHandle = handle

        // Fetch voice list
        if let cVoices = kittentts_model_voices(handle) {
            let json = String(cString: cVoices)
            kittentts_free_string(cVoices)
            voices = (try? JSONDecoder().decode([String].self,
                                                from: Data(json.utf8))) ?? []
        }

        engineState = .ready
    }

    // ─────────────────────────────────────────────────────────────────────
    // MARK: - Synthesis

    func synthesize(text: String, voice: String, speed: Float) async {
        guard case .ready = engineState, let handle = modelHandle else { return }
        stop()
        playState = .synthesizing
        synthError = nil

        let outURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("kittentts_\(UUID().uuidString).wav")

        let errPtr = await Task.detached(priority: .userInitiated) {
            kittentts_synthesize_to_file(handle, text, voice, speed, outURL.path)
        }.value

        if let errPtr {
            synthError = String(cString: errPtr)
            kittentts_free_error(errPtr)
            playState = .idle
            return
        }

        audioFileURL = outURL
        do {
            try play(url: outURL)
        } catch {
            synthError = error.localizedDescription
            playState = .idle
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // MARK: - Audio playback

    private func play(url: URL) throws {
        audioEngine.stop()

        let file = try AVAudioFile(forReading: url)
        audioFile = file

        let format = file.processingFormat
        let duration = Double(file.length) / format.sampleRate

        audioEngine.connect(playerNode, to: audioEngine.mainMixerNode, format: format)

        try AVAudioSession.sharedInstance().setCategory(.playback, mode: .default)
        try AVAudioSession.sharedInstance().setActive(true)
        try audioEngine.start()

        playerNode.scheduleFile(file, at: nil) { [weak self] in
            Task { @MainActor [weak self] in
                self?.playState = .idle
                self?.playProgress = 0
                self?.progressTimer?.invalidate()
            }
        }
        playerNode.play()

        playState = .playing(duration: duration)

        // Progress timer — fires 30 fps
        progressTimer?.invalidate()
        let startTime = Date()
        progressTimer = Timer.scheduledTimer(withTimeInterval: 1.0/30.0, repeats: true) { [weak self] _ in
            guard let self else { return }
            let elapsed = Date().timeIntervalSince(startTime)
            Task { @MainActor in
                self.playProgress = min(elapsed / duration, 1.0)
            }
        }
    }

    func stop() {
        playerNode.stop()
        audioEngine.stop()
        progressTimer?.invalidate()
        playProgress = 0
        playState = .idle
    }

    func togglePlay() {
        switch playState {
        case .playing:
            playerNode.pause()
            progressTimer?.invalidate()
            playState = .idle
        case .idle:
            if let url = audioFileURL {
                try? play(url: url)
            }
        default: break
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // MARK: - Helpers

    var isReady: Bool {
        if case .ready = engineState { return true }
        return false
    }

    var displayVoices: [String] {
        voices.isEmpty ? ["(no model)"] : voices
    }
}

// ─────────────────────────────────────────────────────────────────────────────
private enum AssocKey {
    static var obs = "progressObserver"
}
