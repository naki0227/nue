import AVFoundation
import CoreGraphics
import XCTest
@testable import NueKit

final class RendererTests: XCTestCase {
    /// Writes a solid gray H.264 clip (no audio).
    func makeClip(seconds: Int, size: CGSize, to url: URL) async throws {
        try? FileManager.default.removeItem(at: url)
        let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
        let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: Int(size.width),
            AVVideoHeightKey: Int(size.height),
        ])
        let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
            kCVPixelBufferWidthKey as String: Int(size.width),
            kCVPixelBufferHeightKey as String: Int(size.height),
        ])
        writer.add(input)
        XCTAssertTrue(writer.startWriting())
        writer.startSession(atSourceTime: .zero)
        let fps: Int32 = 30
        for frame in 0..<(seconds * Int(fps)) {
            while !input.isReadyForMoreMediaData { try await Task.sleep(nanoseconds: 2_000_000) }
            var buffer: CVPixelBuffer?
            CVPixelBufferPoolCreatePixelBuffer(nil, adaptor.pixelBufferPool!, &buffer)
            let pixels = buffer!
            CVPixelBufferLockBaseAddress(pixels, [])
            memset(CVPixelBufferGetBaseAddress(pixels), 0x80, CVPixelBufferGetDataSize(pixels))
            CVPixelBufferUnlockBaseAddress(pixels, [])
            adaptor.append(pixels, withPresentationTime: CMTime(value: CMTimeValue(frame), timescale: fps))
        }
        input.markAsFinished()
        await writer.finishWriting()
        XCTAssertEqual(writer.status, .completed, writer.error?.localizedDescription ?? "")
    }

    func rgba(of image: CGImage, x: Int, yFromTop: Int) -> (r: UInt8, g: UInt8, b: UInt8) {
        let width = image.width, height = image.height
        var data = [UInt8](repeating: 0, count: width * height * 4)
        let context = CGContext(data: &data, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        let i = (yFromTop * width + x) * 4
        return (data[i], data[i + 1], data[i + 2])
    }

    func testRendersCutsFormatAndIconPosition() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("nuekit-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let source = dir.appendingPathComponent("clip.mp4")
        try await makeClip(seconds: 4, size: CGSize(width: 640, height: 360), to: source)

        var plan = EditPlan(originalFilename: "clip.mp4", analyzer: "test", cuts: [Cut(start: 0, end: 1.5), Cut(start: 2.5, end: 4)])
        plan.format = "vertical"
        plan.overlays = [Overlay(asset: "heart", start: 0, end: 4, position: "top_right", scale: 0.25)]
        plan.telops = [TimedText(start: 0.2, end: 1.2, text: "テスト", style: CaptionStyle(color: "yellow", position: "top", box: true))]
        plan.subtitles = [TimedText(start: 2.6, end: 3.6, text: "subtitle line")]
        plan.seEvents = [SoundEvent(at: 0.5, type: "pop")]

        let data = URL(fileURLWithPath: #filePath).appendingPathComponent("../../../../../data").standardizedFileURL
        let assets = AssetLibrary(soundDirectory: data.appendingPathComponent("se"), iconDirectory: data.appendingPathComponent("icons"), bgm: data.appendingPathComponent("bgm/default_bgm.mp3"))
        let out = try await Renderer.render(plan: plan, source: source, assets: assets, output: dir.appendingPathComponent("out.mp4"))

        XCTAssertEqual(out.duration, 3.0, accuracy: 0.05)
        XCTAssertNotNil(out.subtitles)
        let rendered = AVURLAsset(url: out.video)
        let duration = try await rendered.load(.duration).seconds
        XCTAssertEqual(duration, 3.0, accuracy: 0.15)
        let track = try await rendered.loadTracks(withMediaType: .video).first!
        let size = try await track.load(.naturalSize)
        XCTAssertEqual(size, CGSize(width: 1080, height: 1920))
        let audioTracks = try await rendered.loadTracks(withMediaType: .audio)
        XCTAssertFalse(audioTracks.isEmpty, "BGM/SE should produce audio")

        let generator = AVAssetImageGenerator(asset: rendered)
        generator.requestedTimeToleranceBefore = .zero
        generator.requestedTimeToleranceAfter = .zero
        let (frame, _) = try await generator.image(at: CMTime(seconds: 0.5, preferredTimescale: 600))
        // Heart icon: 270 px wide at x = 1026 - 270, top = 0.24 * 1920 ≈ 461. Its center is pink.
        let icon = rgba(of: frame, x: 891, yFromTop: 461 + 120)
        XCTAssertGreaterThan(icon.r, 200, "icon not at the top right: \(icon)")
        XCTAssertLessThan(icon.g, 150, "icon not at the top right: \(icon)")
        // The mirrored position (if the overlay were upside down) must stay video gray.
        let mirrored = rgba(of: frame, x: 891, yFromTop: 1920 - 461 - 120)
        XCTAssertLessThan(abs(Int(mirrored.r) - Int(mirrored.g)), 30, "overlay appears flipped: \(mirrored)")
    }

    func testPipelineAnalyzesAndRendersSilentClip() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("nuekit-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let source = dir.appendingPathComponent("silent.mp4")
        try await makeClip(seconds: 3, size: CGSize(width: 360, height: 640), to: source)

        var options = AnalyzeOptions()
        options.format = .square
        options.bgm = false
        let plan = try await NuePipeline.analyze(source: source, options: options, workDirectory: dir)
        XCTAssertEqual(plan.format, "square")
        XCTAssertEqual(plan.outputDuration, 3, accuracy: 0.1)
        let out = try await Renderer.render(plan: plan, source: source, assets: AssetLibrary(soundDirectory: nil, iconDirectory: nil, bgm: nil), output: dir.appendingPathComponent("out.mp4"))
        let track = try await AVURLAsset(url: out.video).loadTracks(withMediaType: .video).first!
        let size = try await track.load(.naturalSize)
        XCTAssertEqual(size, CGSize(width: 1080, height: 1080))
    }

    func testFillTransformCentersAndFocuses() {
        let render = CGSize(width: 1080, height: 1920)
        let t = Renderer.fillTransform(naturalSize: CGSize(width: 1920, height: 1080), preferred: .identity, renderSize: render, focus: 0.5)
        let rect = CGRect(x: 0, y: 0, width: 1920, height: 1080).applying(t)
        XCTAssertEqual(rect.height, 1920, accuracy: 0.5)
        XCTAssertEqual(rect.midX, 540, accuracy: 0.5)
        let left = CGRect(x: 0, y: 0, width: 1920, height: 1080).applying(
            Renderer.fillTransform(naturalSize: CGSize(width: 1920, height: 1080), preferred: .identity, renderSize: render, focus: 0))
        XCTAssertEqual(left.minX, 0, accuracy: 0.5)
        // Subject at 75% of the width: the crop is centered on it.
        let right = CGRect(x: 0, y: 0, width: 1920, height: 1080).applying(
            Renderer.fillTransform(naturalSize: CGSize(width: 1920, height: 1080), preferred: .identity, renderSize: render, focus: 0.75))
        XCTAssertEqual(-right.minX + render.width / 2, right.width * 0.75, accuracy: 0.5)
    }
}
