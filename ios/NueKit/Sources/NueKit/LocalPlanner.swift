import Foundation

/// RMS loudness sample: time (s) and level (dBFS).
public struct LoudnessPoint: Equatable, Sendable {
    public var time: Double
    public var db: Double

    public init(time: Double, db: Double) {
        self.time = time
        self.db = db
    }
}

/// A recognized word with timing, the input for subtitle grouping.
public struct TimedWord: Equatable, Sendable {
    public var text: String
    public var start: Double
    public var end: Double

    public init(text: String, start: Double, end: Double) {
        self.text = text
        self.start = start
        self.end = end
    }
}

/// On-device editing rules. Same behavior as the Rust local analyzer
/// (muscle/src/local.rs) so both clients produce comparable plans.
public enum LocalPlanner {
    public static func rmsCurve(samples: [Float], sampleRate: Double, window: Double) -> [LoudnessPoint] {
        let size = max(Int(sampleRate * window), 1)
        var points: [LoudnessPoint] = []
        var index = 0
        while index < samples.count {
            let end = min(index + size, samples.count)
            var sum: Float = 0
            for i in index..<end { sum += samples[i] * samples[i] }
            let rms = sqrt(sum / Float(end - index))
            let db = rms > 0 ? 20 * log10(Double(rms)) : -120
            points.append(LoudnessPoint(time: Double(index) / sampleRate, db: max(db, -120)))
            index = end
        }
        return points
    }

    /// Runs of windows quieter than `thresholdDb` lasting at least `minDuration`.
    public static func silences(curve: [LoudnessPoint], window: Double, thresholdDb: Double = -35, minDuration: Double = 0.6) -> [TimeSpan] {
        var result: [TimeSpan] = []
        var start: Double?
        for point in curve {
            if point.db < thresholdDb {
                if start == nil { start = point.time }
            } else if let s = start {
                if point.time - s >= minDuration { result.append(TimeSpan(start: s, end: point.time)) }
                start = nil
            }
        }
        if let s = start, let last = curve.last, last.time + window - s >= minDuration {
            result.append(TimeSpan(start: s, end: .infinity))
        }
        return result
    }

    /// Complement of the silent ranges, padded, merged and cleaned up.
    public static func speechRanges(duration: Double, silences: [TimeSpan]) -> [TimeSpan] {
        let pad = 0.15, mergeGap = 0.4, minLength = 0.5
        var ranges: [TimeSpan] = []
        var cursor = 0.0
        for silence in silences {
            if silence.start > cursor { ranges.append(TimeSpan(start: cursor, end: silence.start)) }
            cursor = min(silence.end, duration)
        }
        if cursor < duration { ranges.append(TimeSpan(start: cursor, end: duration)) }

        var merged: [TimeSpan] = []
        for range in ranges {
            let padded = TimeSpan(start: max(range.start - pad, 0), end: min(range.end + pad, duration))
            if var last = merged.last, padded.start - last.end <= mergeGap {
                last.end = max(last.end, padded.end)
                merged[merged.count - 1] = last
            } else {
                merged.append(padded)
            }
        }
        return merged.filter { $0.duration >= minLength }
    }

    /// Local maxima well above the typical level, at least `minGap` apart.
    public static func loudnessPeaks(curve: [LoudnessPoint], minGap: Double) -> [Double] {
        let audible = curve.map(\.db).filter { $0 > -60 }.sorted()
        guard audible.count >= 8 else { return [] }
        let threshold = max(audible[audible.count / 2] + 8, -22)
        var candidates: [LoudnessPoint] = []
        if curve.count >= 3 {
            for i in 1..<(curve.count - 1) {
                let p = curve[i]
                if p.db >= threshold, p.db >= curve[i - 1].db, p.db >= curve[i + 1].db { candidates.append(p) }
            }
        }
        var picked: [Double] = []
        for c in candidates.sorted(by: { $0.db > $1.db }) where picked.allSatisfy({ abs($0 - c.time) >= minGap }) {
            picked.append(c.time)
        }
        return picked.sorted()
    }

