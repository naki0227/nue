//! Editing formats ("styles"): everything that makes a Vlog look like a Vlog
//! and a variety show look like a variety show — editing rules, colour look,
//! transitions, caption / telop / title typography and animation, and audio.
//!
//! Built-in formats live in `presets()`. A JSON file `data/styles/<name>.json`
//! overrides or adds a format; missing fields fall back to the built-in one
//! with the same name (or to `shorts`).

use crate::jobs::data_path;
use crate::look::{Grade, Look};
use crate::motion::Transition;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Animation {
    #[default]
    None,
    Fade,
    /// Scale 0 -> overshoot -> 100% (easeOutBack).
    Pop,
    /// Pop with a second, smaller bounce.
    Bounce,
    /// Rise from below while fading in.
    SlideUp,
    /// Grow from 80% without overshoot (caption chunks).
    Grow,
    /// Characters appear one by one.
    Typewriter,
    /// From blurred and wide-tracked to sharp.
    BlurIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CaptionMode {
    /// Whole subtitle lines.
    #[default]
    Line,
    /// Short chunks of a few words, with the spoken word highlighted.
    Words,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Amount {
    None,
    Minimal,
    #[default]
    Normal,
    Lots,
}

/// Typography and animation for one kind of on-screen text. Sizes are
/// fractions of the frame's short edge so formats work at any resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextStyle {
    /// Font family name (as fontconfig/libass knows it).
    pub font: String,
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub uppercase: bool,
    /// Extra letter spacing as a fraction of the size.
    pub spacing: f64,
    /// Colours as #RRGGBB or #RRGGBBAA (AA = opacity).
    pub color: String,
    pub highlight: String,
    pub outline: String,
    /// Outline width as a fraction of the size (0 = none).
    pub outline_width: f64,
    /// Optional second, outer outline (Japanese variety telops).
    pub outer_outline: Option<String>,
    pub outer_outline_width: f64,
    pub shadow: f64,
    pub shadow_color: String,
    /// Softness of outline/shadow edges as a fraction of the size (soft
    /// drop shadows for vlog captions instead of hard outlines).
    pub blur: f64,
    /// Opaque box behind the text (e.g. "#000000AA"), or none.
    pub background: Option<String>,
    /// Vertical anchor of the text block, 0 = top edge .. 1 = bottom edge.
    pub position: f64,
    /// Anchor used for portrait (9:16) frames, where the bottom ~35% is
    /// covered by the TikTok / Reels / Shorts interface.
    pub position_portrait: Option<f64>,
    /// left | center | right
    pub align: String,
    pub enter: Animation,
    pub exit: Animation,
    /// Entrance duration in milliseconds.
    pub enter_ms: u32,
    pub exit_ms: u32,
    pub mode: CaptionMode,
    /// Max width of a caption chunk in half-width units (CJK char = 2).
    pub chunk_units: usize,
    pub max_lines: usize,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            font: "Noto Sans CJK JP".to_string(),
            size: 0.066,
            bold: true,
            italic: false,
            uppercase: false,
            spacing: 0.0,
            color: "#FFFFFF".to_string(),
            highlight: "#FFE14D".to_string(),
            outline: "#000000".to_string(),
            outline_width: 0.12,
            outer_outline: None,
            outer_outline_width: 0.0,
            shadow: 0.0,
            shadow_color: "#00000080".to_string(),
            blur: 0.0,
            background: None,
            position: 0.86,
            position_portrait: Some(0.64),
            align: "center".to_string(),
            enter: Animation::Fade,
            exit: Animation::Fade,
            enter_ms: 120,
            exit_ms: 100,
            mode: CaptionMode::Line,
            chunk_units: 24,
            max_lines: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TitleKind {
    None,
    /// Big hook line over the first seconds.
    #[default]
    Hook,
    /// Vlog title card: title plus a small date line.
    Card,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TitleStyle {
    pub kind: TitleKind,
    pub text: TextStyle,
    /// Small line under a card title (date).
    pub caption: TextStyle,
    pub show_date: bool,
    pub duration: f64,
}

impl Default for TitleStyle {
    fn default() -> Self {
        TitleStyle {
            kind: TitleKind::Hook,
            text: TextStyle {
                size: 0.11,
                color: "#FFE14D".to_string(),
                outline_width: 0.1,
                position: 0.42,
                enter: Animation::Pop,
                enter_ms: 300,
                ..TextStyle::default()
            },
            caption: TextStyle::default(),
            show_date: false,
            duration: 2.5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditRules {
    pub jump_cut: bool,
    pub remove_fillers: bool,
    /// Alternate framing between jump cuts.
    pub punch_in: bool,
    /// Slow push-in on long takes.
    pub push_in: bool,
    /// Zoom of the alternating jump-cut framing.
    pub punch_amount: f64,
    /// Zoom amount on emphasis (1 = no zoom).
    pub emphasis_zoom: f64,
    /// Camera shake on the strongest emphasis.
    pub shake: bool,
    pub telops: Amount,
    pub sound_effects: Amount,
    pub icons: bool,
}

impl Default for EditRules {
    fn default() -> Self {
        EditRules {
            jump_cut: true,
            remove_fillers: true,
            punch_in: true,
            push_in: true,
            punch_amount: 1.12,
            emphasis_zoom: 1.25,
            shake: false,
            telops: Amount::Normal,
            sound_effects: Amount::Normal,
            icons: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransitionRules {
    /// Between different scenes / takes far apart.
    pub scene: Transition,
    /// Between jump cuts inside one take.
    pub jump: Transition,
    pub duration: f64,
    /// Into the first shot and out of the last one.
    pub opening: Transition,
    pub ending: Transition,
    /// Lengths of the opening and ending transitions (seconds).
    pub opening_duration: f64,
    pub ending_duration: f64,
    /// Whoosh on camera transitions.
    pub whoosh: bool,
}

impl Default for TransitionRules {
    fn default() -> Self {
        TransitionRules {
            scene: Transition::Zoom,
            jump: Transition::Cut,
            duration: 0.3,
            opening: Transition::Cut,
            ending: Transition::Cut,
            opening_duration: 0.6,
            ending_duration: 1.0,
            whoosh: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioRules {
    /// Preferred BGM mood (file name keyword in data/bgm).
    pub bgm_mood: Option<String>,
    pub bgm_volume: f64,
    pub sound_effect_volume: f64,
}

impl Default for AudioRules {
    fn default() -> Self {
        AudioRules {
            bgm_mood: None,
            bgm_volume: 0.25,
            sound_effect_volume: 0.7,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    pub name: String,
    pub label: String,
    pub description: String,
    pub edit: EditRules,
    pub look: Look,
    pub transitions: TransitionRules,
    pub captions: TextStyle,
    pub telops: TextStyle,
    pub title: TitleStyle,
    pub audio: AudioRules,
}

impl Default for Style {
    fn default() -> Self {
        shorts()
    }
}

fn text(font: &str, size: f64) -> TextStyle {
    TextStyle {
        font: font.to_string(),
        size,
        ..TextStyle::default()
    }
}

fn hex3(hex: &str, strength: f64) -> [f64; 3] {
    // Tint direction from a colour: its offset from neutral grey.
    let h = hex.trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(128) as f64 / 255.0 - 0.5;
    [c(0) * strength, c(2) * strength, c(4) * strength]
}

/// Short-form talking video (TikTok / Shorts / Reels): word-by-word captions
/// above the app UI, punch-ins between jump cuts, a hook title, pops and
/// whooshes. Numbers follow the Remotion TikTok template and CapCut-style
/// "Hormozi" captions (see docs/editing-formats.md).
pub fn shorts() -> Style {
    Style {
        name: "shorts".to_string(),
        label: "ショート動画".to_string(),
        description: "1語ずつ光る大きな字幕、テンポの良いカットとズーム、冒頭のつかみ".to_string(),
        edit: EditRules::default(),
        look: Look {
            grade: Grade {
                contrast: 1.05,
                saturation: 1.06,
                ..Grade::default()
            },
            ..Look::default()
        },
        transitions: TransitionRules {
            scene: Transition::Zoom,
            duration: 0.25,
            ..TransitionRules::default()
        },
        captions: TextStyle {
            size: 0.095,
            highlight: "#FFE600".to_string(),
            outline_width: 0.14,
            position: 0.80,
            position_portrait: Some(0.62),
            enter: Animation::Grow,
            enter_ms: 160,
            exit: Animation::None,
            mode: CaptionMode::Words,
            chunk_units: 18,
            ..text("Rounded Mplus 1c ExtraBold", 0.095)
        },
        telops: TextStyle {
            color: "#FFE600".to_string(),
            outline_width: 0.12,
            position: 0.16,
            position_portrait: Some(0.2),
            enter: Animation::Pop,
            enter_ms: 230,
            exit_ms: 120,
            max_lines: 2,
            ..text("Dela Gothic One", 0.085)
        },
        title: TitleStyle {
            kind: TitleKind::Hook,
            text: TextStyle {
                color: "#FFE600".to_string(),
                outline_width: 0.12,
                position: 0.42,
                position_portrait: Some(0.3),
                enter: Animation::Pop,
                enter_ms: 300,
                ..text("Dela Gothic One", 0.11)
            },
            ..TitleStyle::default()
        },
        audio: AudioRules::default(),
    }
}

/// Everyday / travel vlog: matte warm film look with grain, soft quiet
/// captions without outlines, a mincho title card with the date, dips to
/// white for time skips and fades from/to black; no zooms or sound effects.
pub fn vlog() -> Style {
    let soft = |font: &str, size: f64| TextStyle {
        color: "#FFFFFFF0".to_string(),
        outline: "#00000059".to_string(),
        outline_width: 0.06,
        blur: 0.08,
        enter: Animation::Fade,
        enter_ms: 300,
        exit_ms: 300,
        ..text(font, size)
    };
    Style {
        name: "vlog".to_string(),
        label: "Vlog".to_string(),
        description: "フィルム風のくすんだ色、静かな字幕、日付入りのタイトル、ゆったりした転換"
            .to_string(),
        edit: EditRules {
            punch_in: false,
            push_in: true,
            emphasis_zoom: 1.0,
            telops: Amount::Minimal,
            sound_effects: Amount::None,
            icons: false,
            ..EditRules::default()
        },
        look: Look {
            grade: Grade {
                contrast: 0.88,
                saturation: 0.88,
                temperature: 0.35,
                fade: 0.06,
                white: 0.94,
                shadow_tint: hex3("#EBCFB2", 1.0),
                highlight_tint: hex3("#FBE7C6", 1.0),
                split: 0.5,
                ..Grade::default()
            },
            grain: 6,
            vignette: 0.25,
            glow: 0.12,
            ..Look::default()
        },
        transitions: TransitionRules {
            scene: Transition::DipWhite,
            jump: Transition::Cut,
            duration: 0.4,
            opening: Transition::DipBlack,
            ending: Transition::DipBlack,
            opening_duration: 1.0,
            ending_duration: 1.5,
            whoosh: false,
        },
        captions: TextStyle {
            position: 0.88,
            position_portrait: Some(0.68),
            ..soft("Zen Maru Gothic Medium", 0.044)
        },
        telops: TextStyle {
            position: 0.14,
            position_portrait: Some(0.2),
            enter: Animation::SlideUp,
            enter_ms: 450,
            ..soft("Zen Maru Gothic Bold", 0.052)
        },
        title: TitleStyle {
            kind: TitleKind::Card,
            text: TextStyle {
                align: "left".to_string(),
                spacing: 0.08,
                position: 0.72,
                position_portrait: Some(0.56),
                enter: Animation::BlurIn,
                enter_ms: 900,
                exit_ms: 500,
                ..soft("Zen Old Mincho SemiBold", 0.075)
            },
            caption: TextStyle {
                color: "#FFB347".to_string(),
                align: "left".to_string(),
                position: 0.8,
                position_portrait: Some(0.61),
                enter: Animation::Fade,
                enter_ms: 600,
                exit_ms: 500,
                ..soft("Courier Prime", 0.032)
            },
            show_date: true,
            duration: 3.5,
        },
        audio: AudioRules {
            bgm_mood: Some("chill".to_string()),
            bgm_volume: 0.18,
            sound_effect_volume: 0.5,
        },
    }
}

/// Japanese variety / YouTuber edit: double-outlined telops that stamp in,
/// reaction zooms with shake, a sound on every telop, tight jump cuts.
pub fn variety() -> Style {
    Style {
        name: "variety".to_string(),
        label: "バラエティ".to_string(),
        description: "二重フチのテロップ、リアクションズーム＋揺れ、効果音多め、詰めたカット"
            .to_string(),
        edit: EditRules {
            punch_in: true,
            push_in: false,
            emphasis_zoom: 1.35,
            shake: true,
            telops: Amount::Lots,
            sound_effects: Amount::Lots,
            icons: true,
            ..EditRules::default()
        },
        look: Look {
            grade: Grade {
                contrast: 1.06,
                saturation: 1.12,
                ..Grade::default()
            },
            ..Look::default()
        },
        transitions: TransitionRules {
            scene: Transition::Zoom,
            duration: 0.2,
            ..TransitionRules::default()
        },
        captions: TextStyle {
            outline: "#111111".to_string(),
            outline_width: 0.12,
            position: 0.86,
            position_portrait: Some(0.64),
            enter: Animation::Pop,
            enter_ms: 180,
            exit: Animation::None,
            ..text("Rounded Mplus 1c Black", 0.08)
        },
        telops: TextStyle {
            color: "#FFF000".to_string(),
            outline: "#000000".to_string(),
            outline_width: 0.08,
            outer_outline: Some("#FFFFFF".to_string()),
            outer_outline_width: 0.1,
            position: 0.2,
            position_portrait: Some(0.22),
            enter: Animation::Bounce,
            enter_ms: 260,
            exit: Animation::None,
            ..text("Dela Gothic One", 0.1)
        },
        title: TitleStyle {
            kind: TitleKind::Hook,
            text: TextStyle {
                color: "#FFFFFF".to_string(),
                outline: "#E60033".to_string(),
                outline_width: 0.08,
                outer_outline: Some("#FFFFFF".to_string()),
                outer_outline_width: 0.1,
                position: 0.42,
                position_portrait: Some(0.3),
                enter: Animation::Bounce,
                enter_ms: 320,
                ..text("Dela Gothic One", 0.12)
            },
            ..TitleStyle::default()
        },
        audio: AudioRules {
            bgm_mood: Some("energetic".to_string()),
            bgm_volume: 0.25,
            sound_effect_volume: 0.85,
        },
    }
}

/// Cinematic / "emo": 2.39:1 letterbox, teal & orange with halation and
/// grain, long fades, a letter-spaced mincho title that blurs in, small
/// ivory subtitles, no sound effects.
pub fn cinematic() -> Style {
    let ivory = |font: &str, size: f64| TextStyle {
        color: "#F0EBD8".to_string(),
        outline: "#00000080".to_string(),
        outline_width: 0.05,
        blur: 0.06,
        enter: Animation::Fade,
        enter_ms: 400,
        exit_ms: 400,
        ..text(font, size)
    };
    Style {
        name: "cinematic".to_string(),
        label: "シネマティック".to_string(),
        description:
            "シネスコの黒帯、ティール＆オレンジ、粒子とにじみ、長いフェード、字間の広い明朝タイトル"
                .to_string(),
        edit: EditRules {
            punch_in: false,
            push_in: true,
            emphasis_zoom: 1.0,
            telops: Amount::None,
            sound_effects: Amount::None,
            icons: false,
            ..EditRules::default()
        },
        look: Look {
            grade: Grade {
                contrast: 1.08,
                saturation: 0.86,
                fade: 0.04,
                white: 0.96,
                shadow_tint: hex3("#1F4E5A", 1.6),
                highlight_tint: hex3("#F2A65A", 1.4),
                split: 0.7,
                ..Grade::default()
            },
            grain: 9,
            vignette: 0.35,
            letterbox: 2.39,
            glow: 0.22,
        },
        transitions: TransitionRules {
            scene: Transition::DipBlack,
            jump: Transition::Cut,
            duration: 0.6,
            opening: Transition::DipBlack,
            ending: Transition::DipBlack,
            opening_duration: 2.0,
            ending_duration: 2.5,
            whoosh: false,
        },
        captions: TextStyle {
            position: 0.84,
            position_portrait: Some(0.7),
            ..ivory("Zen Old Mincho Regular", 0.036)
        },
        telops: TextStyle {
            position: 0.5,
            position_portrait: Some(0.45),
            ..ivory("Zen Old Mincho SemiBold", 0.048)
        },
        title: TitleStyle {
            kind: TitleKind::Card,
            text: TextStyle {
                spacing: 0.3,
                position: 0.5,
                position_portrait: Some(0.45),
                enter: Animation::BlurIn,
                enter_ms: 1200,
                exit_ms: 1000,
                ..ivory("Zen Old Mincho SemiBold", 0.052)
            },
            caption: ivory("Zen Old Mincho Regular", 0.03),
            show_date: false,
            duration: 3.5,
        },
        audio: AudioRules {
            bgm_mood: Some("cinematic".to_string()),
            bgm_volume: 0.3,
            sound_effect_volume: 0.0,
        },
    }
}

/// Business / explainer: clean boxed captions, white key-point telops with a
/// navy title bar, gentle punch-ins, slide transitions, almost no sound.
pub fn business() -> Style {
    Style {
        name: "business".to_string(),
        label: "ビジネス・解説".to_string(),
        description: "読みやすい帯付き字幕、要点テロップ、控えめな動き".to_string(),
        edit: EditRules {
            punch_in: true,
            punch_amount: 1.08,
            push_in: false,
            emphasis_zoom: 1.0,
            telops: Amount::Normal,
            sound_effects: Amount::Minimal,
            icons: false,
            ..EditRules::default()
        },
        look: Look::default(),
        transitions: TransitionRules {
            scene: Transition::Whip,
            duration: 0.35,
            ..TransitionRules::default()
        },
        captions: TextStyle {
            background: Some("#000000A6".to_string()),
            outline_width: 0.0,
            position: 0.88,
            position_portrait: Some(0.64),
            enter: Animation::Fade,
            enter_ms: 200,
            exit_ms: 150,
            ..text("BIZ UDPGothic Bold", 0.048)
        },
        telops: TextStyle {
            color: "#1A1A1A".to_string(),
            background: Some("#FFFFFFEB".to_string()),
            outline_width: 0.0,
            position: 0.14,
            position_portrait: Some(0.2),
            enter: Animation::SlideUp,
            enter_ms: 300,
            ..text("Zen Kaku Gothic New Bold", 0.056)
        },
        title: TitleStyle {
            kind: TitleKind::Card,
            text: TextStyle {
                background: Some("#1E3A5FE6".to_string()),
                outline_width: 0.0,
                align: "left".to_string(),
                position: 0.78,
                position_portrait: Some(0.58),
                enter: Animation::SlideUp,
                enter_ms: 350,
                ..text("Zen Kaku Gothic New Bold", 0.06)
            },
            show_date: false,
            ..TitleStyle::default()
        },
        audio: AudioRules {
            bgm_mood: Some("calm".to_string()),
            bgm_volume: 0.15,
            sound_effect_volume: 0.4,
        },
    }
}

/// Built-in formats.
pub fn presets() -> Vec<Style> {
    vec![shorts(), vlog(), variety(), cinematic(), business()]
}

/// Loads a format by name: `data/styles/<name>.json` merged over the
/// built-in format named by its `"base"` field (default: the built-in of the
/// same name, else `shorts`).
pub fn load(name: Option<&str>) -> Style {
    let name = name.unwrap_or("shorts");
    let builtin = |n: &str| presets().into_iter().find(|s| s.name == n);
    let path = data_path("styles").join(format!("{name}.json"));
    let Ok(text) = fs::read_to_string(&path) else {
        return builtin(name).unwrap_or_else(shorts);
    };
    let base_name = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v["base"].as_str().map(str::to_string));
    let base = base_name
        .as_deref()
        .and_then(builtin)
        .or_else(|| builtin(name))
        .unwrap_or_else(shorts);
    merge(&base, &text).unwrap_or(base)
}

/// Applies a (partial) JSON format on top of `base`.
pub fn merge(base: &Style, json: &str) -> Option<Style> {
    let mut value = serde_json::to_value(base).ok()?;
    let patch: serde_json::Value = serde_json::from_str(json).ok()?;
    merge_values(&mut value, patch);
    serde_json::from_value(value).ok()
}

fn merge_values(target: &mut serde_json::Value, patch: serde_json::Value) {
    match (target, patch) {
        (serde_json::Value::Object(t), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                merge_values(t.entry(k).or_insert(serde_json::Value::Null), v);
            }
        }
        (t, p) => *t = p,
    }
}

/// Every available format (built-in plus `data/styles/*.json`), for UIs.
pub fn list() -> Vec<Style> {
    let mut styles = presets();
    if let Ok(entries) = fs::read_dir(data_path("styles")) {
        let mut extra: Vec<String> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| p.file_stem()?.to_str().map(str::to_string))
            .filter(|n| !styles.iter().any(|s| &s.name == n))
            .collect();
        extra.sort();
        for name in extra {
            let mut style = load(Some(&name));
            style.name = name;
            styles.push(style);
        }
    }
    styles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_partial_json_over_a_preset() {
        let base = shorts();
        let custom = merge(
            &base,
            r##"{"captions": {"color": "#00FF00"}, "look": {"grain": 6}}"##,
        )
        .unwrap();
        assert_eq!(custom.captions.color, "#00FF00");
        assert_eq!(custom.captions.font, base.captions.font);
        assert_eq!(custom.look.grain, 6);
        assert_eq!(custom.edit, base.edit);
    }

    #[test]
    fn presets_round_trip_through_json() {
        for style in presets() {
            let json = serde_json::to_string(&style).unwrap();
            let back: Style = serde_json::from_str(&json).unwrap();
            assert_eq!(back, style, "{}", style.name);
        }
    }
}
