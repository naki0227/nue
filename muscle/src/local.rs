//! Offline editor: builds an edit plan from audio/video signals only, using
//! the conventions of short-form talking videos.
//!
//! - silence, filler words ("えー", "um") and false starts are cut out
//! - jump cuts alternate between a normal and a punched-in framing; long
//!   takes get a slow push-in so the picture never sits still
//! - vertical crops follow the moving subject instead of the frame center
//! - an opening title hooks the viewer in the first seconds
//! - lines spoken with emphasis (loudness peaks) become big telops with a pop,
//!   the strongest ones also get a zoom and an impact sound
//! - scene changes get a whoosh

use crate::media::{self, MediaInfo, MOTION_COLUMNS};
use crate::meta::subtract_ranges;
use crate::plan::{
    format_time, parse_time, CaptionStyle, Cut, EditPlan, Overlay, SeEvent, Thumbnail, TimedText,
    VisualEffect,
};
use crate::subtitles::{group_words, Transcript, Word};
use std::path::Path;

pub struct LocalSignals {
    pub silences: Vec<(f64, f64)>,
    pub loudness: Vec<(f64, f64)>,
    pub scenes: Vec<f64>,
    /// Per-frame horizontal motion energy for subject-aware framing.
    pub motion: Vec<(f64, Vec<f32>)>,
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
        motion: media::motion_columns(input).unwrap_or_default(),
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

/// Hesitation sounds, compared after stripping elongation marks and punctuation.
const FILLERS: &[&str] = &[
    "え",
    "えと",
    "あの",
    "あのう",
    "まあ",
    "まぁ",
    "うーん",
    "um",
    "uh",
    "uhm",
    "erm",
    "hmm",
    "mm",
];

fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| !"ー〜~っ、。，．,.!！?？…・ 　".contains(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// A filler is a hesitation word said on its own: drawn out, or set apart by
/// pauses ("あの" inside "あの本" is a normal word and stays).
fn is_filler(words: &[Word], i: usize) -> bool {
    let word = &words[i];
    if !FILLERS.contains(&normalize(&word.text).as_str()) {
        return false;
    }
    let drawn_out = word.end - word.start >= 0.25 || word.text.contains('ー');
    let pause_before = i == 0 || word.start - words[i - 1].end >= 0.15;
    let pause_after = i + 1 == words.len() || words[i + 1].start - word.end >= 0.15;
    drawn_out || pause_before || pause_after
}

/// Time spans of filler words, slightly padded.
pub fn filler_spans(words: &[Word]) -> Vec<(f64, f64)> {
    (0..words.len())
        .filter(|i| is_filler(words, *i))
        .map(|i| ((words[i].start - 0.04).max(0.0), words[i].end + 0.04))
        .collect()
}

/// False starts: a line that the next line repeats from the beginning
/// ("今日は" followed by "今日は晴れです") is dropped, keeping the last take.
pub fn retake_spans(lines: &[TimedText]) -> Vec<(f64, f64)> {
    lines
        .windows(2)
        .filter_map(|pair| {
            let (a, b) = (normalize(&pair[0].text), normalize(&pair[1].text));
            let close = match (parse_time(&pair[0].end), parse_time(&pair[1].start)) {
                (Ok(end), Ok(next)) => next - end < 3.0,
                _ => false,
            };
            (close && a.chars().count() >= 3 && b.starts_with(&a)).then(|| {
                (
                    parse_time(&pair[0].start).unwrap_or(0.0),
                    parse_time(&pair[1].start).unwrap_or(0.0),
                )
            })
        })
        .collect()
}

/// Horizontal center (0..1) of the moving subject within `range`, or `None`
/// when motion is weak or spread out (then the frame center is safest).
pub fn subject_center(motion: &[(f64, Vec<f32>)], range: (f64, f64)) -> Option<f64> {
    let mut columns = vec![0f64; MOTION_COLUMNS];
    for (t, frame) in motion.iter().filter(|(t, _)| *t >= range.0 && *t < range.1) {
        let _ = t;
        for (c, v) in frame.iter().enumerate() {
            columns[c] += *v as f64;
        }
    }
    let total: f64 = columns.iter().sum();
    if total < 2_000.0 {
        return None;
    }
    let n = MOTION_COLUMNS as f64;
    let mean = columns
        .iter()
        .enumerate()
        .map(|(c, v)| (c as f64 + 0.5) / n * v)
        .sum::<f64>()
        / total;
    let spread = (columns
        .iter()
        .enumerate()
        .map(|(c, v)| ((c as f64 + 0.5) / n - mean).powi(2) * v)
        .sum::<f64>()
        / total)
        .sqrt();
    // Uniform motion has a spread of about 0.29; a person is far narrower.
    if spread > 0.24 {
        return None;
    }
    // Lean toward the center a little to stay stable between cuts.
    Some((0.5 + (mean - 0.5) * 0.85).clamp(0.0, 1.0))
}

fn line_at(lines: &[TimedText], t: f64) -> Option<&TimedText> {
    lines.iter().find(|l| {
        matches!((parse_time(&l.start), parse_time(&l.end)), (Ok(s), Ok(e)) if t >= s - 0.2 && t < e)
    })
}

fn telop_style(size: Option<&str>) -> CaptionStyle {
    CaptionStyle {
        font: None,
        color: Some("yellow".to_string()),
        position: Some("top".to_string()),
        start_box: Some(true),
        background_asset: None,
        size: size.map(str::to_string),
    }
}

/// Shortens a line for a telop: drops trailing punctuation and keeps about
/// 16 full-width characters, breaking Latin text between words.
fn telop_text(line: &str) -> String {
    const MAX_UNITS: usize = 32;
    let trimmed = line.trim().trim_end_matches(['。', '、', ',', '.']);
    let mut units = 0;
    let mut cut = trimmed.len();
    let mut last_space = None;
    for (i, c) in trimmed.char_indices() {
        units += if c.is_ascii() { 1 } else { 2 };
        if c == ' ' {
            last_space = Some(i);
        }
        if units > MAX_UNITS {
            cut = match last_space {
                Some(space) if c.is_ascii() => space,
                _ => i,
            };
            break;
        }
    }
    if cut < trimmed.len() {
        format!("{}…", trimmed[..cut].trim_end_matches([' ', ',', '、']))
    } else {
        trimmed.to_string()
    }
}

fn loudness_at(curve: &[(f64, f64)], t: f64) -> f64 {
    curve
        .iter()
        .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()))
        .map(|p| p.1)
        .unwrap_or(-120.0)
}

