//! Job metadata sent by the web UI (`<video>_metadata.json`) or CLI flags:
//! script, manual edits and render options, applied on top of an analyzed plan.

use crate::plan::{format_time, parse_time, CaptionStyle, Cut, EditPlan, TimedText, VisualEffect};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ManualCut {
    pub start: String,
    pub end: String,
    /// keep | remove
    #[serde(default)]
    pub action: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ManualCaption {
    pub timestamp: String,
    pub text: String,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ManualEffect {
    pub timestamp: String,
    #[serde(rename = "type")]
    pub effect_type: String,
    #[serde(default)]
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ManualInstructions {
    #[serde(default)]
    pub cuts: Option<Vec<ManualCut>>,
    #[serde(default)]
    pub captions: Option<Vec<ManualCaption>>,
    #[serde(default)]
    pub effects: Option<Vec<ManualEffect>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct JobOptions {
    #[serde(default)]
    pub auto_sound_effects: Option<bool>,
    #[serde(default)]
    pub generate_bgm: Option<bool>,
    #[serde(default)]
    pub subtitles: Option<bool>,
    /// auto | local | gemini
    #[serde(default)]
    pub analyzer: Option<String>,
    /// vertical | horizontal | square
    #[serde(default)]
    pub format: Option<String>,
    /// Remove silent pauses (jump cuts). Local analyzer only.
    #[serde(default)]
    pub jump_cut: Option<bool>,
    /// Editing format: shorts | vlog | variety | cinematic | business | <custom>.
    #[serde(default)]
    pub style: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct JobMetadata {
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub manual_instructions: Option<ManualInstructions>,
    #[serde(default)]
    pub options: Option<JobOptions>,
}

impl JobMetadata {
    pub fn options(&self) -> JobOptions {
        self.options.clone().unwrap_or_default()
    }

    /// Script lines like `00:00:15 - Main point` become timed telops.
    pub fn script_telops(&self) -> Vec<(f64, String)> {
        self.script
            .as_deref()
            .unwrap_or("")
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let (time, text) = line
                    .split_once(" - ")
                    .or_else(|| line.split_once('\t'))
                    .or_else(|| line.split_once(' '))?;
                let start = parse_time(time).ok()?;
                let text = text.trim();
                (!text.is_empty()).then(|| (start, text.to_string()))
            })
            .collect()
    }
}

fn style_for(name: Option<&str>) -> CaptionStyle {
    CaptionStyle {
        font: None,
        color: Some(name.unwrap_or("yellow").to_string()),
        position: Some("top".to_string()),
        start_box: Some(true),
        background_asset: None,
        size: None,
    }
}

/// Subtracts `remove` ranges from `ranges`.
pub fn subtract_ranges(ranges: &[(f64, f64)], remove: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut result = ranges.to_vec();
    for (rs, re) in remove {
        result = result
            .into_iter()
            .flat_map(|(s, e)| {
                let mut parts = Vec::new();
                if *rs > s {
                    parts.push((s, rs.min(e)));
                }
                if *re < e {
                    parts.push((re.max(s), e));
                }
                parts.into_iter().filter(|(a, b)| b - a > 0.05)
            })
            .collect();
    }
    result
}

pub fn apply_metadata(plan: &mut EditPlan, meta: &JobMetadata, duration: f64) {
    let options = meta.options();
    if options.auto_sound_effects == Some(false) {
        plan.options.sound_effects = false;
        plan.se_events.clear();
    }
    if options.generate_bgm == Some(false) {
        plan.options.bgm = false;
    }
    if options.subtitles == Some(false) {
        plan.options.subtitles = false;
    }
    if let Some(format) = options.format.filter(|f| !f.is_empty()) {
        plan.format = Some(format);
    }
    if let Some(style) = options.style.filter(|s| !s.is_empty()) {
        plan.style = Some(style);
    }

    let manual = meta.manual_instructions.clone().unwrap_or_default();

    if let Some(cuts) = manual.cuts.filter(|c| !c.is_empty()) {
        let parse = |c: &ManualCut| {
            let s = parse_time(&c.start).ok()?;
            let e = parse_time(&c.end).ok()?.min(duration);
            (e > s).then_some((s, e))
        };
        let mut keeps: Vec<(f64, f64)> = cuts
            .iter()
            .filter(|c| c.action == "keep")
            .filter_map(parse)
            .collect();
        let removes: Vec<(f64, f64)> = cuts
            .iter()
            .filter(|c| c.action != "keep")
            .filter_map(parse)
            .collect();

        let base = if keeps.is_empty() {
            plan.cut_ranges()
        } else {
            keeps.sort_by(|a, b| a.0.total_cmp(&b.0));
            keeps
        };
        let ranges = subtract_ranges(&base, &removes);
        if !ranges.is_empty() {
            let previous = plan.cuts.clone();
            plan.cuts = ranges
                .into_iter()
                .map(|(s, e)| {
                    let mut cut = Cut::new(s, e);
                    // Keep reframing/captions from the analyzed cut it came from.
                    if let Some(old) = previous.iter().find(|old| {
                        parse_time(&old.start_time).map(|t| t <= s).unwrap_or(false)
                            && parse_time(&old.end_time).map(|t| t > s).unwrap_or(false)
                    }) {
                        cut.focus_point = old.focus_point;
                        cut.description = old.description.clone();
                    }
                    cut
                })
                .collect();
        }
    }

    let mut telops: Vec<TimedText> = manual
        .captions
        .unwrap_or_default()
        .into_iter()
        .filter(|c| !c.text.trim().is_empty())
        .filter_map(|c| {
            let start = parse_time(&c.timestamp).ok()?;
            Some(TimedText {
                start: format_time(start),
                end: format_time(start + c.duration.unwrap_or(3.0)),
                text: c.text.trim().to_string(),
                style: Some(style_for(c.style.as_deref())),
                words: Vec::new(),
            })
        })
        .collect();
    if telops.is_empty() {
        let script = meta.script_telops();
        for (i, (start, text)) in script.iter().enumerate() {
            let end = script
                .get(i + 1)
                .map(|(next, _)| next.min(start + 4.0))
                .unwrap_or(start + 4.0);
            telops.push(TimedText {
                start: format_time(*start),
                end: format_time(end),
                text: text.clone(),
                style: Some(style_for(None)),
                words: Vec::new(),
            });
        }
    }
    if !telops.is_empty() {
        plan.telops.extend(telops);
    }

    for effect in manual.effects.unwrap_or_default() {
        if let Ok(start) = parse_time(&effect.timestamp) {
            plan.visual_effects.push(VisualEffect {
                start: format_time(start),
                end: Some(format_time(start + effect.duration.unwrap_or(2.0))),
                effect_type: effect.effect_type,
                speed: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_with(ranges: &[(f64, f64)]) -> EditPlan {
        let mut plan = EditPlan::new("a.mp4", "a", "local");
        plan.cuts = ranges.iter().map(|(s, e)| Cut::new(*s, *e)).collect();
        plan
    }

    #[test]
    fn subtracts_ranges() {
        let out = subtract_ranges(&[(0.0, 10.0)], &[(2.0, 3.0), (8.0, 12.0)]);
        assert_eq!(out, vec![(0.0, 2.0), (3.0, 8.0)]);
    }

    #[test]
    fn applies_manual_edits_and_options() {
        let meta: JobMetadata = serde_json::from_str(
            r#"{
              "script": "00:00:01 - Intro\n00:00:03 - Point",
              "manual_instructions": {
                "cuts": [{"start": "00:00:04", "end": "00:00:06", "action": "remove"}],
                "effects": [{"timestamp": "00:00:02", "type": "zoom_in"}]
              },
              "options": {"auto_sound_effects": false, "generate_bgm": false, "format": "horizontal"}
            }"#,
        )
        .unwrap();
        let mut plan = plan_with(&[(0.0, 10.0)]);
        apply_metadata(&mut plan, &meta, 10.0);
        assert_eq!(plan.cut_ranges(), vec![(0.0, 4.0), (6.0, 10.0)]);
        assert!(!plan.options.bgm && !plan.options.sound_effects);
        assert_eq!(plan.format.as_deref(), Some("horizontal"));
        assert_eq!(plan.telops.len(), 2);
        assert_eq!(plan.telops[0].end, "00:00:03.000");
        assert_eq!(plan.visual_effects[0].effect_type, "zoom_in");
    }

    #[test]
    fn keep_cuts_replace_analysis() {
        let meta: JobMetadata = serde_json::from_str(
            r#"{"manual_instructions": {"cuts": [
                {"start": "00:00:05", "end": "00:00:07", "action": "keep"},
                {"start": "00:00:01", "end": "00:00:02", "action": "keep"}]}}"#,
        )
        .unwrap();
        let mut plan = plan_with(&[(0.0, 10.0)]);
        apply_metadata(&mut plan, &meta, 10.0);
        assert_eq!(plan.cut_ranges(), vec![(1.0, 2.0), (5.0, 7.0)]);
    }
}
