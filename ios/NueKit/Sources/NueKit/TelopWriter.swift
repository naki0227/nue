import Foundation
#if canImport(FoundationModels)
import FoundationModels
#endif

/// Writes emphasis telops and a thumbnail title. Uses Apple Intelligence
/// (Foundation Models, on-device) when available, otherwise simple rules.
public enum TelopWriter {
    public struct Suggestion: Sendable {
        public var telops: [TimedText]
        public var title: String?
        public var usedAppleIntelligence: Bool
    }

    public static var isAppleIntelligenceAvailable: Bool {
        #if canImport(FoundationModels)
        if #available(iOS 26.0, macOS 26.0, *) {
            if case .available = SystemLanguageModel.default.availability { return true }
        }
        #endif
        return false
    }

    public static func suggest(subtitles: [TimedText], peaks: [Double]) async -> Suggestion {
        #if canImport(FoundationModels)
        if #available(iOS 26.0, macOS 26.0, *), isAppleIntelligenceAvailable, !subtitles.isEmpty {
            if let suggestion = try? await generate(subtitles: subtitles) { return suggestion }
        }
        #endif
        return ruleBased(subtitles: subtitles, peaks: peaks)
    }

    /// Without a language model: emphasize the subtitle lines at loudness peaks.
    static func ruleBased(subtitles: [TimedText], peaks: [Double]) -> Suggestion {
        var telops: [TimedText] = []
        for peak in peaks.prefix(4) {
            guard let line = subtitles.first(where: {
                (Timecode.parse($0.start) ?? .infinity) <= peak && peak < (Timecode.parse($0.end) ?? 0)
            }) else { continue }
            telops.append(TimedText(start: peak, end: peak + 2, text: String(line.text.prefix(14)), style: CaptionStyle(color: "yellow", position: "top", box: true)))
        }
        return Suggestion(telops: telops, title: nil, usedAppleIntelligence: false)
    }

    #if canImport(FoundationModels)
    @available(iOS 26.0, macOS 26.0, *)
    static func generate(subtitles: [TimedText]) async throws -> Suggestion {
        let session = LanguageModelSession(instructions: """
            You are a short-form video editor. From a timestamped transcript, pick the few strongest \
            moments and write punchy on-screen telops in the transcript's language. Use timestamps \
            exactly as they appear in the transcript.
            """)
        let transcript = subtitles.prefix(120).map { "\($0.start): \($0.text)" }.joined(separator: "\n")
        let response = try await session.respond(to: transcript, generating: GeneratedEdit.self)
        let edit = response.content
        let telops = edit.telops.prefix(6).compactMap { telop -> TimedText? in
            guard let start = Timecode.parse(telop.start), !telop.text.isEmpty else { return nil }
            return TimedText(start: start, end: start + 2.5, text: telop.text, style: CaptionStyle(color: "yellow", position: "top", box: true))
        }
        return Suggestion(telops: telops, title: edit.title, usedAppleIntelligence: true)
    }
    #endif
}

#if canImport(FoundationModels)
@available(iOS 26.0, macOS 26.0, *)
@Generable
struct GeneratedTelop {
    @Guide(description: "Timestamp copied from the transcript line, HH:MM:SS.mmm")
    var start: String
    @Guide(description: "Punchy emphasis text, at most 14 characters")
    var text: String
}

@available(iOS 26.0, macOS 26.0, *)
@Generable
struct GeneratedEdit {
    @Guide(description: "One to six telops for the strongest moments")
    var telops: [GeneratedTelop]
    @Guide(description: "Short catchy thumbnail title, at most 16 characters")
    var title: String
}
#endif
