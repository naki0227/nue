import AVKit
import NueKit
import Photos
import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

/// A picked video copied into the app's temporary directory.
struct PickedMovie: Transferable {
    let url: URL

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(contentType: .movie) { movie in
            SentTransferredFile(movie.url)
        } importing: { received in
            let target = FileManager.default.temporaryDirectory
                .appendingPathComponent("source-\(UUID().uuidString).\(received.file.pathExtension.isEmpty ? "mov" : received.file.pathExtension)")
            try FileManager.default.copyItem(at: received.file, to: target)
            return PickedMovie(url: target)
        }
    }
}

@MainActor
final class EditorModel: ObservableObject {
    enum Phase: Equatable {
        case idle, working(String), done, failed(String)
    }

    @Published var phase: Phase = .idle
    @Published var source: URL?
    @Published var plan: EditPlan?
    @Published var output: Renderer.Output?
    @Published var options = AnalyzeOptions()
    @Published var savedToPhotos = false

    let assets = AssetLibrary.bundled()

    var workDirectory: URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("nue", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    func load(_ item: PhotosPickerItem?) async {
        guard let item else { return }
        phase = .working("動画を読み込み中…")
        do {
            guard let movie = try await item.loadTransferable(type: PickedMovie.self) else {
                phase = .failed("動画を読み込めませんでした")
                return
            }
            source = movie.url
            plan = nil
            output = nil
            phase = .idle
        } catch {
            phase = .failed(error.localizedDescription)
        }
    }

    func analyzeAndRender() async {
        guard let source else { return }
        do {
            let options = self.options
            let plan = try await NuePipeline.analyze(source: source, options: options, workDirectory: workDirectory) { stage in
                Task { @MainActor in
                    switch stage {
                    case .readingAudio: self.phase = .working("音声を解析中…")
                    case .transcribing: self.phase = .working("字幕を作成中（端末内）…")
                    case .planning: self.phase = .working("編集を組み立て中…")
                    case .rendering: self.phase = .working("書き出し中…")
                    }
                }
            }
            self.plan = plan
            await render()
        } catch {
            phase = .failed(error.localizedDescription)
        }
    }

    func render() async {
        guard let source, var plan else { return }
        plan.format = options.format.rawValue
        phase = .working("書き出し中…")
        savedToPhotos = false
        do {
            let target = workDirectory.appendingPathComponent("nue-\(Int(Date().timeIntervalSince1970)).mp4")
            output = try await Renderer.render(plan: plan, source: source, assets: assets, output: target)
            phase = .done
        } catch {
            phase = .failed(error.localizedDescription)
        }
    }

    var planFile: URL? {
        guard let plan, let data = try? plan.jsonData() else { return nil }
        let url = workDirectory.appendingPathComponent("plan.json")
        try? data.write(to: url)
        return url
    }

    func saveToPhotos() async {
        guard let video = output?.video else { return }
        let status = await PHPhotoLibrary.requestAuthorization(for: .addOnly)
        guard status == .authorized || status == .limited else {
            phase = .failed("写真への保存が許可されていません")
            return
        }
        do {
            try await PHPhotoLibrary.shared().performChanges {
                PHAssetChangeRequest.creationRequestForAssetFromVideo(atFileURL: video)
            }
            savedToPhotos = true
        } catch {
            phase = .failed(error.localizedDescription)
        }
    }
}

struct ContentView: View {
    @StateObject private var model = EditorModel()
    @State private var pickerItem: PhotosPickerItem?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    PhotosPicker(selection: $pickerItem, matching: .videos) {
                        Label(model.source == nil ? "動画を選ぶ" : "別の動画を選ぶ", systemImage: "film")
                    }
                    if let source = model.source {
                        Text(source.lastPathComponent).font(.caption).foregroundStyle(.secondary)
                    }
                }

