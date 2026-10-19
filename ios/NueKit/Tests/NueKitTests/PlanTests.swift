import XCTest
@testable import NueKit

final class PlanTests: XCTestCase {
    func testTimecodeMatchesRustParser() {
        XCTAssertEqual(Timecode.parse("00:01:02"), 62)
        XCTAssertEqual(Timecode.parse("00:00:59.500"), 59.5)
        XCTAssertEqual(Timecode.parse("00:00:01,250"), 1.25)
        XCTAssertEqual(Timecode.parse("01:30"), 90)
        XCTAssertEqual(Timecode.parse("01:02:500")!, 62.5, accuracy: 1e-9)
        XCTAssertNil(Timecode.parse("abc"))
        XCTAssertEqual(Timecode.format(3723.456), "01:02:03.456")
    }

    func testDecodesPlanWrittenByRust() throws {
        let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("Fixtures/rust_plan.json")
        let plan = try JSONDecoder().decode(EditPlan.self, from: Data(contentsOf: url))
        XCTAssertEqual(plan.analyzer, "local")
        XCTAssertEqual(plan.cutRanges.count, 2)
        XCTAssertFalse(plan.subtitles.isEmpty)
        XCTAssertEqual(plan.telops.first?.style?.color, "yellow")
        XCTAssertTrue(plan.options.bgm)
        // Round trip keeps the schema's snake_case keys.
        let json = String(data: try plan.jsonData(), encoding: .utf8)!
        XCTAssertTrue(json.contains("\"start_time\""))
        XCTAssertTrue(json.contains("\"se_events\""))
        XCTAssertEqual(try JSONDecoder().decode(EditPlan.self, from: plan.jsonData()), plan)
    }

    func testToleratesNullsAndMissingFields() throws {
        let json = #"{"original_filename":"a.mp4","cuts":[{"start_time":"00:00:00","end_time":"00:00:05"}],"se_events":null,"editing_style":{"tempo":"fast","n":1},"thumbnail":{"timestamp":"00:00:01"}}"#
        let plan = try JSONDecoder().decode(EditPlan.self, from: Data(json.utf8))
        XCTAssertTrue(plan.seEvents.isEmpty)
        XCTAssertNil(plan.editingStyle)
        XCTAssertEqual(plan.thumbnail?.text, "")
        XCTAssertEqual(plan.outputDuration, 5)
    }

    func testSpeechRangesMatchRust() {
        let ranges = LocalPlanner.speechRanges(duration: 10, silences: [TimeSpan(start: 2, end: 4), TimeSpan(start: 9, end: .infinity)])
        XCTAssertEqual(ranges.count, 2)
        XCTAssertEqual(ranges[0].end, 2.15, accuracy: 1e-9)
        XCTAssertEqual(ranges[1].start, 3.85, accuracy: 1e-9)
        XCTAssertEqual(ranges[1].end, 9.15, accuracy: 1e-9)
    }

    func testLoudnessPeaksMatchRust() {
        var curve = (0..<80).map { LoudnessPoint(time: Double($0) * 0.25, db: -30) }
        curve[20].db = -10
        curve[22].db = -12
        curve[60].db = -8
        XCTAssertEqual(LocalPlanner.loudnessPeaks(curve: curve, minGap: 6), [5, 15])
    }

    func testSilencesFromCurve() {
        let curve = (0..<20).map { LoudnessPoint(time: Double($0) * 0.25, db: (4..<10).contains($0) ? -60 : -20) }
        XCTAssertEqual(LocalPlanner.silences(curve: curve, window: 0.25), [TimeSpan(start: 1, end: 2.5)])
    }

    func testRmsOfFullScaleSine() {
        let samples = (0..<16000).map { Float(sin(Double($0) * 2 * .pi * 440 / 16000)) }
        let curve = LocalPlanner.rmsCurve(samples: samples, sampleRate: 16000, window: 0.25)
        XCTAssertEqual(curve.count, 4)
        XCTAssertEqual(curve[0].db, -3.01, accuracy: 0.05)
    }

    func testGroupsWordsIntoSubtitleLines() {
        let words = [
            TimedWord(text: "Hello", start: 0, end: 0.4), TimedWord(text: "world.", start: 0.5, end: 0.9),
            TimedWord(text: "今日は", start: 2.0, end: 2.4), TimedWord(text: "晴れ", start: 2.4, end: 2.8),
        ]
        let lines = LocalPlanner.groupSubtitles(words)
        XCTAssertEqual(lines.map(\.text), ["Hello world.", "今日は晴れ"])
        XCTAssertEqual(lines[1].start, "00:00:02.000")
    }

    func testOutputSrtRemapsAcrossCuts() {
        let subs = [TimedText(start: 0, end: 2.5, text: "a"), TimedText(start: 5, end: 7, text: "b")]
        let srt = LocalPlanner.outputSRT(subtitles: subs, ranges: [TimeSpan(start: 0, end: 1), TimeSpan(start: 5, end: 7)])
        XCTAssertTrue(srt.contains("00:00:00,000 --> 00:00:01,000\na"))
        XCTAssertTrue(srt.contains("00:00:01,000 --> 00:00:03,000\nb"))
    }

    func testBuildsPlanLikeRust() {
        var curve = (0..<80).map { LoudnessPoint(time: Double($0) * 0.25, db: -25) }
        for i in 30..<40 { curve[i].db = -70 } // 7.5 s - 10 s silent
        curve[60].db = -5
        let plan = LocalPlanner.buildPlan(fileName: "a.mov", duration: 20, hasAudio: true, loudness: curve, window: 0.25, subtitles: [], jumpCut: true)
        XCTAssertEqual(plan.cutRanges.count, 2)
        XCTAssertEqual(plan.seEvents.map(\.type), ["impact"])
        XCTAssertEqual(plan.overlays.first?.asset, "exclaim")
    }

    func testResolvesSoundAliasesLikeRust() {
        let data = URL(fileURLWithPath: #filePath).appendingPathComponent("../../../../../data").standardizedFileURL
        let library = AssetLibrary(soundDirectory: data.appendingPathComponent("se"), iconDirectory: data.appendingPathComponent("icons"), bgm: nil)
        XCTAssertEqual(library.sound("funny")?.lastPathComponent, "boing.wav")
        XCTAssertEqual(library.sound("ding")?.lastPathComponent, "ding.wav")
        XCTAssertEqual(library.sound("unknown")?.lastPathComponent, "impact.wav")
        XCTAssertNotNil(library.icon("heart"))
        XCTAssertTrue(library.iconNames.contains("fire"))
    }
}
