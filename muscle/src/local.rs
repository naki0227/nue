//! Offline analyzer: builds an edit plan from audio/video signals only.
//!
//! - silencedetect -> jump cuts that drop dead air
//! - loudness peaks -> impact sound effect + punch-in zoom
//! - scene changes -> whoosh on visual transitions (silent footage is split there)
//! - transcript    -> subtitles, thumbnail title

use crate::media::{self, MediaInfo};
use crate::plan::{
    format_time, parse_time, Cut, EditPlan, Overlay, SeEvent, Thumbnail, TimedText, VisualEffect,
};
use std::path::Path;

pub struct LocalSignals {
    pub silences: Vec<(f64, f64)>,
    pub loudness: Vec<(f64, f64)>,
    pub scenes: Vec<f64>,
}

pub fn collect_signals(input: &Path, info: &MediaInfo) -> LocalSignals {
    let (silences, loudness) = if info.has_audio {
        (
            media::detect_silence(input, -35.0, 0.6).unwrap_or_default(),
            media::loudness_curve(input, 0.25).unwrap_or_default(),
        )
    } else {
        (Vec::new(), Vec::new())
    };
    LocalSignals {
        silences,
        loudness,
        scenes: media::scene_changes(input, 0.35).unwrap_or_default(),
    }
}