pub fn build_plan(
    file_name: &str,
    job_id: &str,
    info: &MediaInfo,
    signals: &LocalSignals,
    transcript: Transcript,
    jump_cut: bool,
) -> EditPlan {
    let duration = info.duration;
    let mut plan = EditPlan::new(file_name, job_id, "local");

    // 1) What to keep: speech without dead air, fillers and false starts.
    let mut ranges = if jump_cut && !signals.silences.is_empty() {
        speech_ranges(duration, &signals.silences)
    } else {
        Vec::new()
    };
    let kept: f64 = ranges.iter().map(|(s, e)| e - s).sum();
    if kept < duration * 0.25 {
        ranges = if info.has_audio {
            vec![(0.0, duration)]
        } else {
            scene_ranges(duration, &signals.scenes)
        };
    }
    let words = transcript.words;
    let mut removed = Vec::new();
    if jump_cut && !words.is_empty() {
        removed.extend(filler_spans(&words));
        removed.extend(retake_spans(&group_words(&words)));
        let trimmed: Vec<(f64, f64)> = subtract_ranges(&ranges, &removed)
            .into_iter()
            .filter(|(s, e)| e - s >= 0.4)
            .collect();
        if !trimmed.is_empty() {
            ranges = trimmed;
        }
    }
    let lines = if words.is_empty() {
        transcript.lines
    } else {
        let spoken: Vec<Word> = words
            .iter()
            .filter(|w| {
                !removed
                    .iter()
                    .any(|(s, e)| w.start >= *s - 0.01 && w.end <= *e + 0.01)
            })
            .cloned()
            .collect();
        group_words(&spoken)
    };

    // 2) Framing: follow the subject, alternate punch-ins across jump cuts,
    //    slowly push in on long takes.
    plan.cuts = ranges
        .iter()
        .map(|(s, e)| {
            let mut cut = Cut::new(*s, *e);
            cut.focus_point = subject_center(&signals.motion, (*s, *e));
            cut
        })
        .collect();
    let mut punched = false;
    for (i, (s, e)) in ranges.iter().enumerate() {
        let jump = i > 0 && s - ranges[i - 1].1 < 1.5;
        punched = jump && !punched;
        let effect = if punched {
            "punch_in"
        } else if e - s >= 4.0 {
            "push_in"
        } else {
            continue;
        };
        plan.visual_effects.push(VisualEffect {
            start: format_time(*s),
            end: Some(format_time(*e)),
            effect_type: effect.to_string(),
            speed: None,
        });
    }

    // 3) Emphasis: loud moments become telops; the strongest also zoom and hit.
    let peaks: Vec<f64> = loudness_peaks(&signals.loudness, 6.0)
        .into_iter()
        .filter(|t| in_ranges(&ranges, *t))
        .collect();
    let mut by_strength = peaks.clone();
    by_strength.sort_by(|a, b| {
        loudness_at(&signals.loudness, *b).total_cmp(&loudness_at(&signals.loudness, *a))
    });
    let strongest: Vec<f64> = by_strength.iter().take(3).copied().collect();

    for t in peaks.iter().take(12) {
        let strong = strongest.contains(t);
        if let Some(line) = line_at(&lines, *t) {
            let start = parse_time(&line.start).unwrap_or(*t).max(t - 0.3);
            let end = parse_time(&line.end)
                .unwrap_or(t + 2.0)
                .max(start + 1.5)
                .min(start + 3.0);
            plan.telops.push(TimedText {
                start: format_time(start),
                end: format_time(end),
                text: telop_text(&line.text),
                style: Some(telop_style(None)),
            });
            let icon = if line.text.ends_with(['！', '!']) {
                Some("exclaim")
            } else if line.text.ends_with(['？', '?']) {
                Some("question")
            } else {
                None
            };
            if let Some(icon) = icon {
                plan.overlays.push(Overlay {
                    asset: icon.to_string(),
                    start: format_time(start),
                    end: format_time(end),
                    position: Some("top_right".to_string()),
                    scale: Some(0.16),
                });
            }
            plan.se_events.push(SeEvent {
                timestamp: format_time(start),
                event_type: if strong { "impact" } else { "pop" }.to_string(),
                tag: Some("emphasis".to_string()),
            });
        } else if strong {
            plan.se_events.push(SeEvent {
                timestamp: format_time(*t),
                event_type: "impact".to_string(),
                tag: Some("loudness_peak".to_string()),
            });
        }
        if strong {
            plan.visual_effects.push(VisualEffect {
                start: format_time(*t),
                end: Some(format_time((t + 0.9).min(duration))),
                effect_type: "zoom_in".to_string(),
                speed: Some("fast".to_string()),
            });
        }
    }

    // 4) Hook: an opening title over the first seconds.
    let title = strongest
        .first()
        .and_then(|t| line_at(&lines, *t))
        .or_else(|| lines.iter().find(|l| l.text.chars().count() >= 4))
        .map(|l| telop_text(&l.text))
        .unwrap_or_default();
    if let Some((first_start, first_end)) = ranges.first().copied() {
        if !title.is_empty() {
            let hook_end = (first_start + 2.5).min(first_end);
            plan.telops
                .retain(|t| parse_time(&t.start).map(|s| s >= hook_end).unwrap_or(true));
            plan.telops.insert(
                0,
                TimedText {
                    start: format_time(first_start),
                    end: format_time(hook_end),
                    text: title.clone(),
                    style: Some(CaptionStyle {
                        position: Some("center".to_string()),
                        ..telop_style(Some("large"))
                    }),
                },
            );
        }
        plan.se_events.push(SeEvent {
            timestamp: format_time(first_start),
            event_type: "whoosh".to_string(),
            tag: Some("opening".to_string()),
        });
    }

    // 5) Scene changes: whoosh, away from other sounds.
    let mut last_whoosh = f64::NEG_INFINITY;
    for t in &signals.scenes {
        let busy = plan.se_events.iter().any(|se| {
            parse_time(&se.timestamp)
                .map(|s| (s - t).abs() < 1.5)
                .unwrap_or(false)
        });
        if in_ranges(&ranges, *t) && !busy && t - last_whoosh >= 6.0 {
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

    let thumb_time = strongest
        .first()
        .copied()
        .or_else(|| ranges.first().map(|(s, e)| s + (e - s) / 2.0))
        .unwrap_or(duration / 2.0);
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
    plan.subtitles = lines;
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, start: f64, end: f64) -> Word {
        Word {
            text: text.to_string(),
            start,
            end,
            space_before: false,
        }
    }

    fn info(duration: f64, has_audio: bool) -> MediaInfo {
        MediaInfo {
            duration,
            has_audio,
            width: 1920,
            height: 1080,
            fps: 30.0,
            color_transfer: String::new(),
            rotation: 0,
        }
    }

    fn no_signals() -> LocalSignals {
        LocalSignals {
            silences: vec![],
            loudness: vec![],
            scenes: vec![],
            motion: vec![],
        }
    }

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
        assert_eq!(ranges.len(), 1);
        assert!((ranges[0].1 - 5.15).abs() < 1e-9);
    }

    #[test]
    fn finds_spaced_peaks() {
        let mut curve: Vec<(f64, f64)> = (0..80).map(|i| (i as f64 * 0.25, -30.0)).collect();
        curve[20].1 = -10.0;
        curve[22].1 = -12.0;
        curve[60].1 = -8.0;
        assert_eq!(loudness_peaks(&curve, 6.0), vec![5.0, 15.0]);
    }

    #[test]
    fn silent_footage_splits_on_scenes() {
        let signals = LocalSignals {
            scenes: vec![4.0, 8.0],
            ..no_signals()
        };
        let plan = build_plan(
            "a.mp4",
            "a",
            &info(12.0, false),
            &signals,
            Transcript::default(),
            true,
        );
        assert_eq!(plan.cut_ranges(), vec![(0.0, 4.0), (4.0, 8.0), (8.0, 12.0)]);
    }

    #[test]
    fn detects_fillers_but_keeps_words() {
        let words = vec![
            word("えー", 0.0, 0.5),
            word("今日", 0.6, 0.9),
            word("は", 0.9, 1.0),
            word("あの", 1.0, 1.1),
            word("本", 1.1, 1.3),
            word("um", 2.0, 2.1),
        ];
        let spans = filler_spans(&words);
        // "えー" (drawn out) and "um" (pause before) are fillers; "あの本" is not.
        assert_eq!(spans.len(), 2);
        assert!((spans[0].1 - 0.54).abs() < 1e-9);
        assert!((spans[1].0 - 1.96).abs() < 1e-9);
    }

    #[test]
    fn shortens_telops_at_word_boundaries() {
        assert_eq!(telop_text("すごい！"), "すごい！");
        assert_eq!(
            telop_text("今日は本当にいい天気ですね、散歩に行きましょう。"),
            "今日は本当にいい天気ですね、散歩…"
        );
        assert_eq!(
            telop_text("And so, my fellow Americans, ask not"),
            "And so, my fellow Americans, ask…"
        );
    }

    #[test]
    fn drops_false_starts() {
        let line = |s: f64, e: f64, t: &str| TimedText {
            start: format_time(s),
            end: format_time(e),
            text: t.into(),
            style: None,
        };
        let lines = vec![
            line(0.0, 0.8, "今日は"),
            line(1.2, 3.0, "今日は晴れです。"),
            line(3.5, 4.0, "はい"),
        ];
        assert_eq!(retake_spans(&lines), vec![(0.0, 1.2)]);
    }

    #[test]
    fn finds_subject_on_the_right() {
        let mut frame = vec![0f32; MOTION_COLUMNS];
        frame[44..52].fill(500.0);
        let motion: Vec<(f64, Vec<f32>)> =
            (0..8).map(|i| (i as f64 * 0.25, frame.clone())).collect();
        let center = subject_center(&motion, (0.0, 2.0)).unwrap();
        assert!(center > 0.7 && center < 0.8, "{center}");
        let flat = vec![(0.5, vec![100f32; MOTION_COLUMNS])];
        assert_eq!(subject_center(&flat, (0.0, 1.0)), None);
    }

    #[test]
    fn edits_like_a_short_form_video() {
        // Speech 0-4 s, pause, speech 5-12 s with a filler at 5.0 and an emphatic line at 9 s.
        let words = vec![
            word("今日", 0.2, 0.6),
            word("は", 0.6, 0.8),
            word("いい", 0.8, 1.2),
            word("天気", 1.2, 1.8),
            word("。", 1.8, 1.9),
            word("えー", 5.0, 5.6),
            word("それ", 5.8, 6.2),
            word("で", 6.2, 6.4),
            word("すごい", 9.0, 9.6),
            word("！", 9.6, 9.7),
            word("おわり", 11.0, 11.8),
        ];
        let mut loudness: Vec<(f64, f64)> = (0..48).map(|i| (i as f64 * 0.25, -30.0)).collect();
        loudness[37].1 = -8.0; // 9.25 s
        let signals = LocalSignals {
            silences: vec![(4.0, 5.0)],
            loudness,
            ..no_signals()
        };
        let transcript = Transcript {
            lines: group_words(&words),
            words,
        };
        let plan = build_plan("a.mp4", "a", &info(12.0, true), &signals, transcript, true);

        let ranges = plan.cut_ranges();
        assert!(
            ranges.iter().all(|(s, e)| !(*s < 5.5 && *e > 5.1)),
            "filler kept: {ranges:?}"
        );
        assert!(plan
            .visual_effects
            .iter()
            .any(|v| v.effect_type == "punch_in"));
        let hook = &plan.telops[0];
        assert_eq!(hook.text, "すごい！");
        assert_eq!(hook.style.as_ref().unwrap().size.as_deref(), Some("large"));
        assert!(plan.overlays.iter().any(|o| o.asset == "exclaim"));
        assert!(plan.se_events.iter().any(|se| se.event_type == "whoosh"));
        assert!(plan.subtitles.iter().all(|s| !s.text.contains("えー")));
        assert!(plan.subtitles.iter().any(|s| s.text == "すごい！"));
        assert_eq!(plan.thumbnail.unwrap().text, "すごい！");
    }
}
