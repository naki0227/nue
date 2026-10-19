import Foundation
import Speech

/// On-device speech recognition -> subtitle lines. Nothing leaves the device
/// when the recognizer supports on-device recognition.
public enum Transcriber {
    public static func requestAuthorization() async -> Bool {
        await withCheckedContinuation { continuation in
            SFSpeechRecognizer.requestAuthorization { continuation.resume(returning: $0 == .authorized) }
        }
    }

    public static func transcribe(audioURL: URL, locale: Locale = .current) async throws -> [TimedText] {
        guard let recognizer = SFSpeechRecognizer(locale: locale) ?? SFSpeechRecognizer(), recognizer.isAvailable else {
            return []
        }
        let request = SFSpeechURLRecognitionRequest(url: audioURL)
        request.shouldReportPartialResults = false
        request.addsPunctuation = true
        if recognizer.supportsOnDeviceRecognition {
            request.requiresOnDeviceRecognition = true
        }
        let box = ResultBox()
        let result: SFSpeechRecognitionResult = try await withCheckedThrowingContinuation { continuation in
            box.task = recognizer.recognitionTask(with: request) { result, error in
                if let error {
                    box.finish { continuation.resume(throwing: error) }
                } else if let result, result.isFinal {
                    box.finish { continuation.resume(returning: result) }
                }
            }
        }
        let words = result.bestTranscription.segments.map {
            TimedWord(text: $0.substring, start: $0.timestamp, end: $0.timestamp + $0.duration)
        }
        return LocalPlanner.groupSubtitles(words)
    }
}

/// Resumes the continuation exactly once even if the recognizer calls back again.
private final class ResultBox: @unchecked Sendable {
    private let lock = NSLock()
    private var done = false
    var task: SFSpeechRecognitionTask?

    func finish(_ resume: () -> Void) {
        lock.lock()
        defer { lock.unlock() }
        guard !done else { return }
        done = true
        resume()
    }
}