/// Complement of the silent ranges, padded and cleaned up.
pub fn speech_ranges(duration: f64, silences: &[(f64, f64)]) -> Vec<(f64, f64)> {
    const PAD: f64 = 0.15;
    const MERGE_GAP: f64 = 0.4;
    const MIN_LEN: f64 = 0.5;

    let mut ranges = Vec::new();
    let mut cursor = 0.0;
    for (start, end) in silences {
        if *start > cursor {
            ranges.push((cursor, *start));
        }
        cursor = end.min(duration);
    }
    if cursor < duration {
        ranges.push((cursor, duration));
    }

    let padded: Vec<(f64, f64)> = ranges
        .into_iter()
        .map(|(s, e)| ((s - PAD).max(0.0), (e + PAD).min(duration)))
        .collect();
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (s, e) in padded {
        match merged.last_mut() {
            Some(last) if s - last.1 <= MERGE_GAP => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
        .into_iter()
        .filter(|(s, e)| e - s >= MIN_LEN)
        .collect()
}

/// Local maxima well above the typical level, at least `min_gap` apart.
pub fn loudness_peaks(curve: &[(f64, f64)], min_gap: f64) -> Vec<f64> {
    let audible: Vec<f64> = curve
        .iter()
        .map(|(_, db)| *db)
        .filter(|db| *db > -60.0)
        .collect();
    if audible.len() < 8 {
        return Vec::new();
    }
    let mut sorted = audible.clone();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    let threshold = (median + 8.0).max(-22.0);

    let mut candidates: Vec<(f64, f64)> = curve
        .windows(3)
        .filter(|w| w[1].1 >= threshold && w[1].1 >= w[0].1 && w[1].1 >= w[2].1)
        .map(|w| w[1])
        .collect();
    // Loudest first, then enforce spacing.
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut picked: Vec<f64> = Vec::new();
    for (t, _) in candidates {
        if picked.iter().all(|p| (p - t).abs() >= min_gap) {
            picked.push(t);
        }
    }
    picked.sort_by(f64::total_cmp);
    picked
}

fn in_ranges(ranges: &[(f64, f64)], t: f64) -> bool {
    ranges.iter().any(|(s, e)| t >= *s && t < *e)
}

/// Splits footage at scene changes so silent videos still get a rhythm.
fn scene_ranges(duration: f64, scenes: &[f64]) -> Vec<(f64, f64)> {
    let mut points: Vec<f64> = scenes
        .iter()
        .copied()
        .filter(|t| *t > 1.0 && *t < duration - 1.0)
        .collect();
    points.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    let mut ranges = Vec::new();
    let mut cursor = 0.0;
    for p in points {
        ranges.push((cursor, p));
        cursor = p;
    }
    ranges.push((cursor, duration));
    ranges
}

pub fn build_plan(
    file_name: &str,
    job_id: &str,
    info: &MediaInfo,
    signals: &LocalSignals,
    subtitles: Vec<TimedText>,
    jump_cut: bool,
) -> EditPlan {
    let duration = info.duration;
    let mut plan = EditPlan::new(file_name, job_id, "local");

    let mut ranges = if jump_cut && !signals.silences.is_empty() {
        speech_ranges(duration, &signals.silences)
    } else {
        Vec::new()
    };
    // Mostly-silent or music-only footage: keep everything rather than gut it.
    let kept: f64 = ranges.iter().map(|(s, e)| e - s).sum();
    if kept < duration * 0.25 {
        ranges = if info.has_audio {
            vec![(0.0, duration)]
        } else {
            scene_ranges(duration, &signals.scenes)
        };
    }
    plan.cuts = ranges.iter().map(|(s, e)| Cut::new(*s, *e)).collect();

    let peaks: Vec<f64> = loudness_peaks(&signals.loudness, 6.0)
        .into_iter()
        .filter(|t| in_ranges(&ranges, *t))
        .take(12)
        .collect();
    for t in &peaks {
        plan.se_events.push(SeEvent {
            timestamp: format_time(*t),
            event_type: "impact".to_string(),
            tag: Some("loudness_peak".to_string()),
        });
        plan.visual_effects.push(VisualEffect {
            start: format_time(*t),
            end: Some(format_time((t + 1.2).min(duration))),
            effect_type: "zoom_in".to_string(),
            speed: Some("fast".to_string()),
        });
        plan.overlays.push(Overlay {
            asset: "exclaim".to_string(),
            start: format_time(*t),
            end: format_time((t + 1.0).min(duration)),
            position: Some("top_right".to_string()),
            scale: Some(0.18),
        });
    }

    let mut last_whoosh = f64::NEG_INFINITY;
    for t in &signals.scenes {
        let near_peak = peaks.iter().any(|p| (p - t).abs() < 1.5);
        if in_ranges(&ranges, *t) && !near_peak && t - last_whoosh >= 4.0 {
            plan.se_events.push(SeEvent {
                timestamp: format_time(*t),
                event_type: "whoosh".to_string(),
                tag: Some("scene_change".to_string()),
            });
            last_whoosh = *t;
        }
    }
    plan.se_events.sort_by(|a, b| {
        parse_time(&a.timestamp)
            .unwrap_or(0.0)
            .total_cmp(&parse_time(&b.timestamp).unwrap_or(0.0))
    });

    let thumb_time = peaks
        .first()
        .copied()
        .or_else(|| ranges.first().map(|(s, e)| s + (e - s) / 2.0))
        .unwrap_or(duration / 2.0);
    let title = subtitles
        .iter()
        .map(|s| s.text.trim())
        .find(|t| t.chars().count() >= 4)
        .map(|t| t.chars().take(16).collect::<String>())
        .unwrap_or_default();
    plan.thumbnail = Some(Thumbnail {
        timestamp: format_time(thumb_time),
        text: title,
        color: Some("yellow".to_string()),
    });

    let mood = if peaks.len() as f64 / duration.max(1.0) > 0.05 {
        "energetic"
    } else {
        "calm"
    };
    plan.editing_style = Some(serde_json::json!({
        "tempo": if jump_cut { "fast" } else { "natural" },
        "mood": mood
    }));
    plan.subtitles = subtitles;
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_silence_with_padding() {
        let ranges = speech_ranges(10.0, &[(2.0, 4.0), (9.0, f64::INFINITY)]);
        assert_eq!(ranges.len(), 2);
        assert!((ranges[0].0 - 0.0).abs() < 1e-9 && (ranges[0].1 - 2.15).abs() < 1e-9);
        assert!((ranges[1].0 - 3.85).abs() < 1e-9 && (ranges[1].1 - 9.15).abs() < 1e-9);
    }

    #[test]
    fn merges_short_gaps_and_drops_blips() {
        let ranges = speech_ranges(10.0, &[(3.0, 3.6), (5.0, 9.8)]);
        // 3.0-3.6 gap is closed by padding/merge; 9.8-10 blip (0.35s) is dropped.
        assert_eq!(ranges.len(), 1);
        assert!((ranges[0].1 - 5.15).abs() < 1e-9);
    }

    #[test]
    fn finds_spaced_peaks() {
        let mut curve: Vec<(f64, f64)> = (0..80).map(|i| (i as f64 * 0.25, -30.0)).collect();
        curve[20].1 = -10.0; // 5.0s
        curve[22].1 = -12.0; // 5.5s, too close
        curve[60].1 = -8.0; // 15.0s
        assert_eq!(loudness_peaks(&curve, 6.0), vec![5.0, 15.0]);
    }

    #[test]
    fn silent_footage_splits_on_scenes() {
        let info = MediaInfo {
            duration: 12.0,
            has_audio: false,
            width: 1920,
            height: 1080,
        };
        let signals = LocalSignals {
            silences: vec![],
            loudness: vec![],
            scenes: vec![4.0, 8.0],
        };
        let plan = build_plan("a.mp4", "a", &info, &signals, vec![], true);
        assert_eq!(plan.cut_ranges(), vec![(0.0, 4.0), (4.0, 8.0), (8.0, 12.0)]);
    }
}
