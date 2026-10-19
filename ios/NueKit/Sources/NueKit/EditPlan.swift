import CoreGraphics
import Foundation

// Swift mirror of docs/edit-plan.schema.json. Plans written here can be
// rendered by the Rust CLI (`muscle render`) and vice versa.

public struct CaptionStyle: Codable, Equatable, Sendable {
    public var font: String?
    public var color: String?
    public var position: String?
    public var box: Bool?
    public var backgroundAsset: String?
    /// normal | large (opening titles)
    public var size: String?

    enum CodingKeys: String, CodingKey {
        case font, color, position, box, size
        case backgroundAsset = "background_asset"
    }

    public init(color: String? = nil, position: String? = nil, box: Bool? = nil, size: String? = nil) {
        self.color = color
        self.position = position
        self.box = box
        self.size = size
    }
}

public struct Cut: Codable, Equatable, Sendable {
    public var startTime: String
    public var endTime: String
    public var description: String?
    public var caption: String?
    public var captionStyle: CaptionStyle?
    public var focusPoint: Double?

    enum CodingKeys: String, CodingKey {
        case startTime = "start_time"
        case endTime = "end_time"
        case description, caption
        case captionStyle = "caption_style"
        case focusPoint = "focus_point"
    }

    public init(start: Double, end: Double) {
        startTime = Timecode.format(start)
        endTime = Timecode.format(end)
    }
}

public struct SoundEvent: Codable, Equatable, Sendable {
    public var timestamp: String
    public var type: String
    public var tag: String?

    public init(at time: Double, type: String, tag: String? = nil) {
        timestamp = Timecode.format(time)
        self.type = type
        self.tag = tag
    }
}

public struct VisualEffect: Codable, Equatable, Sendable {
    public var start: String
    public var end: String?
    public var type: String
    public var speed: String?

    public init(start: Double, end: Double, type: String) {
        self.start = Timecode.format(start)
        self.end = Timecode.format(end)
        self.type = type
    }
}

public struct TimedText: Codable, Equatable, Sendable {
    public var start: String
    public var end: String
    public var text: String
    public var style: CaptionStyle?

    public init(start: Double, end: Double, text: String, style: CaptionStyle? = nil) {
        self.start = Timecode.format(start)
        self.end = Timecode.format(end)
        self.text = text
        self.style = style
    }
}

public struct Overlay: Codable, Equatable, Sendable {
    public var asset: String
    public var start: String
    public var end: String
    public var position: String?
    public var scale: Double?

    public init(asset: String, start: Double, end: Double, position: String? = nil, scale: Double? = nil) {
        self.asset = asset
        self.start = Timecode.format(start)
        self.end = Timecode.format(end)
        self.position = position
        self.scale = scale
    }
}

public struct Thumbnail: Codable, Equatable, Sendable {
    public var timestamp: String
    public var text: String
    public var color: String?

    public init(at time: Double, text: String, color: String? = nil) {
        timestamp = Timecode.format(time)
        self.text = text
        self.color = color
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        timestamp = try c.decode(String.self, forKey: .timestamp)
        text = try c.decodeIfPresent(String.self, forKey: .text) ?? ""
        color = try c.decodeIfPresent(String.self, forKey: .color)
    }
}

public struct RenderOptions: Codable, Equatable, Sendable {
    public var bgm = true
    public var soundEffects = true
    public var subtitles = true

    enum CodingKeys: String, CodingKey {
        case bgm
        case soundEffects = "sound_effects"
        case subtitles
    }

    public init() {}

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        bgm = try c.decodeIfPresent(Bool.self, forKey: .bgm) ?? true
        soundEffects = try c.decodeIfPresent(Bool.self, forKey: .soundEffects) ?? true
        subtitles = try c.decodeIfPresent(Bool.self, forKey: .subtitles) ?? true
    }
}

public enum VideoFormat: String, CaseIterable, Sendable {
    case vertical, horizontal, square

    public var renderSize: CGSize {
        switch self {
        case .vertical: return CGSize(width: 1080, height: 1920)
        case .horizontal: return CGSize(width: 1920, height: 1080)
        case .square: return CGSize(width: 1080, height: 1080)
        }
    }

    public init(planValue: String?) {
        switch planValue {
        case "horizontal", "landscape", "16:9": self = .horizontal
        case "square", "1:1": self = .square
        default: self = .vertical
        }
    }
}

