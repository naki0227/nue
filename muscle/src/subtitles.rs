//! Speech-to-subtitle generation (local whisper.cpp) and SRT read/write.

use crate::jobs::{data_path, log_json};
use crate::media;
use crate::plan::{format_time, parse_time, source_to_output, TimedText};
use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Locates a whisper.cpp model: `NUE_WHISPER_MODEL`, else the first
/// `ggml-*.bin` in `data/models`.
fn whisper_model() -> Option<PathBuf> {
    if let Ok(path) = env::var("NUE_WHISPER_MODEL") {
        let path = PathBuf::from(path);
        return path.exists().then_some(path);
    }
    let mut models: Vec<PathBuf> = fs::read_dir(data_path("models"))
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("ggml-") && n.ends_with(".bin"))
                .unwrap_or(false)
        })
        .collect();
    models.sort();
    models.into_iter().next()
}

fn whisper_binary() -> String {
    env::var("NUE_WHISPER_BIN").unwrap_or_else(|_| "whisper-cli".to_string())
}

/// Transcribes speech into subtitle lines. Sources, in order: a sidecar
/// `<video>.srt` next to the input, then whisper.cpp. Returns an empty list
/// when neither is available so rendering can continue without subtitles.
pub fn transcribe(input: &Path, work_dir: &Path, has_audio: bool) -> Vec<TimedText> {
    for sidecar in [
        input.with_extension("srt"),
        PathBuf::from(format!("{}.srt", input.display())),
    ] {
        if let Ok(content) = fs::read_to_string(&sidecar) {
            log_json(
                "INFO",
                "Using sidecar subtitles",
                Some("subtitles_sidecar"),
                sidecar.to_str(),
            );
            return parse_srt(&content);
        }
    }
    if !has_audio {
        return Vec::new();
    }
    let Some(model) = whisper_model() else {
        log_json(
            "INFO",
            "No whisper model found (set NUE_WHISPER_MODEL or put ggml-*.bin in data/models); skipping subtitles",
            Some("subtitles_skipped"),
            None,
        );
        return Vec::new();
    };
    match run_whisper(input, work_dir, &model) {
        Ok(lines) => {
            log_json(
                "INFO",
                &format!("Transcribed {} subtitle lines", lines.len()),
                Some("subtitles_done"),
                None,
            );
            lines
        }
        Err(error) => {
            log_json(
                "WARN",
                &format!("Transcription failed: {}", error),
                Some("subtitles_failed"),
                None,
            );
            Vec::new()
        }
    }
}

fn run_whisper(input: &Path, work_dir: &Path, model: &Path) -> Result<Vec<TimedText>> {
    fs::create_dir_all(work_dir)?;
    let wav = work_dir.join("speech.wav");
    media::extract_speech_wav(input, &wav)?;
    let out_base = work_dir.join("speech");
    let language = env::var("NUE_LANGUAGE").unwrap_or_else(|_| "auto".to_string());
    let output = Command::new(whisper_binary())
        .arg("-m")
        .arg(model)
        .arg("-f")
        .arg(&wav)
        .args([
            "-l",
            &language,
            "-osrt",
            "-np",
            "--max-len",
            "32",
            "--split-on-word",
        ])
        .arg("-of")
        .arg(&out_base)
        .output()
        .with_context(|| format!("Failed to run {}", whisper_binary()))?;
    if !output.status.success() {
        bail!(
            "whisper failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let srt = fs::read_to_string(out_base.with_extension("srt"))?;
    let _ = fs::remove_file(&wav);
    Ok(parse_srt(&srt))
}

pub fn parse_srt(content: &str) -> Vec<TimedText> {
    let normalized = content.replace("\r\n", "\n");
    normalized
        .split("\n\n")
        .filter_map(|block| {
            let mut lines = block.lines().map(str::trim).filter(|l| !l.is_empty());
            let mut line = lines.next()?;
            if !line.contains("-->") {
                line = lines.next()?;
            }
            let (start, end) = line.split_once("-->")?;
            let start = parse_time(start.trim()).ok()?;
            let end = parse_time(end.split_whitespace().next()?).ok()?;
            let text = lines.collect::<Vec<_>>().join(" ").trim().to_string();
            (!text.is_empty() && end > start && !is_noise_marker(&text)).then(|| TimedText {
                start: format_time(start),
                end: format_time(end),
                text,
                style: None,
            })
        })
        .collect()
}

/// whisper emits markers such as "[Music]" or "(拍手)" for non-speech.
fn is_noise_marker(text: &str) -> bool {
    let t = text.trim();
    (t.starts_with('[') && t.ends_with(']')) || (t.starts_with('(') && t.ends_with(')'))
}

fn srt_time(seconds: f64) -> String {
    format_time(seconds).replace('.', ",")
}

/// Writes subtitles on the edited timeline, splitting lines that straddle cuts.
pub fn to_output_srt(subtitles: &[TimedText], ranges: &[(f64, f64)]) -> String {
    let mut out = String::new();
    let mut index = 1;
    for line in subtitles {
        let (Ok(start), Ok(end)) = (parse_time(&line.start), parse_time(&line.end)) else {
            continue;
        };
        for (cut_start, cut_end) in ranges {
            let s = start.max(*cut_start);
            let e = end.min(*cut_end);
            // Skip slivers left where a line straddles a cut.
            if e - s < 0.3 {
                continue;
            }
            let (Some(os), Some(oe)) = (
                source_to_output(ranges, s),
                source_to_output(ranges, e - 0.001).map(|t| t + 0.001),
            ) else {
                continue;
            };
            out.push_str(&format!(
                "{}\n{} --> {}\n{}\n\n",
                index,
                srt_time(os),
                srt_time(oe),
                line.text
            ));
            index += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "1\n00:00:00,000 --> 00:00:02,500\n こんにちは\n\n2\r\n00:00:03,000 --> 00:00:05,000\r\n[Music]\r\n\r\n3\n00:00:05,000 --> 00:00:07,000\nline one\nline two\n";

    #[test]
    fn parses_srt_and_skips_noise() {
        let subs = parse_srt(SAMPLE);
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].text, "こんにちは");
        assert_eq!(subs[0].end, "00:00:02.500");
        assert_eq!(subs[1].text, "line one line two");
    }

    #[test]
    fn remaps_srt_to_edited_timeline() {
        let subs = parse_srt(SAMPLE);
        // Keep 0-1s and 5-7s: first line is trimmed, second shifts to 1s.
        let srt = to_output_srt(&subs, &[(0.0, 1.0), (5.0, 7.0)]);
        assert!(srt.contains("00:00:00,000 --> 00:00:01,000\nこんにちは"));
        assert!(srt.contains("00:00:01,000 --> 00:00:03,000\nline one line two"));
    }
}
