import AVFoundation
import Accelerate
import Speech

@Observable
@MainActor
final class VoiceRecorder {
    private(set) var isRecording = false
    private(set) var isPaused = false
    private(set) var duration: TimeInterval = 0
    private(set) var levels: [CGFloat] = []
    private(set) var recordingURL: URL?
    private(set) var transcript: String = ""

    private var audioEngine: AVAudioEngine?
    private var audioFile: AVAudioFile?
    private var tempCAFURL: URL?
    private var timer: Timer?
    private var recordingStartTime: Date?
    private var pausedDuration: TimeInterval = 0
    private var pauseStartTime: Date?

    private var speechRecognizer: SFSpeechRecognizer?
    private var speechRequest: SFSpeechAudioBufferRecognitionRequest?
    private var speechTask: SFSpeechRecognitionTask?

    // Latest RMS power from the tap callback, read on main actor by the timer.
    private nonisolated(unsafe) var latestPower: Float = 0

    func startRecording() {
        // Activate audio session BEFORE creating the engine / querying the
        // input format. After a previous stopEngine() deactivates the session,
        // inputNode.outputFormat returns an invalid format (0 Hz / 0 ch).
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.playAndRecord, mode: .measurement, options: [.duckOthers, .defaultToSpeaker])
            try session.setActive(true)
        } catch {
            print("VoiceRecorder: failed to activate audio session: \(error)")
            return
        }

        let engine = AVAudioEngine()
        let inputNode = engine.inputNode
        let inputFormat = inputNode.outputFormat(forBus: 0)

        // Temp file for raw PCM recording
        let tempDir = FileManager.default.temporaryDirectory
        let cafURL = tempDir.appendingPathComponent("voice_\(UUID().uuidString).caf")
        tempCAFURL = cafURL

        do {
            audioFile = try AVAudioFile(forWriting: cafURL, settings: inputFormat.settings)
        } catch {
            print("VoiceRecorder: failed to create audio file: \(error)")
            return
        }

        // Set up speech recognition (best-effort, non-blocking)
        startSpeechRecognition()

        inputNode.installTap(onBus: 0, bufferSize: 1024, format: inputFormat) { [weak self] buffer, _ in
            guard let self else { return }
            // Write PCM to file
            try? self.audioFile?.write(from: buffer)

            // Feed speech recognizer
            self.speechRequest?.append(buffer)

            // Compute RMS power
            guard let channelData = buffer.floatChannelData?[0] else { return }
            let frames = buffer.frameLength
            var rms: Float = 0
            vDSP_measqv(channelData, 1, &rms, vDSP_Length(frames))
            rms = sqrtf(rms)
            self.latestPower = rms
        }

        do {
            try engine.start()
        } catch {
            print("VoiceRecorder: failed to start engine: \(error)")
            return
        }

        audioEngine = engine
        isRecording = true
        isPaused = false
        duration = 0
        levels = []
        transcript = ""
        pausedDuration = 0
        recordingStartTime = Date()

        // 30Hz timer to poll duration + append power levels
        timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 30.0, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.timerTick()
            }
        }
    }

    func stopRecording() async -> URL? {
        stopEngine()

        guard let cafURL = tempCAFURL else {
            resetState()
            return nil
        }

        // Convert CAF (PCM) -> M4A (AAC)
        let outputURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("voice_\(UUID().uuidString).m4a")

        let success = await convertToM4A(from: cafURL, to: outputURL)

        // Clean up temp CAF
        try? FileManager.default.removeItem(at: cafURL)

        let result: URL?
        if success == true {
            recordingURL = outputURL
            result = outputURL
        } else {
            result = nil
        }

        resetState()
        return result
    }

    func cancelRecording() {
        stopEngine()
        if let cafURL = tempCAFURL {
            try? FileManager.default.removeItem(at: cafURL)
        }
        resetState()
    }

    func pauseRecording() {
        guard isRecording, !isPaused else { return }
        audioEngine?.pause()
        isPaused = true
        pauseStartTime = Date()
    }

    func resumeRecording() {
        guard isRecording, isPaused else { return }
        if let pauseStart = pauseStartTime {
            pausedDuration += Date().timeIntervalSince(pauseStart)
        }
        pauseStartTime = nil
        try? audioEngine?.start()
        isPaused = false
    }

    // MARK: - Speech Recognition

    private func startSpeechRecognition() {
        let recognizer = SFSpeechRecognizer()
        guard let recognizer, recognizer.isAvailable else { return }

        // Check authorization status — only proceed if already authorized.
        // We don't prompt here; the permission is requested lazily by the system
        // when the recognition task starts if status is .notDetermined.
        let status = SFSpeechRecognizer.authorizationStatus()
        guard status == .authorized || status == .notDetermined else { return }

        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.addsPunctuation = true

        speechRecognizer = recognizer
        speechRequest = request

        speechTask = recognizer.recognitionTask(with: request) { [weak self] result, error in
            guard let self, let result else { return }
            Task { @MainActor [weak self] in
                self?.transcript = result.bestTranscription.formattedString
            }
        }
    }

    private func stopSpeechRecognition() {
        speechRequest?.endAudio()
        speechTask?.cancel()
        speechRequest = nil
        speechTask = nil
        speechRecognizer = nil
    }

    // MARK: - Private

    private func timerTick() {
        guard isRecording, !isPaused, let startTime = recordingStartTime else { return }

        var currentPausedDuration = pausedDuration
        if let pauseStart = pauseStartTime {
            currentPausedDuration += Date().timeIntervalSince(pauseStart)
        }
        duration = Date().timeIntervalSince(startTime) - currentPausedDuration

        let rms = latestPower
        let db = 20 * log10f(max(rms, 1e-6))
        let normalized = CGFloat(max(0, min(1, (db + 50) / 50)))
        levels.append(normalized)
    }

    private func stopEngine() {
        timer?.invalidate()
        timer = nil

        stopSpeechRecognition()

        audioEngine?.inputNode.removeTap(onBus: 0)
        audioEngine?.stop()
        audioEngine = nil
        audioFile = nil

        // Deactivate and reset to default mode so subsequent playback
        // doesn't inherit the .measurement receiver route.
        let session = AVAudioSession.sharedInstance()
        try? session.setActive(false, options: .notifyOthersOnDeactivation)
        try? session.setCategory(.playAndRecord, mode: .default, options: [.defaultToSpeaker])
    }

    private func resetState() {
        isRecording = false
        isPaused = false
        duration = 0
        levels = []
        recordingURL = nil
        tempCAFURL = nil
        recordingStartTime = nil
        pausedDuration = 0
        pauseStartTime = nil
        latestPower = 0
        // transcript is intentionally NOT reset here — the caller reads it after stop
    }

    private func convertToM4A(from inputURL: URL, to outputURL: URL) async -> Bool? {
        let asset = AVAsset(url: inputURL)

        guard let exportSession = AVAssetExportSession(asset: asset, presetName: AVAssetExportPresetAppleM4A) else {
            return nil
        }

        exportSession.outputURL = outputURL
        exportSession.outputFileType = .m4a

        await exportSession.export()

        switch exportSession.status {
        case .completed:
            return true
        default:
            if let error = exportSession.error {
                print("VoiceRecorder: export failed: \(error)")
            }
            return false
        }
    }
}
