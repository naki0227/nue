import AVFoundation
import CoreGraphics
import CoreText
import Foundation
import ImageIO
import QuartzCore

/// AVFoundation renderer: edit plan -> finished video, entirely on device.
/// Mirrors the Rust/FFmpeg renderer: reframing, zoom/pan, telops, subtitles,
/// icon overlays, sound effects and ducked BGM on the edited timeline.
public enum Renderer {
    static let timescale: CMTimeScale = 600

    static func time(_ seconds: Double) -> CMTime {
        CMTime(seconds: seconds, preferredTimescale: timescale)
    }

    public struct Output: Sendable {
        public var video: URL
        public var subtitles: URL?
        public var duration: Double
    }

    public static func render(plan: EditPlan, source: URL, assets: AssetLibrary, output: URL) async throws -> Output {
        let asset = AVURLAsset(url: source)
        guard let sourceVideo = try await asset.loadTracks(withMediaType: .video).first else {
            throw NueError.noVideoTrack
        }
        let sourceAudio = try await asset.loadTracks(withMediaType: .audio).first
        let sourceDuration = try await asset.load(.duration).seconds
        let ranges = plan.cutRanges
            .map { TimeSpan(start: min($0.start, sourceDuration), end: min($0.end, sourceDuration)) }
            .filter { $0.duration > 0.05 }
        guard !ranges.isEmpty else { throw NueError.noUsableCuts }

        let composition = AVMutableComposition()
        guard let videoTrack = composition.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid) else {
            throw NueError.exportFailed("could not add a video track")
        }
        let voiceTrack = sourceAudio == nil ? nil : composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid)

        var cursor = CMTime.zero
        var cutStarts: [Double] = []
        for range in ranges {
            let timeRange = CMTimeRange(start: time(range.start), duration: time(range.duration))
            cutStarts.append(cursor.seconds)
            try videoTrack.insertTimeRange(timeRange, of: sourceVideo, at: cursor)
            if let sourceAudio, let voiceTrack {
                try voiceTrack.insertTimeRange(timeRange, of: sourceAudio, at: cursor)
            }
            cursor = cursor + timeRange.duration
        }
        let total = cursor.seconds
        let format = VideoFormat(planValue: plan.format)
        let renderSize = format.renderSize

        // Picture: reframe each cut, then zoom/pan windows.
        let naturalSize = try await sourceVideo.load(.naturalSize)
        let preferred = try await sourceVideo.load(.preferredTransform)
        let layer = AVMutableVideoCompositionLayerInstruction(assetTrack: videoTrack)
        for event in transformEvents(plan: plan, ranges: ranges, cutStarts: cutStarts, naturalSize: naturalSize, preferred: preferred, renderSize: renderSize) {
            switch event {
            case .set(let at, let transform):
                layer.setTransform(transform, at: time(at))
            case .ramp(let from, let to, let start, let end):
                layer.setTransformRamp(fromStart: from, toEnd: to, timeRange: CMTimeRange(start: time(start), end: time(end)))
            }
        }
        let instruction = AVMutableVideoCompositionInstruction()
        instruction.timeRange = CMTimeRange(start: .zero, duration: cursor)
        instruction.layerInstructions = [layer]

        let videoComposition = AVMutableVideoComposition()
        videoComposition.renderSize = renderSize
        // Keep the source frame rate (24-60 fps) instead of forcing 30.
        let sourceFPS = Double(try await sourceVideo.load(.nominalFrameRate))
        let fps = sourceFPS.isFinite && sourceFPS > 1 ? min(max(sourceFPS.rounded(), 24), 60) : 30
        videoComposition.frameDuration = CMTime(value: 1, timescale: CMTimeScale(fps))
        videoComposition.instructions = [instruction]

        let (parent, videoLayer) = overlayLayers(plan: plan, ranges: ranges, renderSize: renderSize, assets: assets)
        videoComposition.animationTool = AVVideoCompositionCoreAnimationTool(postProcessingAsVideoLayer: videoLayer, in: parent)

        // Sound: voice, sound effects and BGM ducked under subtitles.
        var mixParameters: [AVMutableAudioMixInputParameters] = []
        if let voiceTrack {
            let params = AVMutableAudioMixInputParameters(track: voiceTrack)
            params.setVolume(1.0, at: .zero)
            mixParameters.append(params)
        }
        if plan.options.soundEffects {
            mixParameters += try await addSoundEffects(plan: plan, ranges: ranges, total: total, assets: assets, to: composition)
        }
        if plan.options.bgm, let bgmURL = assets.bgm {
            if let params = try await addBGM(url: bgmURL, total: total, plan: plan, ranges: ranges, hasVoice: voiceTrack != nil, to: composition) {
                mixParameters.append(params)
            }
        }
        let audioMix = AVMutableAudioMix()
        audioMix.inputParameters = mixParameters

        try? FileManager.default.removeItem(at: output)
        guard let export = AVAssetExportSession(asset: composition, presetName: AVAssetExportPresetHighestQuality) else {
            throw NueError.exportFailed("export session unavailable")
        }
        export.outputURL = output
        export.outputFileType = .mp4
        export.videoComposition = videoComposition
        export.audioMix = audioMix
        export.shouldOptimizeForNetworkUse = true
        await export.export()
        guard export.status == .completed else {
            throw NueError.exportFailed(export.error?.localizedDescription ?? "status \(export.status.rawValue)")
        }

        var subtitlesURL: URL?
        let srt = LocalPlanner.outputSRT(subtitles: plan.subtitles, ranges: ranges)
        if !srt.isEmpty {
            let url = output.deletingPathExtension().appendingPathExtension("srt")
            try srt.write(to: url, atomically: true, encoding: .utf8)
            subtitlesURL = url
        }
        return Output(video: output, subtitles: subtitlesURL, duration: total)
    }

    // MARK: - Picture

    enum TransformEvent {
        case set(Double, CGAffineTransform)
        case ramp(CGAffineTransform, CGAffineTransform, Double, Double)

        var start: Double {
            switch self {
            case .set(let t, _): return t
            case .ramp(_, _, let t, _): return t
            }
        }
    }

    /// Fills `renderSize` with the (oriented) source, centering the crop on the
    /// subject (`focus` = its horizontal center, 0...1); `zoom` scales around
    /// the frame center.
    static func fillTransform(naturalSize: CGSize, preferred: CGAffineTransform, renderSize: CGSize, focus: Double, zoom: Double = 1) -> CGAffineTransform {
        let oriented = CGRect(origin: .zero, size: naturalSize).applying(preferred)
        let display = CGSize(width: abs(oriented.width), height: abs(oriented.height))
        guard display.width > 0, display.height > 0 else { return preferred }
        let scale = max(renderSize.width / display.width, renderSize.height / display.height)
        let scaled = CGSize(width: display.width * scale, height: display.height * scale)
        let center = scaled.width * CGFloat(min(max(focus, 0), 1))
        let tx = -min(max(center - renderSize.width / 2, 0), scaled.width - renderSize.width)
        let ty = -(scaled.height - renderSize.height) / 2
        var transform = preferred
            .concatenating(CGAffineTransform(translationX: -oriented.minX, y: -oriented.minY))
            .concatenating(CGAffineTransform(scaleX: scale, y: scale))
            .concatenating(CGAffineTransform(translationX: tx, y: ty))
        if zoom != 1 {
            let cx = renderSize.width / 2, cy = renderSize.height / 2
            transform = transform
                .concatenating(CGAffineTransform(translationX: -cx, y: -cy))
                .concatenating(CGAffineTransform(scaleX: zoom, y: zoom))
                .concatenating(CGAffineTransform(translationX: cx, y: cy))
        }
        return transform
    }

    static func transformEvents(plan: EditPlan, ranges: [TimeSpan], cutStarts: [Double], naturalSize: CGSize, preferred: CGAffineTransform, renderSize: CGSize) -> [TransformEvent] {
        let focusByCut: [Double] = plan.cuts.filter { cut in
            guard let s = Timecode.parse(cut.startTime), let e = Timecode.parse(cut.endTime) else { return false }
            return e > s
        }.map { $0.focusPoint ?? 0.5 }
        func base(_ index: Int, zoom: Double = 1, focus: Double? = nil) -> CGAffineTransform {
            let f = focus ?? (index < focusByCut.count ? focusByCut[index] : 0.5)
            return fillTransform(naturalSize: naturalSize, preferred: preferred, renderSize: renderSize, focus: f, zoom: zoom)
        }
        var events: [TransformEvent] = []
        for (index, range) in ranges.enumerated() {
            let cutStart = cutStarts[index]
            events.append(.set(cutStart, base(index)))
            // A push-in ramps the whole cut; other transforms inside it would
            // overlap the ramp, which AVFoundation rejects.
            let pushed = plan.visualEffects.contains { effect in
                guard effect.type == "push_in", let s = Timecode.parse(effect.start) else { return false }
                return s < range.end && (effect.end.flatMap(Timecode.parse) ?? s) > range.start
            }
            for effect in plan.visualEffects where !pushed || effect.type == "push_in" {
                guard let s = Timecode.parse(effect.start) else { continue }
                let e = effect.end.flatMap(Timecode.parse) ?? s + 1.5
                let a = max(s, range.start), b = min(e, range.end)
                guard b - a > 0.05 else { continue }
                let oa = cutStart + a - range.start, ob = cutStart + b - range.start
                switch effect.type {
                case "zoom_in", "zoom_out", "punch_in":
                    let zoom = ["zoom_in": 1.25, "zoom_out": 1.1, "punch_in": 1.12][effect.type] ?? 1
                    events.append(.set(oa, base(index, zoom: zoom)))
                    events.append(.set(ob, base(index)))
                case "push_in":
                    events.append(.ramp(base(index), base(index, zoom: 1.06), oa, ob))
                case "pan_left", "pan_right":
                    let f = index < focusByCut.count ? focusByCut[index] : 0.5
                    let target = min(max(f + (effect.type == "pan_right" ? 0.35 : -0.35), 0), 1)
                    events.append(.ramp(base(index), base(index, focus: target), oa, ob))
                default:
                    continue
                }
            }
        }
        return events.sorted { $0.start < $1.start }
    }

    // MARK: - Overlays

    static func color(_ name: String?) -> CGColor {
        switch (name ?? "white").lowercased() {
        case "yellow": return CGColor(red: 1, green: 0.88, blue: 0.3, alpha: 1)
        case "cyan", "blue": return CGColor(red: 0.3, green: 0.91, blue: 1, alpha: 1)
        case "red": return CGColor(red: 1, green: 0.3, blue: 0.3, alpha: 1)
        case "pink": return CGColor(red: 1, green: 0.44, blue: 0.71, alpha: 1)
        case "green": return CGColor(red: 0.36, green: 1, blue: 0.55, alpha: 1)
        case "orange": return CGColor(red: 1, green: 0.65, blue: 0.24, alpha: 1)
        case "black": return CGColor(red: 0.07, green: 0.07, blue: 0.07, alpha: 1)
        default: return CGColor(red: 1, green: 1, blue: 1, alpha: 1)
        }
    }

    static func font(size: CGFloat) -> CTFont {
        for name in ["HiraginoSans-W7", "HiraginoSans-W6", "HiraKakuProN-W6"] {
            let font = CTFontCreateWithName(name as CFString, size, nil)
            if (CTFontCopyPostScriptName(font) as String) == name { return font }
        }
        return CTFontCreateUIFontForLanguage(.emphasizedSystem, size, nil)
            ?? CTFontCreateWithName("Helvetica-Bold" as CFString, size, nil)
    }

    /// Shows `layer` only between `start` and `end` (output seconds).
    static func show(_ layer: CALayer, from start: Double, to end: Double) {
        layer.opacity = 0
        let animation = CABasicAnimation(keyPath: "opacity")
        animation.fromValue = 1
        animation.toValue = 1
        animation.beginTime = start <= 0 ? AVCoreAnimationBeginTimeAtZero : start
        animation.duration = max(end - max(start, 0), 0.05)
        animation.isRemovedOnCompletion = false
        layer.add(animation, forKey: "visible")
    }

    /// Text layer positioned by its top edge (`topY`, measured from the top of the frame).
    static func textLayer(_ text: String, fontSize: CGFloat, color: CGColor, boxed: Bool, stroke: Double = -4, topY: CGFloat, renderSize: CGSize) -> CALayer {
        let attributes: [NSAttributedString.Key: Any] = [
            NSAttributedString.Key(kCTFontAttributeName as String): font(size: fontSize),
            NSAttributedString.Key(kCTForegroundColorAttributeName as String): color,
            NSAttributedString.Key(kCTStrokeColorAttributeName as String): CGColor(red: 0, green: 0, blue: 0, alpha: 1),
            NSAttributedString.Key(kCTStrokeWidthAttributeName as String): stroke,
        ]
        let string = NSAttributedString(string: text, attributes: attributes)
        let maxWidth = renderSize.width * 0.88
        let framesetter = CTFramesetterCreateWithAttributedString(string)
        let fit = CTFramesetterSuggestFrameSizeWithConstraints(framesetter, CFRange(location: 0, length: 0), nil, CGSize(width: maxWidth, height: .greatestFiniteMagnitude), nil)
        let padding = boxed ? fontSize * 0.25 : 0
        let size = CGSize(width: ceil(fit.width) + padding * 2 + 4, height: ceil(fit.height) + padding * 2)

        let container = CALayer()
        // Core Animation in AVFoundation uses a bottom-left origin.
        container.frame = CGRect(x: (renderSize.width - size.width) / 2, y: renderSize.height - topY - size.height, width: size.width, height: size.height)
        if boxed {
            container.backgroundColor = CGColor(red: 0, green: 0, blue: 0, alpha: 0.55)
            container.cornerRadius = fontSize * 0.15
        }
        let textLayer = CATextLayer()
        textLayer.string = string
        textLayer.isWrapped = true
        textLayer.alignmentMode = .center
        textLayer.contentsScale = 2
        textLayer.frame = CGRect(x: padding, y: padding, width: size.width - padding * 2, height: size.height - padding * 2)
        container.addSublayer(textLayer)
        return container
    }

    static func loadImage(_ url: URL) -> CGImage? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        return CGImageSourceCreateImageAtIndex(source, 0, nil)
    }

    /// Output-time windows of a source-time span, split across cuts.
    static func outputWindows(start: String, end: String?, defaultLength: Double, ranges: [TimeSpan], minLength: Double = 0.05) -> [(Double, Double)] {
        guard let s = Timecode.parse(start) else { return [] }
        let e = end.flatMap(Timecode.parse).flatMap { $0 > s ? $0 : nil } ?? s + defaultLength
        var windows: [(Double, Double)] = []
        var offset = 0.0
        for range in ranges {
            let a = max(s, range.start), b = min(e, range.end)
            if b - a >= minLength { windows.append((offset + a - range.start, offset + b - range.start)) }
            offset += range.duration
        }
        return windows
    }

    static func overlayLayers(plan: EditPlan, ranges: [TimeSpan], renderSize: CGSize, assets: AssetLibrary) -> (CALayer, CALayer) {
        let frame = CGRect(origin: .zero, size: renderSize)
        let parent = CALayer()
        parent.frame = frame
        let videoLayer = CALayer()
        videoLayer.frame = frame
        parent.addSublayer(videoLayer)

        let shortEdge = min(renderSize.width, renderSize.height)
        let landscape = renderSize.width > renderSize.height
        func topY(for position: String?, estimatedHeight: CGFloat) -> CGFloat {
            switch position ?? "bottom" {
            case "top": return renderSize.height * 0.13
            case "center", "middle": return (renderSize.height - estimatedHeight) / 2
            default: return renderSize.height * (landscape ? 0.80 : 0.66) - estimatedHeight
            }
        }

        var offset = 0.0
        for (index, range) in ranges.enumerated() {
            let cut = plan.cuts.filter { Timecode.parse($0.endTime) ?? 0 > Timecode.parse($0.startTime) ?? 0 }[safe: index]
            if let caption = cut?.caption, !caption.isEmpty {
                let size = shortEdge * 84 / 1080
                let layer = textLayer(caption, fontSize: size, color: color(cut?.captionStyle?.color), boxed: cut?.captionStyle?.box ?? false, topY: topY(for: cut?.captionStyle?.position, estimatedHeight: size * 1.4), renderSize: renderSize)
                show(layer, from: offset, to: offset + range.duration)
                parent.addSublayer(layer)
            }
            offset += range.duration
        }

        for telop in plan.telops {
            let size = shortEdge * (telop.style?.size == "large" ? 116 : 84) / 1080
            for (a, b) in outputWindows(start: telop.start, end: telop.end, defaultLength: 3, ranges: ranges) {
                let layer = textLayer(telop.text, fontSize: size, color: color(telop.style?.color), boxed: telop.style?.box ?? false, topY: topY(for: telop.style?.position, estimatedHeight: size * 1.4), renderSize: renderSize)
                show(layer, from: a, to: b)
                parent.addSublayer(layer)
            }
        }

        if plan.options.subtitles {
            // Bold white with a heavy outline, Japanese YouTube style.
            let size = shortEdge * 72 / 1080
            for line in plan.subtitles {
                for (a, b) in outputWindows(start: line.start, end: line.end, defaultLength: 2, ranges: ranges, minLength: 0.3) {
                    let bottom = renderSize.height * (landscape ? 0.95 : 0.84)
                    let layer = textLayer(line.text, fontSize: size, color: color("white"), boxed: false, stroke: -7, topY: bottom - size * 1.6, renderSize: renderSize)
                    show(layer, from: a, to: b)
                    parent.addSublayer(layer)
                }
            }
        }

        for overlay in plan.overlays {
            guard let url = assets.icon(overlay.asset), let image = loadImage(url) else { continue }
            let width = renderSize.width * CGFloat(min(max(overlay.scale ?? 0.18, 0.05), 0.8))
            let height = width * CGFloat(image.height) / CGFloat(max(image.width, 1))
            let margin = renderSize.width * 0.05
            let top = renderSize.height * 0.24
            let x: CGFloat
            let yTop: CGFloat
            switch overlay.position ?? "top_right" {
            case "top_left":
                x = margin
                yTop = top
            case "bottom_left":
                x = margin
                yTop = renderSize.height * 0.70 - height
            case "bottom_right":
                x = renderSize.width - margin - width
                yTop = renderSize.height * 0.70 - height
            case "center":
                x = (renderSize.width - width) / 2
                yTop = (renderSize.height - height) / 2
            default:
                x = renderSize.width - margin - width
                yTop = top
            }
            for (a, b) in outputWindows(start: overlay.start, end: overlay.end, defaultLength: 1, ranges: ranges) {
                let layer = CALayer()
                layer.contents = image
                layer.frame = CGRect(x: x, y: renderSize.height - yTop - height, width: width, height: height)
                show(layer, from: a, to: b)
                parent.addSublayer(layer)
            }
        }
        return (parent, videoLayer)
    }

    // MARK: - Sound

    static func addSoundEffects(plan: EditPlan, ranges: [TimeSpan], total: Double, assets: AssetLibrary, to composition: AVMutableComposition) async throws -> [AVMutableAudioMixInputParameters] {
        // Reuse a track until its previous effect has finished playing.
        var lanes: [(track: AVMutableCompositionTrack, freeAt: Double)] = []
        for event in plan.seEvents.prefix(48) {
            guard let source = Timecode.parse(event.timestamp),
                  let at = Timecode.sourceToOutput(source, ranges: ranges),
                  let url = assets.sound(event.type) else { continue }
            let soundAsset = AVURLAsset(url: url)
            guard let soundTrack = try await soundAsset.loadTracks(withMediaType: .audio).first else { continue }
            let length = min(try await soundAsset.load(.duration).seconds, total - at)
            guard length > 0.02 else { continue }
            let laneIndex: Int
            if let free = lanes.firstIndex(where: { $0.freeAt <= at }) {
                laneIndex = free
            } else if let track = composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid) {
                lanes.append((track, 0))
                laneIndex = lanes.count - 1
            } else {
                continue
            }
            try lanes[laneIndex].track.insertTimeRange(CMTimeRange(start: .zero, duration: time(length)), of: soundTrack, at: time(at))
            lanes[laneIndex].freeAt = at + length
        }
        return lanes.map { lane in
            let params = AVMutableAudioMixInputParameters(track: lane.track)
            params.setVolume(0.7, at: .zero)
            return params
        }
    }

    static func addBGM(url: URL, total: Double, plan: EditPlan, ranges: [TimeSpan], hasVoice: Bool, to composition: AVMutableComposition) async throws -> AVMutableAudioMixInputParameters? {
        let bgmAsset = AVURLAsset(url: url)
        guard let bgmTrack = try await bgmAsset.loadTracks(withMediaType: .audio).first,
              let track = composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid) else { return nil }
        let length = try await bgmAsset.load(.duration).seconds
        guard length > 0.5 else { return nil }
        var cursor = 0.0
        while cursor < total {
            let piece = min(length, total - cursor)
            try track.insertTimeRange(CMTimeRange(start: .zero, duration: time(piece)), of: bgmTrack, at: time(cursor))
            cursor += piece
        }
        let params = AVMutableAudioMixInputParameters(track: track)
        let normal: Float = hasVoice ? 0.22 : 0.4
        let ducked: Float = 0.08
        params.setVolume(normal, at: .zero)
        // Duck under speech (subtitle windows), fade out over the last 1.5 s.
        var lastEnd = 0.0
        for line in plan.subtitles {
            for (a, b) in outputWindows(start: line.start, end: line.end, defaultLength: 2, ranges: ranges, minLength: 0.5) where a >= lastEnd && b < total - 1.8 {
                // A ramp's end volume holds until the next ramp.
                params.setVolumeRamp(fromStartVolume: normal, toEndVolume: ducked, timeRange: CMTimeRange(start: time(a), duration: time(0.2)))
                params.setVolumeRamp(fromStartVolume: ducked, toEndVolume: normal, timeRange: CMTimeRange(start: time(b), duration: time(0.3)))
                lastEnd = b + 0.3
            }
        }
        let fadeStart = max(total - 1.5, lastEnd)
        if fadeStart < total {
            params.setVolumeRamp(fromStartVolume: normal, toEndVolume: 0, timeRange: CMTimeRange(start: time(fadeStart), end: time(total)))
        }
        return params
    }
}

extension Array {
    subscript(safe index: Int) -> Element? { indices.contains(index) ? self[index] : nil }
}
