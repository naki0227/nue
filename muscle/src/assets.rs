//! Lookup of bundled fonts, sound effects, BGM and icon assets under `data/`.

use crate::jobs::{data_path, log_json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const FONT_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Black.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Bold.ttc",
    "/System/Library/Fonts/ヒラギノ角ゴシック W6.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
];

/// Font used for captions, subtitles and thumbnails. `NUE_FONT` wins, then a
/// bundled `data/fonts/*.ttf|otf|ttc`, then well-known system fonts.
pub fn font_path() -> &'static str {
    static FONT: OnceLock<String> = OnceLock::new();
    FONT.get_or_init(|| {
        if let Ok(font) = env::var("NUE_FONT") {
            return font;
        }
        let bundled = list_files(&data_path("fonts"), &["ttf", "otf", "ttc"]);
        if let Some((_, path)) = bundled.into_iter().next() {
            return path.to_string_lossy().to_string();
        }
        if let Some(found) = FONT_CANDIDATES.iter().find(|p| Path::new(p).exists()) {
            return found.to_string();
        }
        let fc = Command::new("fc-match")
            .args(["-f", "%{file}", "sans:bold:lang=ja"])
            .output()
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .filter(|path| !path.is_empty());
        fc.unwrap_or_else(|| {
            log_json(
                "WARN",
                "No font found; set NUE_FONT",
                Some("font_missing"),
                None,
            );
            FONT_CANDIDATES[0].to_string()
        })
    })
}

fn list_files(dir: &Path, extensions: &[&str]) -> Vec<(String, PathBuf)> {
    let mut files: Vec<(String, PathBuf)> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|e| e.path()))
                .filter(|path| {
                    path.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| extensions.contains(&e.to_lowercase().as_str()))
                        .unwrap_or(false)
                })
                .filter_map(|path| {
                    let stem = path.file_stem()?.to_str()?.to_string();
                    Some((stem, path))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// Sound-effect names available in `data/se` (file stems).
pub fn sound_effect_names() -> Vec<String> {
    list_files(&data_path("se"), &["wav", "mp3", "m4a", "ogg"])
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// Icon names available in `data/icons` (file stems).
pub fn icon_names() -> Vec<String> {
    list_files(&data_path("icons"), &["png"])
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// Resolves a sound-effect type to a file. Exact stem match first, then
/// keyword aliases, then the default "impact" sound.
pub fn sound_effect(kind: &str) -> Option<PathBuf> {
    let files = list_files(&data_path("se"), &["wav", "mp3", "m4a", "ogg"]);
    let kind = kind.to_lowercase();
    let find = |name: &str| {
        files
            .iter()
            .find(|(stem, _)| stem.to_lowercase() == name)
            .map(|(_, path)| path.clone())
    };
    if let Some(path) = find(&kind) {
        return Some(path);
    }
    let alias = [
        (&["funny", "laugh", "boing", "comic"][..], "boing"),
        (&["whoosh", "swoosh", "transition", "swipe"][..], "whoosh"),
        (&["correct", "ding", "bell", "success", "chime"][..], "ding"),
        (&["pop", "appear", "click", "bubble"][..], "pop"),
        (&["fail", "wrong", "sad", "buzz"][..], "buzzer"),
        (&["shine", "sparkle", "magic", "kira"][..], "sparkle"),
        (&["impact", "serious", "hit", "don", "shock"][..], "impact"),
    ]
    .iter()
    .find(|(keywords, _)| keywords.iter().any(|k| kind.contains(k)))
    .map(|(_, target)| *target)
    .unwrap_or("impact");
    find(alias)
        .or_else(|| find("synth_don"))
        .or_else(|| files.first().map(|(_, path)| path.clone()))
}

pub fn icon(name: &str) -> Option<PathBuf> {
    let path = data_path("icons").join(format!("{}.png", name));
    path.exists().then_some(path)
}

/// BGM for the plan: an explicit path if it exists, otherwise a track in
/// `data/bgm` whose name contains the mood, otherwise `default_bgm.mp3`.
pub fn bgm(explicit: Option<&str>, mood: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = explicit.map(PathBuf::from) {
        if path.exists() {
            return Some(path);
        }
        let in_dir = data_path("bgm").join(&path);
        if in_dir.exists() {
            return Some(in_dir);
        }
    }
    let tracks = list_files(&data_path("bgm"), &["mp3", "wav", "m4a", "ogg"]);
    if let Some(mood) = mood.map(str::to_lowercase) {
        if let Some((_, path)) = tracks
            .iter()
            .find(|(name, _)| name.to_lowercase().contains(&mood))
        {
            return Some(path.clone());
        }
    }
    tracks
        .iter()
        .find(|(name, _)| name == "default_bgm")
        .or_else(|| tracks.first())
        .map(|(_, path)| path.clone())
}
