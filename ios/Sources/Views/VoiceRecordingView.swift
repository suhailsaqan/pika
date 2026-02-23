import SwiftUI

struct VoiceRecordingView: View {
    let recorder: VoiceRecorder
    let onSend: () -> Void
    let onCancel: () -> Void

    var body: some View {
        VStack(spacing: 8) {
            // Transcript (scrolling, appears as text comes in)
            if !recorder.transcript.isEmpty {
                ScrollView(.vertical, showsIndicators: false) {
                    Text(recorder.transcript)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 60)
            }

            // Duration + waveform
            HStack(spacing: 10) {
                // Recording indicator dot + duration
                HStack(spacing: 6) {
                    Circle()
                        .fill(Color.red)
                        .frame(width: 8, height: 8)
                        .opacity(recorder.isPaused ? 0.4 : 1)

                    Text(formattedDuration)
                        .font(.subheadline.monospacedDigit())
                        .foregroundStyle(.primary)
                }
                .frame(width: 64, alignment: .leading)

                WaveformView(levels: recorder.levels)
                    .frame(height: 28)
            }

            // Controls: delete, pause/resume, send
            HStack {
                Button {
                    onCancel()
                } label: {
                    Image(systemName: "trash")
                        .font(.body)
                        .foregroundStyle(.secondary)
                        .frame(width: 36, height: 36)
                }

                Spacer()

                Button {
                    if recorder.isPaused {
                        recorder.resumeRecording()
                    } else {
                        recorder.pauseRecording()
                    }
                } label: {
                    Image(systemName: recorder.isPaused ? "record.circle" : "pause.circle.fill")
                        .font(.title2)
                        .foregroundStyle(.primary)
                        .frame(width: 36, height: 36)
                }

                Spacer()

                Button {
                    onSend()
                } label: {
                    Image(systemName: "arrow.up.circle.fill")
                        .font(.title2)
                        .frame(width: 36, height: 36)
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    private var formattedDuration: String {
        let minutes = Int(recorder.duration) / 60
        let seconds = Int(recorder.duration) % 60
        return String(format: "%d:%02d", minutes, seconds)
    }
}

private struct WaveformView: View {
    let levels: [CGFloat]

    private let barWidth: CGFloat = 3
    private let barSpacing: CGFloat = 2

    var body: some View {
        GeometryReader { geo in
            let maxBars = Int(geo.size.width / (barWidth + barSpacing))
            let visibleLevels = levels.suffix(maxBars)
            let height = geo.size.height

            HStack(alignment: .center, spacing: barSpacing) {
                ForEach(Array(visibleLevels.enumerated()), id: \.offset) { _, level in
                    RoundedRectangle(cornerRadius: 1.5)
                        .fill(Color.accentColor.opacity(0.7))
                        .frame(width: barWidth, height: max(2, level * height))
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .trailing)
        }
    }
}
