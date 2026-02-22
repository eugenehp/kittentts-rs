import SwiftUI

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - Root view — switches between loading / error / TTS screens

struct ContentView: View {
    @EnvironmentObject var engine: TTSEngine

    var body: some View {
        Group {
            switch engine.engineState {
            case .downloading(let fraction, let label):
                DownloadView(fraction: fraction, label: label)
            case .loading:
                LoadingView()
            case .error(let msg):
                ErrorView(message: msg)
            case .ready:
                TTSView()
            }
        }
        .animation(.easeInOut(duration: 0.3), value: engine.engineState)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - Download screen

struct DownloadView: View {
    let fraction: Double
    let label: String

    var body: some View {
        VStack(spacing: 32) {
            Image(systemName: "arrow.down.circle.fill")
                .font(.system(size: 64))
                .foregroundStyle(Color.accentColor)

            VStack(spacing: 12) {
                Text("Setting up KittenTTS")
                    .font(.title2.bold())
                Text("Downloading model files from HuggingFace.\nThis only happens once (~40 MB).")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }

            VStack(spacing: 8) {
                ProgressView(value: fraction)
                    .progressViewStyle(.linear)
                    .tint(.accentColor)
                    .frame(maxWidth: 280)
                Text(label)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(40)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - Loading screen (model init)

struct LoadingView: View {
    var body: some View {
        VStack(spacing: 20) {
            ProgressView()
                .scaleEffect(1.4)
            Text("Loading model…")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - Error screen

struct ErrorView: View {
    let message: String
	
	init(message: String) {
		self.message = message
		print(message)
	}

    var body: some View {
        VStack(spacing: 24) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(size: 56))
                .foregroundStyle(.red)
            Text("Something went wrong")
                .font(.title3.bold())
            Text(message)
				.textSelection(.enabled)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - Main TTS screen

struct TTSView: View {
    @EnvironmentObject var engine: TTSEngine

    @State private var inputText = "Hello! I'm KittenTTS, a fast on-device speech engine."
    @State private var selectedVoice = ""
    @State private var speed: Double = 1.0
    @FocusState private var editorFocused: Bool

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {

                    // ── Text input ─────────────────────────────────────────
                    VStack(alignment: .leading, spacing: 8) {
                        Label("Text", systemImage: "text.quote")
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(.secondary)

                        TextEditor(text: $inputText)
                            .focused($editorFocused)
                            .frame(minHeight: 130)
                            .padding(10)
                            .background(Color(.secondarySystemGroupedBackground))
                            .clipShape(RoundedRectangle(cornerRadius: 12))
                    }

                    // ── Voice picker ───────────────────────────────────────
                    VStack(alignment: .leading, spacing: 8) {
                        Label("Voice", systemImage: "person.wave.2.fill")
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(.secondary)

                        Menu {
                            ForEach(engine.displayVoices, id: \.self) { voice in
                                Button(voice) { selectedVoice = voice }
                            }
                        } label: {
                            HStack {
                                Text(selectedVoice.isEmpty ? "Select voice" : selectedVoice)
                                    .foregroundStyle(selectedVoice.isEmpty ? .secondary : .primary)
                                Spacer()
                                Image(systemName: "chevron.up.chevron.down")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            .padding(.horizontal, 14)
                            .padding(.vertical, 12)
                            .background(Color(.secondarySystemGroupedBackground))
                            .clipShape(RoundedRectangle(cornerRadius: 12))
                        }
                    }

                    // ── Speed slider ───────────────────────────────────────
                    VStack(alignment: .leading, spacing: 8) {
                        Label("Speed — \(speed, specifier: "%.1f")×",
                              systemImage: "gauge.with.needle")
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(.secondary)

                        Slider(value: $speed, in: 0.5...2.0, step: 0.1) {
                            Text("Speed")
                        } minimumValueLabel: {
                            Text("0.5×").font(.caption2).foregroundStyle(.secondary)
                        } maximumValueLabel: {
                            Text("2×").font(.caption2).foregroundStyle(.secondary)
                        }
                    }

                    // ── Speak button ───────────────────────────────────────
                    Button {
                        editorFocused = false
                        guard !selectedVoice.isEmpty else { return }
                        Task {
                            await engine.synthesize(
                                text: inputText,
                                voice: selectedVoice,
                                speed: Float(speed)
                            )
                        }
                    } label: {
                        HStack {
                            if case .synthesizing = engine.playState {
                                ProgressView()
                                    .tint(.white)
                                    .padding(.trailing, 4)
                            } else {
                                Image(systemName: "waveform")
                            }
                            Text(engine.playState == .synthesizing ? "Synthesizing…" : "Speak")
                                .fontWeight(.semibold)
                        }
                        .frame(maxWidth: .infinity, minHeight: 52)
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(!engine.isReady
                              || engine.playState == .synthesizing
                              || inputText.isEmpty
                              || selectedVoice.isEmpty)

                    // ── Error banner ───────────────────────────────────────
                    if let err = engine.synthError {
                        Label(err, systemImage: "exclamationmark.circle.fill")
                            .font(.footnote)
                            .foregroundStyle(.red)
                            .padding(12)
                            .background(Color.red.opacity(0.08))
                            .clipShape(RoundedRectangle(cornerRadius: 10))
                    }
                }
                .padding(20)
            }
            .background(Color(.systemGroupedBackground))
            .navigationTitle("🐱 KittenTTS")
            .navigationBarTitleDisplayMode(.large)
            .safeAreaInset(edge: .bottom) {
                // Player bar shown whenever there is audio ready
                if case .playing(let dur) = engine.playState {
                    AudioPlayerBar(duration: dur)
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                } else if engine.playState == .idle, engine.playProgress > 0 {
                    // Still show bar with replay option after audio ends
                    AudioPlayerBar(duration: nil)
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                }
            }
            .animation(.spring(duration: 0.35), value: engine.playState)
            .onAppear {
                if selectedVoice.isEmpty {
                    selectedVoice = engine.voices.first ?? ""
                }
            }
            .onChange(of: engine.voices) { voices in
                if selectedVoice.isEmpty { selectedVoice = voices.first ?? "" }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
#Preview {
    ContentView()
        .environmentObject({
            let e = TTSEngine()
            return e
        }())
}
