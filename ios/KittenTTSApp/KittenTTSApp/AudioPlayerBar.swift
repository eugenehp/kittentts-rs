import SwiftUI

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - AudioPlayerBar

/// Sticky bottom bar shown while audio is playing (or after it finishes).
struct AudioPlayerBar: View {
    @EnvironmentObject var engine: TTSEngine

    /// Total duration in seconds.  `nil` after playback finishes.
    let duration: TimeInterval?

    // ── Waveform animation state ──────────────────────────────────────────
    @State private var barHeights: [CGFloat] = Array(repeating: 4, count: 20)
    @State private var animating = false

    var body: some View {
        VStack(spacing: 0) {
            Divider()

            HStack(spacing: 16) {

                // Play / pause / replay button
                Button {
                    engine.togglePlay()
                } label: {
                    Image(systemName: iconName)
                        .font(.title2.weight(.semibold))
                        .foregroundStyle(Color.accentColor)
                        .frame(width: 44, height: 44)
                }

                // Progress + waveform
                VStack(spacing: 6) {
                    // Scrubber
                    GeometryReader { geo in
                        ZStack(alignment: .leading) {
                            Capsule()
                                .fill(Color.secondary.opacity(0.25))
                                .frame(height: 4)
                            Capsule()
                                .fill(Color.accentColor)
                                .frame(width: geo.size.width * engine.playProgress, height: 4)
                        }
                        .frame(maxHeight: .infinity, alignment: .center)
                    }
                    .frame(height: 14)

                    // Timestamps
                    HStack {
                        Text(formatTime(engine.playProgress * (duration ?? 0)))
                            .font(.caption2.monospacedDigit())
                            .foregroundStyle(.secondary)
                        Spacer()
                        if let dur = duration {
                            Text(formatTime(dur))
                                .font(.caption2.monospacedDigit())
                                .foregroundStyle(.secondary)
                        }
                    }
                }

                // Animated waveform bars
                HStack(alignment: .center, spacing: 2) {
                    ForEach(0..<20, id: \.self) { i in
                        Capsule()
                            .fill(Color.accentColor.opacity(0.85))
                            .frame(width: 2.5, height: barHeights[i])
                    }
                }
                .frame(width: 72, height: 28, alignment: .center)
                .clipped()
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
            .background(.ultraThinMaterial)
        }
        .onAppear  { startAnimation() }
        .onDisappear { animating = false }
        .onChange(of: engine.playState) { state in
            if case .playing = state { startAnimation() } else { animating = false }
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    private var iconName: String {
        switch engine.playState {
        case .playing: return "pause.fill"
        default:       return engine.playProgress > 0 ? "arrow.clockwise" : "play.fill"
        }
    }

    private func formatTime(_ t: TimeInterval) -> String {
        let secs = Int(t)
        return String(format: "%d:%02d", secs / 60, secs % 60)
    }

    // ─────────────────────────────────────────────────────────────────────
    // MARK: - Waveform animation

    private func startAnimation() {
        animating = true
        animate()
    }

    private func animate() {
        guard animating else { return }
        withAnimation(.easeInOut(duration: Double.random(in: 0.2...0.45))) {
            barHeights = barHeights.map { _ in CGFloat.random(in: 4...26) }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
            animate()
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
#Preview {
    AudioPlayerBar(duration: 3.5)
        .environmentObject({
            let e = TTSEngine()
            return e
        }())
}