    public static func buildPlan(
        fileName: String,
        duration: Double,
        hasAudio: Bool,
        loudness: [LoudnessPoint],
        window: Double,
        subtitles: [TimedText],
        jumpCut: Bool
    ) -> EditPlan {
        var plan = EditPlan(originalFilename: fileName, analyzer: "ios")
        let silent = hasAudio ? silences(curve: loudness, window: window) : []
        var ranges = jumpCut && !silent.isEmpty ? speechRanges(duration: duration, silences: silent) : []
        if ranges.reduce(0, { $0 + $1.duration }) < duration * 0.25 {
            ranges = [TimeSpan(start: 0, end: duration)]
        }
        plan.cuts = ranges.map { Cut(start: $0.start, end: $0.end) }

        let peaks = Array(loudnessPeaks(curve: loudness, minGap: 6).filter { t in ranges.contains { $0.contains(t) } }.prefix(12))
        for t in peaks {
            plan.seEvents.append(SoundEvent(at: t, type: "impact", tag: "loudness_peak"))
            plan.visualEffects.append(VisualEffect(start: t, end: min(t + 1.2, duration), type: "zoom_in"))
            plan.overlays.append(Overlay(asset: "exclaim", start: t, end: min(t + 1, duration), position: "top_right", scale: 0.18))
        }
        let thumbTime = peaks.first ?? ranges.first.map { $0.start + $0.duration / 2 } ?? duration / 2
        let title = subtitles.map { $0.text.trimmingCharacters(in: .whitespaces) }.first { $0.count >= 4 } ?? ""
        plan.thumbnail = Thumbnail(at: thumbTime, text: String(title.prefix(16)), color: "yellow")
        plan.editingStyle = ["tempo": jumpCut ? "fast" : "natural", "mood": Double(peaks.count) / max(duration, 1) > 0.05 ? "energetic" : "calm"]
        plan.subtitles = subtitles
        return plan
    }

    /// Display width in half-width units (wide glyphs count 2).
    static func units(_ c: Character) -> Int { c.isASCII ? 1 : 2 }

    /// Groups recognized words into subtitle lines: breaks on long pauses,
    /// sentence ends, or when a line exceeds `maxUnits`.
    public static func groupSubtitles(_ words: [TimedWord], maxUnits: Int = 32, maxGap: Double = 0.6) -> [TimedText] {
        var lines: [TimedText] = []
        var text = ""
        var start = 0.0
        var end = 0.0
        func flush() {
            let trimmed = text.trimmingCharacters(in: .whitespaces)
            if !trimmed.isEmpty { lines.append(TimedText(start: start, end: end, text: trimmed)) }
            text = ""
        }
        for word in words {
            let piece = word.text.trimmingCharacters(in: .whitespaces)
            guard !piece.isEmpty else { continue }
            let currentUnits = text.reduce(0) { $0 + units($1) }
            let pieceUnits = piece.reduce(0) { $0 + units($1) }
            if !text.isEmpty && (word.start - end > maxGap || currentUnits + pieceUnits + 1 > maxUnits) {
                flush()
            }
            if text.isEmpty {
                start = word.start
                text = piece
            } else {
                // Latin words need a space; Japanese segments join directly.
                let needsSpace = text.last.map { $0.isASCII && $0 != " " } == true && piece.first?.isASCII == true
                text += needsSpace ? " " + piece : piece
            }
            end = word.end
            if let last = piece.last, "。！？.!?".contains(last) { flush() }
        }
        flush()
        return lines
    }

    /// SRT on the edited timeline, splitting lines that straddle cuts.
    public static func outputSRT(subtitles: [TimedText], ranges: [TimeSpan]) -> String {
        func stamp(_ t: Double) -> String { Timecode.format(t).replacingOccurrences(of: ".", with: ",") }
        var out = ""
        var index = 1
        for line in subtitles {
            guard let s = Timecode.parse(line.start), let e = Timecode.parse(line.end) else { continue }
            for range in ranges {
                let a = max(s, range.start), b = min(e, range.end)
                guard b - a >= 0.3,
                      let os = Timecode.sourceToOutput(a, ranges: ranges),
                      let oe = Timecode.sourceToOutput(b - 0.001, ranges: ranges).map({ $0 + 0.001 })
                else { continue }
                out += "\(index)\n\(stamp(os)) --> \(stamp(oe))\n\(line.text)\n\n"
                index += 1
            }
        }
        return out
    }
}
