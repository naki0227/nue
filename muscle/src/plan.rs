//! Edit-plan JSON schema shared by every analyzer (local, Gemini, the iOS app)
//! and the renderer. Times are source-video timestamps written as
//! `HH:MM:SS` or `HH:MM:SS.mmm`.

use anyhow::{bail, Result};
use serde::{Deserialize, Deserializer, Serialize};

/// Treats `null` the same as a missing field so loosely generated JSON still parses.
fn null_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CaptionStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
    #[serde(rename = "box", default, skip_serializing_if = "Option::is_none")]
    pub start_box: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_asset: Option<String>,
    /// normal | large (opening titles)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Cut {
    pub start_time: String,
    pub end_time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Legacy per-cut caption, shown for the whole cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption_style: Option<CaptionStyle>,
    /// Horizontal crop focus for reframing, 0.0 (left) .. 1.0 (right).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_point: Option<f64>,
}

impl Cut {
    pub fn new(start: f64, end: f64) -> Self {
        Cut {
            start_time: format_time(start),
            end_time: format_time(end),
            description: None,
            caption: None,
            caption_style: None,
            focus_point: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeEvent {
    pub timestamp: String,
    /// Sound name: matches `data/se/<type>.wav|mp3` or a built-in alias.
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualEffect {
    pub start: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    /// zoom_in | zoom_out | pan_left | pan_right
    #[serde(rename = "type")]
    pub effect_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Thumbnail {
    pub timestamp: String,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// A timed text line: speech subtitles and emphasis telops share this shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimedText {
    pub start: String,
    pub end: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<CaptionStyle>,
}

/// An icon/sticker image from `data/icons/<asset>.png` laid over the video.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Overlay {
    pub asset: String,
    pub start: String,
    pub end: String,
    /// top_left | top_right | bottom_left | bottom_right | center
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
    /// Width as a fraction of the output width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderOptions {
    #[serde(default = "yes")]
    pub bgm: bool,
    #[serde(default = "yes")]
    pub sound_effects: bool,
    #[serde(default = "yes")]
    pub subtitles: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            bgm: true,
            sound_effects: true,
            subtitles: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EditPlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    pub original_filename: String,
    /// Which analyzer produced the plan: local | gemini | fallback | external.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analyzer: Option<String>,
    /// vertical (1080x1920, default) | horizontal (1920x1080) | square (1080x1080)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgm_path: Option<String>,
    pub cuts: Vec<Cut>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editing_style: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "null_default")]
    pub se_events: Vec<SeEvent>,
    #[serde(default, deserialize_with = "null_default")]
    pub visual_effects: Vec<VisualEffect>,
    #[serde(default, deserialize_with = "null_default")]
    pub telops: Vec<TimedText>,
    #[serde(default, deserialize_with = "null_default")]
    pub subtitles: Vec<TimedText>,
    #[serde(default, deserialize_with = "null_default")]
    pub overlays: Vec<Overlay>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<Thumbnail>,
    #[serde(default, deserialize_with = "null_default")]
    pub options: RenderOptions,
}

impl EditPlan {
    pub fn new(original_filename: &str, job_id: &str, analyzer: &str) -> Self {
        EditPlan {
            job_id: Some(job_id.to_string()),
            original_filename: original_filename.to_string(),
            analyzer: Some(analyzer.to_string()),
            format: None,
            bgm_path: None,
            cuts: Vec::new(),
            editing_style: None,
            se_events: Vec::new(),
            visual_effects: Vec::new(),
            telops: Vec::new(),
            subtitles: Vec::new(),
            overlays: Vec::new(),
            thumbnail: None,
            options: RenderOptions::default(),
        }
    }

    /// Cut ranges in source seconds, dropping invalid ones.
    pub fn cut_ranges(&self) -> Vec<(f64, f64)> {
        self.cuts
            .iter()
            .filter_map(|cut| {
                let start = parse_time(&cut.start_time).ok()?;
                let end = parse_time(&cut.end_time).ok()?;
                (end > start).then_some((start, end))
            })
            .collect()
    }

    pub fn output_duration(&self) -> f64 {
        self.cut_ranges().iter().map(|(s, e)| e - s).sum()
    }
}

/// Parses `HH:MM:SS(.fff)`, `MM:SS(.fff)` or plain seconds. The legacy
/// `MM:SS:mmm` form (three-digit integer milliseconds) is still accepted.
pub fn parse_time(time_str: &str) -> Result<f64> {
    let trimmed = time_str.trim().replace(',', ".");
    let parts: Vec<&str> = trimmed.split(':').collect();
    let seconds = match parts.as_slice() {
        [h, m, s] => {
            let (first, second, third) = (h.parse::<f64>()?, m.parse::<f64>()?, s.parse::<f64>()?);
            if !s.contains('.') && s.len() == 3 && third > 59.0 {
                first * 60.0 + second + third / 1000.0
            } else {
                first * 3600.0 + second * 60.0 + third
            }
        }
        [m, s] => m.parse::<f64>()? * 60.0 + s.parse::<f64>()?,
        [s] => s.parse::<f64>()?,
        _ => bail!("Unrecognized timestamp: {}", time_str),
    };
    if !seconds.is_finite() || seconds < 0.0 {
        bail!("Invalid timestamp: {}", time_str);
    }
    Ok(seconds)
}

pub fn format_time(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let secs = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    format!("{hours:02}:{minutes:02}:{secs:02}.{millis:03}")
}

/// Maps a source timestamp onto the edited timeline. Returns `None` when the
/// moment was cut out.
pub fn source_to_output(ranges: &[(f64, f64)], source: f64) -> Option<f64> {
    let mut offset = 0.0;
    for (start, end) in ranges {
        if source >= *start && source < *end {
            return Some(offset + source - start);
        }
        offset += end - start;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_formats() {
        assert_eq!(parse_time("00:01:02").unwrap(), 62.0);
        assert_eq!(parse_time("00:00:59.500").unwrap(), 59.5);
        assert_eq!(parse_time("00:00:01,250").unwrap(), 1.25);
        assert_eq!(parse_time("01:30").unwrap(), 90.0);
        assert_eq!(parse_time("4.5").unwrap(), 4.5);
        // legacy MM:SS:mmm
        assert!((parse_time("01:02:500").unwrap() - 62.5).abs() < 1e-9);
        assert!(parse_time("abc").is_err());
    }

    #[test]
    fn formats_round_trip() {
        assert_eq!(format_time(3723.456), "01:02:03.456");
        assert_eq!(parse_time(&format_time(12.345)).unwrap(), 12.345);
    }

    #[test]
    fn maps_source_time_to_output() {
        let ranges = vec![(1.0, 3.0), (5.0, 8.0)];
        assert_eq!(source_to_output(&ranges, 2.0), Some(1.0));
        assert_eq!(source_to_output(&ranges, 4.0), None);
        assert_eq!(source_to_output(&ranges, 6.0), Some(3.0));
    }

    #[test]
    fn accepts_legacy_gemini_json() {
        let json = r#"{
            "original_filename": "a.mp4",
            "cuts": [{"start_time": "00:00:00", "end_time": "00:00:05", "filter": "none",
                      "caption": "hi", "caption_style": {"box": true, "color": "yellow"}}],
            "se_events": null,
            "visual_effects": [{"start": "00:00:01", "type": "zoom_in"}],
            "thumbnail": {"timestamp": "00:00:02", "text": "WOW"}
        }"#;
        let plan: EditPlan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.cuts.len(), 1);
        assert!(plan.se_events.is_empty());
        assert!(plan.options.bgm);
        assert_eq!(plan.output_duration(), 5.0);
    }
}
