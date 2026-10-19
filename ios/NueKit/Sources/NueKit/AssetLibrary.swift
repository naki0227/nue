import Foundation

/// Sound effects, icons and BGM. File names are the names used in edit plans,
/// shared with the Rust renderer's data/se and data/icons.
public struct AssetLibrary: Sendable {
    public var soundDirectory: URL?
    public var iconDirectory: URL?
    public var bgm: URL?

    public init(soundDirectory: URL?, iconDirectory: URL?, bgm: URL?) {
        self.soundDirectory = soundDirectory
        self.iconDirectory = iconDirectory
        self.bgm = bgm
    }

    /// Assets bundled in the app as folder references `se`, `icons`, `bgm`.
    public static func bundled(_ bundle: Bundle = .main) -> AssetLibrary {
        let root = bundle.resourceURL
        let bgmDir = root?.appendingPathComponent("bgm")
        let bgm = bgmDir.flatMap { files(in: $0, extensions: ["mp3", "m4a", "wav"]).first }
        return AssetLibrary(
            soundDirectory: root?.appendingPathComponent("se"),
            iconDirectory: root?.appendingPathComponent("icons"),
            bgm: bgm)
    }

    static func files(in directory: URL, extensions: [String]) -> [URL] {
        let items = (try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)) ?? []
        return items.filter { extensions.contains($0.pathExtension.lowercased()) }
            .sorted { $0.lastPathComponent < $1.lastPathComponent }
    }

    public var soundNames: [String] {
        soundDirectory.map { Self.files(in: $0, extensions: ["wav", "mp3", "m4a"]).map { $0.deletingPathExtension().lastPathComponent } } ?? []
    }

    public var iconNames: [String] {
        iconDirectory.map { Self.files(in: $0, extensions: ["png"]).map { $0.deletingPathExtension().lastPathComponent } } ?? []
    }

    private static let aliases: [([String], String)] = [
        (["funny", "laugh", "boing", "comic"], "boing"),
        (["whoosh", "swoosh", "transition", "swipe"], "whoosh"),
        (["correct", "ding", "bell", "success", "chime"], "ding"),
        (["pop", "appear", "click", "bubble"], "pop"),
        (["fail", "wrong", "sad", "buzz"], "buzzer"),
        (["shine", "sparkle", "magic", "kira"], "sparkle"),
        (["impact", "serious", "hit", "don", "shock"], "impact"),
    ]

    /// Same resolution order as the Rust renderer: exact name, alias, "impact".
    public func sound(_ kind: String) -> URL? {
        guard let dir = soundDirectory else { return nil }
        let files = Self.files(in: dir, extensions: ["wav", "mp3", "m4a"])
        func find(_ name: String) -> URL? {
            files.first { $0.deletingPathExtension().lastPathComponent.lowercased() == name }
        }
        let kind = kind.lowercased()
        if let exact = find(kind) { return exact }
        let target = Self.aliases.first { keywords, _ in keywords.contains { kind.contains($0) } }?.1 ?? "impact"
        return find(target) ?? find("synth_don") ?? files.first
    }

    public func icon(_ name: String) -> URL? {
        guard let url = iconDirectory?.appendingPathComponent("\(name).png"),
              FileManager.default.fileExists(atPath: url.path) else { return nil }
        return url
    }
}