                Section("設定") {
                    Picker("形式", selection: $model.options.format) {
                        Text("縦 9:16").tag(VideoFormat.vertical)
                        Text("横 16:9").tag(VideoFormat.horizontal)
                        Text("正方形").tag(VideoFormat.square)
                    }
                    Toggle("字幕（端末内で文字起こし）", isOn: $model.options.subtitles)
                    Toggle("無音カット", isOn: $model.options.jumpCut)
                    Toggle("効果音", isOn: $model.options.soundEffects)
                    Toggle("BGM", isOn: $model.options.bgm)
                    Toggle("Apple Intelligenceでテロップ", isOn: $model.options.appleIntelligence)
                        .disabled(!TelopWriter.isAppleIntelligenceAvailable)
                    if !TelopWriter.isAppleIntelligenceAvailable {
                        Text("この端末ではApple Intelligenceが使えないため、ルールでテロップを作ります。")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }

                Section {
                    Button {
                        Task { await model.analyzeAndRender() }
                    } label: {
                        Label("自動で編集する", systemImage: "wand.and.stars")
                    }
                    .disabled(model.source == nil || isWorking)
                    switch model.phase {
                    case .working(let message):
                        HStack { ProgressView(); Text(message) }
                    case .failed(let message):
                        Text(message).foregroundStyle(.red)
                    default:
                        EmptyView()
                    }
                }

                if let output = model.output, model.phase == .done {
                    Section("完成") {
                        VideoPlayer(player: AVPlayer(url: output.video))
                            .frame(height: 420)
                        ShareLink(item: output.video) { Label("動画を共有", systemImage: "square.and.arrow.up") }
                        Button {
                            Task { await model.saveToPhotos() }
                        } label: {
                            Label(model.savedToPhotos ? "写真に保存しました" : "写真に保存", systemImage: "photo.on.rectangle")
                        }
                        if let srt = output.subtitles {
                            ShareLink(item: srt) { Label("字幕 (.srt)", systemImage: "captions.bubble") }
                        }
                        if let planFile = model.planFile {
                            ShareLink(item: planFile) { Label("編集指示書 (.json)", systemImage: "doc.text") }
                        }
                    }
                }

                if model.plan != nil {
                    TelopEditor(model: model)
                }
            }
            .navigationTitle("Nue")
        }
        .onChange(of: pickerItem) { _, item in
            Task { await model.load(item) }
        }
    }

    private var isWorking: Bool {
        if case .working = model.phase { return true }
        return false
    }
}

/// Quick fixes before re-rendering: telop text and which cuts to keep.
struct TelopEditor: View {
    @ObservedObject var model: EditorModel

    var body: some View {
        Section("手直し") {
            if let plan = model.plan {
                ForEach(plan.telops.indices, id: \.self) { index in
                    TextField("テロップ", text: Binding(
                        get: { model.plan?.telops[safeIndex: index]?.text ?? "" },
                        set: { model.plan?.telops[index].text = $0 }))
                }
                Button("テロップを追加") {
                    model.plan?.telops.append(TimedText(start: 0, end: 3, text: "テロップ", style: CaptionStyle(color: "yellow", position: "top", box: true)))
                }
                ForEach(plan.cuts.indices, id: \.self) { index in
                    HStack {
                        Text("カット \(index + 1)")
                        Spacer()
                        Text("\(plan.cuts[index].startTime.prefix(8)) – \(plan.cuts[index].endTime.prefix(8))")
                            .font(.caption.monospaced()).foregroundStyle(.secondary)
                    }
                    .swipeActions {
                        if plan.cuts.count > 1 {
                            Button("削除", role: .destructive) { model.plan?.cuts.remove(at: index) }
                        }
                    }
                }
                Button {
                    Task { await model.render() }
                } label: {
                    Label("書き出し直す", systemImage: "arrow.clockwise")
                }
            }
        }
    }
}

extension Array {
    subscript(safeIndex index: Int) -> Element? { indices.contains(index) ? self[index] : nil }
}