public struct EditPlan: Codable, Equatable, Sendable {
    public var jobId: String?
    public var originalFilename: String
    public var analyzer: String?
    public var format: String?
    public var bgmPath: String?
    public var cuts: [Cut]
    public var editingStyle: [String: String]?
    public var seEvents: [SoundEvent] = []
    public var visualEffects: [VisualEffect] = []
    public var telops: [TimedText] = []
    public var subtitles: [TimedText] = []
    public var overlays: [Overlay] = []
    public var thumbnail: Thumbnail?
    public var options = RenderOptions()

    enum CodingKeys: String, CodingKey {
        case jobId = "job_id"
        case originalFilename = "original_filename"
        case analyzer, format
        case bgmPath = "bgm_path"
        case cuts
        case editingStyle = "editing_style"
        case seEvents = "se_events"
        case visualEffects = "visual_effects"
        case telops, subtitles, overlays, thumbnail, options
    }

    public init(originalFilename: String, analyzer: String, cuts: [Cut] = []) {
        self.originalFilename = originalFilename
        self.analyzer = analyzer
        self.cuts = cuts
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        jobId = try c.decodeIfPresent(String.self, forKey: .jobId)
        originalFilename = try c.decode(String.self, forKey: .originalFilename)
        analyzer = try c.decodeIfPresent(String.self, forKey: .analyzer)
        format = try c.decodeIfPresent(String.self, forKey: .format)
        bgmPath = try c.decodeIfPresent(String.self, forKey: .bgmPath)
        cuts = try c.decode([Cut].self, forKey: .cuts)
        // editing_style is free-form JSON; keep it only when it is a string map.
        editingStyle = try? c.decodeIfPresent([String: String].self, forKey: .editingStyle)
        seEvents = try c.decodeIfPresent([SoundEvent].self, forKey: .seEvents) ?? []
        visualEffects = try c.decodeIfPresent([VisualEffect].self, forKey: .visualEffects) ?? []
        telops = try c.decodeIfPresent([TimedText].self, forKey: .telops) ?? []
        subtitles = try c.decodeIfPresent([TimedText].self, forKey: .subtitles) ?? []
        overlays = try c.decodeIfPresent([Overlay].self, forKey: .overlays) ?? []
        thumbnail = try c.decodeIfPresent(Thumbnail.self, forKey: .thumbnail)
        options = try c.decodeIfPresent(RenderOptions.self, forKey: .options) ?? RenderOptions()
    }

    /// Valid cut ranges in source seconds.
    public var cutRanges: [TimeSpan] {
        cuts.compactMap { cut in
            guard let s = Timecode.parse(cut.startTime), let e = Timecode.parse(cut.endTime), e > s else {
                return nil
            }
            return TimeSpan(start: s, end: e)
        }
    }

    public var outputDuration: Double {
        cutRanges.reduce(0) { $0 + $1.duration }
    }

    public func jsonData() throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        return try encoder.encode(self)
    }
}

public struct TimeSpan: Equatable, Sendable {
    public var start: Double
    public var end: Double
    public var duration: Double { end - start }

    public init(start: Double, end: Double) {
        self.start = start
        self.end = end
    }

    public func contains(_ t: Double) -> Bool { t >= start && t < end }
}

public enum Timecode {
    /// Parses `HH:MM:SS(.fff)`, `MM:SS(.fff)`, plain seconds, and the legacy
    /// `MM:SS:mmm` form. Matches the Rust parser.
    public static func parse(_ value: String) -> Double? {
        let text = value.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: ",", with: ".")
        let parts = text.split(separator: ":", omittingEmptySubsequences: false).map(String.init)
        let numbers = parts.map(Double.init)
        guard !numbers.contains(where: { $0 == nil }) else { return nil }
        let n = numbers.map { $0! }
        let seconds: Double
        switch n.count {
        case 3:
            if !parts[2].contains("."), parts[2].count == 3, n[2] > 59 {
                seconds = n[0] * 60 + n[1] + n[2] / 1000
            } else {
                seconds = n[0] * 3600 + n[1] * 60 + n[2]
            }
        case 2: seconds = n[0] * 60 + n[1]
        case 1: seconds = n[0]
        default: return nil
        }
        return seconds.isFinite && seconds >= 0 ? seconds : nil
    }

    public static func format(_ seconds: Double) -> String {
        let ms = Int((max(seconds, 0) * 1000).rounded())
        return String(
            format: "%02d:%02d:%02d.%03d",
            ms / 3_600_000, (ms % 3_600_000) / 60_000, (ms % 60_000) / 1000, ms % 1000)
    }

    /// Maps a source timestamp onto the edited timeline; nil when cut out.
    public static func sourceToOutput(_ source: Double, ranges: [TimeSpan]) -> Double? {
        var offset = 0.0
        for range in ranges {
            if range.contains(source) { return offset + source - range.start }
            offset += range.duration
        }
        return nil
    }
}
