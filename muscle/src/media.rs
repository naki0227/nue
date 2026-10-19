//! Local media analysis with ffprobe/ffmpeg: no network, no API key.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    pub duration: f64,
    pub has_audio: bool,
    pub width: u32,
    pub height: u32,
}

pub fn probe(path: &Path) -> Result<MediaInfo> {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries"])
        .arg("format=duration:stream=codec_type,width,height")
        .args(["-of", "json"])
        .arg(path)
        .output()
        .context("Failed to run ffprobe (is ffmpeg installed?)")?;
    if !output.status.success() {
        bail!(
            "ffprobe failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let streams = json["streams"].as_array().cloned().unwrap_or_default();
    let video = streams.iter().find(|s| s["codec_type"] == "video");
    let duration = json["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse::<f64>().ok())
        .filter(|d| *d > 0.0)
        .context("Could not read video duration")?;

    Ok(MediaInfo {
        duration,
        has_audio: streams.iter().any(|s| s["codec_type"] == "audio"),
        width: video.and_then(|v| v["width"].as_u64()).unwrap_or(0) as u32,
        height: video.and_then(|v| v["height"].as_u64()).unwrap_or(0) as u32,
    })
}

/// Silent ranges reported by ffmpeg's `silencedetect`.
pub fn detect_silence(path: &Path, noise_db: f64, min_duration: f64) -> Result<Vec<(f64, f64)>> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(path)
        .args(["-vn", "-af"])
        .arg(format!(
            "silencedetect=noise={}dB:d={}",
            noise_db, min_duration
        ))
        .args(["-f", "null", "-"])
        .output()
        .context("Failed to run ffmpeg silencedetect")?;
    Ok(parse_silencedetect(&String::from_utf8_lossy(
        &output.stderr,
    )))
}

pub fn parse_silencedetect(log: &str) -> Vec<(f64, f64)> {
    let mut ranges = Vec::new();
    let mut open: Option<f64> = None;
    for line in log.lines() {
        if let Some(value) = value_after(line, "silence_start:") {
            open = Some(value.max(0.0));
        } else if let Some(value) = value_after(line, "silence_end:") {
            ranges.push((open.take().unwrap_or(0.0), value));
        }
    }
    if let Some(start) = open {
        // Silence runs to the end of the file.
        ranges.push((start, f64::INFINITY));
    }
    ranges
}

fn value_after(line: &str, key: &str) -> Option<f64> {
    let rest = &line[line.find(key)? + key.len()..];
    rest.split_whitespace().next()?.parse().ok()
}

/// RMS loudness (dBFS) sampled every `window` seconds.
pub fn loudness_curve(path: &Path, window: f64) -> Result<Vec<(f64, f64)>> {
    let samples = (8000.0 * window).round().max(1.0) as u32;
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(path)
        .args(["-vn", "-af"])
        .arg(format!(
            "aresample=8000,asetnsamples=n={samples}:p=0,astats=metadata=1:reset=1,\
             ametadata=print:key=lavfi.astats.Overall.RMS_level:file=-"
        ))
        .args(["-f", "null", "-"])
        .output()
        .context("Failed to run ffmpeg astats")?;
    Ok(parse_ametadata(&String::from_utf8_lossy(&output.stdout)))
}

pub fn parse_ametadata(log: &str) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    let mut time: Option<f64> = None;
    for line in log.lines() {
        if let Some(value) = value_after(line, "pts_time:") {
            time = Some(value);
        } else if let Some((_, value)) = line.split_once("RMS_level=") {
            if let (Some(t), Ok(db)) = (time, value.trim().parse::<f64>()) {
                points.push((t, if db.is_finite() { db } else { -120.0 }));
            } else if let Some(t) = time {
                points.push((t, -120.0));
            }
        }
    }
    points
}

/// Timestamps where the picture changes sharply.
pub fn scene_changes(path: &Path, threshold: f64) -> Result<Vec<f64>> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(path)
        .args(["-an", "-vf"])
        .arg(format!(
            "scale=160:-2,select='gt(scene,{threshold})',metadata=print:file=-"
        ))
        .args(["-f", "null", "-"])
        .output()
        .context("Failed to run ffmpeg scene detection")?;
    let log = String::from_utf8_lossy(&output.stdout);
    Ok(log
        .lines()
        .filter_map(|line| value_after(line, "pts_time:"))
        .collect())
}

/// Extracts 16 kHz mono WAV, the input format speech recognizers expect.
pub fn extract_speech_wav(path: &Path, out: &Path) -> Result<()> {
    let status = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
        .arg(out)
        .status()
        .context("Failed to run ffmpeg audio extraction")?;
    if !status.success() {
        bail!("Audio extraction failed for {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_silencedetect_output() {
        let log = "[silencedetect @ 0x1] silence_start: 1.5\n\
                   [silencedetect @ 0x1] silence_end: 3.25 | silence_duration: 1.75\n\
                   [silencedetect @ 0x1] silence_start: 9\n";
        let ranges = parse_silencedetect(log);
        assert_eq!(ranges[0], (1.5, 3.25));
        assert_eq!(ranges[1].0, 9.0);
        assert!(ranges[1].1.is_infinite());
    }

    #[test]
    fn parses_ametadata_output() {
        let log = "frame:0    pts:0       pts_time:0\nlavfi.astats.Overall.RMS_level=-25.5\n\
                   frame:1    pts:4000    pts_time:0.5\nlavfi.astats.Overall.RMS_level=-inf\n";
        let points = parse_ametadata(log);
        assert_eq!(points, vec![(0.0, -25.5), (0.5, -120.0)]);
    }
}
