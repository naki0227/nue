import Foundation

public struct AnalyzeOptions: Sendable {
    public var subtitles = true
    public var jumpCut = true
    public var soundEffects = true
    public var bgm = true
    public var appleIntelligence = true
    public var format: VideoFormat = .vertical
    public var locale: Locale = .current

    public init() {}
}

/// analyze -> plan -> render, all on device.
public enum NuePipeline {
    public enum Stage: Sendable {
        case readingAudio, transcribing, planning, rendering
    }

    public static func analyze(source: URL, options: AnalyzeOptions, workDirectory: URL, progress: @Sendable (Stage) -> Void = { _ in }) async throws -> EditPlan {
        progress(.readingAudio)
        let signals = try await MediaAnalyzer.analyze(url: source)

        var subtitles: [TimedText] = []
        if options.subtitles, signals.hasAudio, await Transcriber.requestAuthorization() {
            progress(.transcribing)
            let audio = workDirectory.appendingPathComponent("speech.m4a")
            try await MediaAnalyzer.exportAudio(from: source, to: audio)
            subtitles = (try? await Transcriber.transcribe(audioURL: audio, locale: options.locale)) ?? []
            try? FileManager.default.removeItem(at: audio)
        }

        progress(.planning)
        var plan = LocalPlanner.buildPlan(
            fileName: source.lastPathComponent,
            duration: signals.duration,
            hasAudio: signals.hasAudio,
            loudness: signals.loudness,
            window: signals.window,
            subtitles: subtitles,
            jumpCut: options.jumpCut)
        if options.appleIntelligence {
            let peaks = plan.seEvents.compactMap { Timecode.parse($0.timestamp) }
            let suggestion = await TelopWriter.suggest(subtitles: subtitles, peaks: peaks)
            plan.telops = suggestion.telops
            if let title = suggestion.title ?? suggestion.telops.first?.text, var thumbnail = plan.thumbnail {
                thumbnail.text = title
                plan.thumbnail = thumbnail
            }
            if suggestion.usedAppleIntelligence { plan.analyzer = "ios-apple-intelligence" }
        }
        plan.format = options.format.rawValue
        plan.options.subtitles = options.subtitles
        plan.options.soundEffects = options.soundEffects
        plan.options.bgm = options.bgm
        if !options.soundEffects { plan.seEvents = [] }
        return plan
    }
}
