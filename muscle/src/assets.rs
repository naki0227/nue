//! Lookup of bundled fonts, sound effects, BGM and icon assets under `data/`.

use crate::jobs::data_path;
use std::fs;
use std::path::{Path, PathBuf};

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
