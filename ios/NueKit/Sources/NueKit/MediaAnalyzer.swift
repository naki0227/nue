import AVFoundation
import Foundation

public struct MediaSignals: Sendable {
    public var duration: Double
    public var hasAudio: Bool
    public var loudness: [LoudnessPoint]
    public var window: Double
}

public enum NueError: LocalizedError {
    case noVideoTrack
    case noUsableCuts
    case exportFailed(String)

    public var errorDescription: String? {
        switch self {
        case .noVideoTrack: return "The file has no video track."
        case .noUsableCuts: return "The edit plan has no usable cuts."
        case .exportFailed(let reason): return "Export failed: \(reason)"
        }
    }
}

public enum MediaAnalyzer {
    /// Decodes the audio track to 16 kHz mono and measures loudness.
    public static func analyze(url: URL, window: Double = 0.25) async throws -> MediaSignals {
        let asset = AVURLAsset(url: url)
        let duration = try await asset.load(.duration).seconds
        guard let track = try await asset.loadTracks(withMediaType: .audio).first else {
            return MediaSignals(duration: duration, hasAudio: false, loudness: [], window: window)
        }
        let sampleRate = 16_000.0
        let reader = try AVAssetReader(asset: asset)
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVLinearPCMBitDepthKey: 32,
            AVLinearPCMIsFloatKey: true,
            AVLinearPCMIsBigEndianKey: false,
            AVLinearPCMIsNonInterleaved: false,
            AVNumberOfChannelsKey: 1,
            AVSampleRateKey: sampleRate,
        ])
        reader.add(output)
        reader.startReading()
        var samples: [Float] = []
        while let buffer = output.copyNextSampleBuffer() {
            guard let block = CMSampleBufferGetDataBuffer(buffer) else { continue }
            let length = CMBlockBufferGetDataLength(block)
            var chunk = [Float](repeating: 0, count: length / MemoryLayout<Float>.size)
            chunk.withUnsafeMutableBytes { raw in
                _ = CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: raw.count, destination: raw.baseAddress!)
            }
            samples.append(contentsOf: chunk)
        }
        let curve = LocalPlanner.rmsCurve(samples: samples, sampleRate: sampleRate, window: window)
        return MediaSignals(duration: duration, hasAudio: true, loudness: curve, window: window)
    }

    /// Extracts the audio track to an .m4a file for speech recognition.
    public static func exportAudio(from url: URL, to output: URL) async throws {
        let asset = AVURLAsset(url: url)
        guard let session = AVAssetExportSession(asset: asset, presetName: AVAssetExportPresetAppleM4A) else {
            throw NueError.exportFailed("audio export unavailable")
        }
        try? FileManager.default.removeItem(at: output)
        session.outputURL = output
        session.outputFileType = .m4a
        await session.export()
        if session.status != .completed {
            throw NueError.exportFailed(session.error?.localizedDescription ?? "audio export")
        }
    }
}
