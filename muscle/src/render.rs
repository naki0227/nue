//! FFmpeg renderer: edit plan -> finished video, thumbnail and SRT.
//!
//! Each cut is rendered as its own segment (reframe, zoom/pan, icons, text),
//! then segments are concatenated and the soundtrack (voice, ducked BGM,
//! sound effects) is mixed on the edited timeline.

use crate::assets;
use crate::jobs::{data_path, log_json, update_job_status, JobUrls};
use crate::media::{self, MediaInfo};
use crate::plan::{parse_time, source_to_output, CaptionStyle, EditPlan};
use crate::subtitles;
use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_SOUND_EFFECTS: usize = 48;
/// Short fades at every cut boundary so jump cuts never click.
const CUT_FADE: f64 = 0.012;

/// Output frame rate: the source rate snapped to a standard rate, at most 60.
pub fn output_fps(source: f64) -> &'static str {
    const RATES: [(f64, &str); 8] = [
        (23.976, "24000/1001"),
        (24.0, "24"),
        (25.0, "25"),
        (29.97, "30000/1001"),
        (30.0, "30"),
        (50.0, "50"),
        (59.94, "60000/1001"),
        (60.0, "60"),
    ];
    if source.is_nan() || source <= 1.0 {
        return "30";
    }
    let target = source.min(60.0);
    RATES
        .iter()
        .min_by(|a, b| (a.0 - target).abs().total_cmp(&(b.0 - target).abs()))
        .map(|r| r.1)
        .unwrap_or("30")
}

/// How the source is turned into output frames, shared by every segment.
struct VideoSettings {
    frame: Frame,
    fps: &'static str,
    /// Filters applied to the raw source before reframing (HDR tone mapping).
    source_filter: String,
    /// Mild sharpening when reframing enlarges the picture noticeably (opt-in:
    /// it looks crisper but strays from the source).
    sharpen: bool,
}

impl VideoSettings {
    fn new(info: &MediaInfo, frame: Frame) -> VideoSettings {
        // iPhone footage is usually HLG/Dolby Vision. Tone map to SDR BT.709 so it
        // does not come out washed out on players that assume SDR.
        let source_filter = if info.is_hdr() {
            format!(
                "zscale=tin={}:min=bt2020nc:pin=bt2020:t=linear:npl=100,format=gbrpf32le,\
                 zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p,",
                info.color_transfer
            )
        } else {
            String::new()
        };
        let (sw, sh) = info.display_size();
        let enlargement = if sw > 0 && sh > 0 {
            (frame.width as f64 / sw as f64).max(frame.height as f64 / sh as f64)
        } else {
            1.0
        };
        VideoSettings {
            frame,
            fps: output_fps(info.fps),
            source_filter,
            sharpen: enlargement > 1.15 && env::var("NUE_SHARPEN").is_ok_and(|v| v == "1"),
        }
    }

    /// Scales and crops `[input]` to the frame at `zoom`, focused horizontally.
    fn reframe(&self, input: &str, output: &str, x_expr: &str, zoom: f64) -> String {
        let (w, h) = (self.frame.width, self.frame.height);
        let even = |v: f64| ((v / 2.0).round() as u32) * 2;
        let (zw, zh) = (even(w as f64 * zoom), even(h as f64 * zoom));
        format!(
            "[{input}]scale={zw}:{zh}:force_original_aspect_ratio=increase:flags=lanczos,\
             crop={w}:{h}:x={}:y=(ih-oh)/2{}[{output}]",
            quote(x_expr),
            if self.sharpen { ",cas=0.35" } else { "" }
        )
    }
}

/// Encoder settings: visually lossless H.264 tagged as BT.709.
fn video_codec_args() -> Vec<String> {
    let preset = env::var("NUE_X264_PRESET").unwrap_or_else(|_| "medium".to_string());
    let crf = env::var("NUE_CRF").unwrap_or_else(|_| "18".to_string());
    [
        "-c:v",
        "libx264",
        "-preset",
        &preset,
        "-crf",
        &crf,
        "-profile:v",
        "high",
        "-pix_fmt",
        "yuv420p",
        "-colorspace",
        "bt709",
        "-color_primaries",
        "bt709",
        "-color_trc",
        "bt709",
        "-color_range",
        "tv",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
}

impl Frame {
    pub fn for_format(format: Option<&str>) -> Frame {
        match format.unwrap_or("vertical") {
            "horizontal" | "landscape" | "16:9" => Frame {
                width: 1920,
                height: 1080,
            },
            "square" | "1:1" => Frame {
                width: 1080,
                height: 1080,
            },
            _ => Frame {
                width: 1080,
                height: 1920,
            },
        }
    }

    fn is_landscape(&self) -> bool {
        self.width > self.height
    }

    /// Font size scaled to the short edge of the frame.
    fn font(&self, base_at_1080: f64) -> u32 {
        (base_at_1080 * self.width.min(self.height) as f64 / 1080.0).round() as u32
    }
}

pub struct RenderOutput {
    pub video: PathBuf,
    pub thumbnail: Option<PathBuf>,
    pub subtitles: Option<PathBuf>,
    pub plan: Option<PathBuf>,
}

/// Quotes a value for an ffmpeg filtergraph option.
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "/").replace('\'', "'\\''"))
}

fn color(name: Option<&str>) -> &'static str {
    match name.unwrap_or("white").to_lowercase().as_str() {
        "yellow" => "#FFE14D",
        "cyan" | "blue" => "#4DE8FF",
        "red" => "#FF4D4D",
        "pink" => "#FF6FB5",
        "green" => "#5CFF8D",
        "orange" => "#FFA53D",
        "black" => "#111111",
        _ => "white",
    }
}

/// Character width in half-width units (CJK and other wide glyphs count 2).
fn char_units(c: char) -> usize {
    if c.is_ascii() {
        1
    } else {
        2
    }
}

/// Wraps text to `max_units`, preferring spaces for Latin text and allowing a
/// break anywhere in CJK text. Japanese closing punctuation never starts a line.
pub fn wrap_text(text: &str, max_units: usize, max_lines: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut units = 0;
    let mut last_space: Option<usize> = None;
    for c in text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
    {
        let w = char_units(c);
        let forbidden_start = "、。，．・ー」』）！？!?,.".contains(c);
        if units + w > max_units && !line.is_empty() && !forbidden_start {
            if let Some(idx) = last_space.filter(|_| c != ' ' && c.is_ascii()) {
                let rest = line[idx + 1..].to_string();
                line.truncate(idx);
                lines.push(line.trim_end().to_string());
                units = rest.chars().map(char_units).sum();
                line = rest;
            } else {
                lines.push(line.trim_end().to_string());
                line = String::new();
                units = 0;
            }
            last_space = None;
            if c == ' ' {
                continue;
            }
        }
        if c == ' ' {
            last_space = Some(line.len());
        }
        line.push(c);
        units += w;
    }
    if !line.trim().is_empty() {
        lines.push(line.trim().to_string());
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            last.push('…');
        }
    }
    lines.join("\n")
}

struct TextLayer {
    text: String,
    font_size: u32,
    color: &'static str,
    y: String,
    boxed: bool,
    border: u32,
    window: Option<(f64, f64)>,
}

fn text_layer_y(position: Option<&str>, frame: Frame) -> String {
    match position.unwrap_or("bottom") {
        "top" => "h*0.13".to_string(),
        "center" | "middle" => "(h-text_h)/2".to_string(),
        _ if frame.is_landscape() => "h*0.80-text_h".to_string(),
        _ => "h*0.66-text_h".to_string(),
    }
}

fn styled_layer(
    text: &str,
    style: Option<&CaptionStyle>,
    frame: Frame,
    window: Option<(f64, f64)>,
) -> TextLayer {
    let font_size = frame.font(84.0);
    let max_units = (frame.width as f64 * 0.88 / (font_size as f64 * 0.5)) as usize;
    TextLayer {
        text: wrap_text(text, max_units, 3),
        font_size,
        color: color(style.and_then(|s| s.color.as_deref())),
        y: text_layer_y(style.and_then(|s| s.position.as_deref()), frame),
        boxed: style.and_then(|s| s.start_box).unwrap_or(false),
        border: (font_size / 14).max(2),
        window,
    }
}

fn subtitle_layer(text: &str, frame: Frame, window: (f64, f64)) -> TextLayer {
    let font_size = frame.font(58.0);
    let max_units = (frame.width as f64 * 0.9 / (font_size as f64 * 0.5)) as usize;
    TextLayer {
        text: wrap_text(text, max_units, 2),
        font_size,
        color: "white",
        y: if frame.is_landscape() {
            "h*0.95-text_h".to_string()
        } else {
            "h*0.84-text_h".to_string()
        },
        boxed: true,
        border: (font_size / 14).max(2),
        window: Some(window),
    }
}

fn drawtext(layer: &TextLayer, text_file: &Path) -> String {
    let mut filter = format!(
        "drawtext=fontfile={}:textfile={}:expansion=none:fontsize={}:fontcolor={}:\
         borderw={}:bordercolor=black@0.9:line_spacing={}:x=(w-text_w)/2:y={}",
        quote(assets::font_path()),
        quote(&text_file.to_string_lossy()),
        layer.font_size,
        layer.color,
        layer.border,
        layer.font_size / 5,
        quote(&layer.y),
    );
    if layer.boxed {
        filter.push_str(&format!(
            ":box=1:boxcolor=black@0.55:boxborderw={}",
            layer.font_size / 4
        ));
    }
    if let Some((a, b)) = layer.window {
        filter.push_str(&format!(
            ":enable={}",
            quote(&format!("between(t,{:.3},{:.3})", a, b))
        ));
    }
    filter
}

/// Clips a source-time window to a cut and converts it to segment-local time.
fn local_window(
    start: &str,
    end: Option<&str>,
    default_len: f64,
    cut: (f64, f64),
) -> Option<(f64, f64)> {
    let s = parse_time(start).ok()?;
    let e = end
        .and_then(|e| parse_time(e).ok())
        .filter(|e| *e > s)
        .unwrap_or(s + default_len);
    let a = s.max(cut.0);
    let b = e.min(cut.1);
    (b - a > 0.05).then_some((a - cut.0, b - cut.0))
}

fn overlay_xy(position: Option<&str>) -> (&'static str, &'static str) {
    match position.unwrap_or("top_right") {
        "top_left" => ("main_w*0.05", "main_h*0.24"),
        "bottom_left" => ("main_w*0.05", "main_h*0.70-overlay_h"),
        "bottom_right" => ("main_w*0.95-overlay_w", "main_h*0.70-overlay_h"),
        "center" => ("(main_w-overlay_w)/2", "(main_h-overlay_h)/2"),
        _ => ("main_w*0.95-overlay_w", "main_h*0.24"),
    }
}

struct Segment<'a> {
    index: usize,
    range: (f64, f64),
    focus: f64,
    caption: Option<(&'a str, Option<&'a CaptionStyle>)>,
}

fn render_segment(
    plan: &EditPlan,
    segment: &Segment,
    video_path: &Path,
    has_audio: bool,
    video: &VideoSettings,
    work_dir: &Path,
) -> Result<PathBuf> {
    let (cut_start, cut_end) = segment.range;
    let duration = cut_end - cut_start;
    // Matroska with PCM audio: the only lossy audio encode happens once, at the end.
    let segment_path = work_dir.join(format!("seg_{:04}.mkv", segment.index));
    let frame = video.frame;
    let w = frame.width;

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-hide_banner", "-loglevel", "error"])
        .arg("-ss")
        .arg(format!("{:.3}", cut_start))
        .arg("-t")
        .arg(format!("{:.3}", duration))
        .arg("-i")
        .arg(video_path);
    let mut next_input = 1;

    // Reframe; pans move the crop window over time.
    let mut x_expr = format!("(iw-ow)*{:.3}", segment.focus);
    for effect in &plan.visual_effects {
        let direction = match effect.effect_type.as_str() {
            "pan_right" => 1.0,
            "pan_left" => -1.0,
            _ => continue,
        };
        if let Some((a, b)) = local_window(&effect.start, effect.end.as_deref(), 2.0, segment.range)
        {
            x_expr = format!(
                "if(lt(t,{a:.3}),{base},(iw-ow)*clip({f:.3}+{d}*0.5*min((t-{a:.3})/{len:.3},1),0,1))",
                base = x_expr,
                f = segment.focus,
                d = direction,
                len = (b - a).max(0.1),
            );
        }
    }
    // Zoomed views are cut from the full-resolution source, not by enlarging the
    // already-reframed picture, so punch-ins stay sharp.
    let zooms: Vec<(f64, f64, f64)> = plan
        .visual_effects
        .iter()
        .filter_map(|effect| {
            let zoom = match effect.effect_type.as_str() {
                "zoom_in" => 1.25,
                "zoom_out" => 1.1,
                _ => return None,
            };
            local_window(&effect.start, effect.end.as_deref(), 1.5, segment.range)
                .map(|(a, b)| (zoom, a, b))
        })
        .collect();
    let branches = (0..=zooms.len())
        .map(|i| format!("[s{i}]"))
        .collect::<String>();
    let mut graph = vec![
        format!(
            "[0:v]{}fps={},setsar=1,split={}{branches}",
            video.source_filter,
            video.fps,
            zooms.len() + 1
        ),
        video.reframe("s0", "v0", &x_expr, 1.0),
    ];
    let mut label = 0;
    let focus_expr = format!("(iw-ow)*{:.3}", segment.focus);
    for (i, (zoom, a, b)) in zooms.iter().enumerate() {
        graph.push(video.reframe(
            &format!("s{}", i + 1),
            &format!("z{label}"),
            &focus_expr,
            *zoom,
        ));
        graph.push(format!(
            "[v{l}][z{l}]overlay=enable={}[v{n}]",
            quote(&format!("between(t,{a:.3},{b:.3})")),
            l = label,
            n = label + 1,
        ));
        label += 1;
    }

    for overlay in &plan.overlays {
        let Some((a, b)) = local_window(&overlay.start, Some(&overlay.end), 1.0, segment.range)
        else {
            continue;
        };
        let Some(icon) = assets::icon(&overlay.asset) else {
            log_json(
                "WARN",
                &format!("Icon not found: {}", overlay.asset),
                Some("icon_missing"),
                None,
            );
            continue;
        };
        cmd.args(["-loop", "1", "-framerate", video.fps, "-t"])
            .arg(format!("{:.3}", duration))
            .arg("-i")
            .arg(icon);
        let icon_w = (w as f64 * overlay.scale.unwrap_or(0.18).clamp(0.05, 0.8)).round() as u32;
        let (x, y) = overlay_xy(overlay.position.as_deref());
        graph.push(format!(
            "[{input}:v]scale={icon_w}:-1,format=rgba[i{l}];[v{l}][i{l}]overlay=x={x}:y={y}:eof_action=pass:enable={}[v{n}]",
            quote(&format!("between(t,{a:.3},{b:.3})")),
            input = next_input,
            l = label,
            n = label + 1,
        ));
        next_input += 1;
        label += 1;
    }

    let mut layers = Vec::new();
    if let Some((text, style)) = segment.caption {
        layers.push(styled_layer(text, style, frame, None));
    }
    for telop in &plan.telops {
        if let Some(window) = local_window(&telop.start, Some(&telop.end), 3.0, segment.range) {
            layers.push(styled_layer(
                &telop.text,
                telop.style.as_ref(),
                frame,
                Some(window),
            ));
        }
    }
    if plan.options.subtitles {
        for line in &plan.subtitles {
            if let Some(window) = local_window(&line.start, Some(&line.end), 2.0, segment.range)
                .filter(|(a, b)| b - a >= 0.3)
            {
                layers.push(subtitle_layer(&line.text, frame, window));
            }
        }
    }
    let mut text_filters = Vec::new();
    for (i, layer) in layers.iter().enumerate() {
        let text_file = work_dir.join(format!("seg_{:04}_text_{:03}.txt", segment.index, i));
        fs::write(&text_file, &layer.text)?;
        text_filters.push(drawtext(layer, &text_file));
    }
    if text_filters.is_empty() {
        graph.push(format!("[v{label}]null[vout]"));
    } else {
        graph.push(format!("[v{label}]{}[vout]", text_filters.join(",")));
    }

    let audio_map = if has_audio {
        let fade_out = (duration - CUT_FADE).max(0.0);
        graph.push(format!(
            "[0:a]aresample=48000,aformat=channel_layouts=stereo,apad,atrim=0:{duration:.3},\
             afade=t=in:d={CUT_FADE},afade=t=out:st={fade_out:.3}:d={CUT_FADE}[aout]"
        ));
        "[aout]".to_string()
    } else {
        cmd.args(["-f", "lavfi", "-t"])
            .arg(format!("{:.3}", duration))
            .args(["-i", "anullsrc=r=48000:cl=stereo"]);
        format!("{}:a", next_input)
    };

    let output = cmd
        .arg("-filter_complex")
        .arg(graph.join(";"))
        .args(["-map", "[vout]", "-map", &audio_map])
        .args(video_codec_args())
        .args(["-r", video.fps])
        .args(["-c:a", "pcm_s16le", "-ar", "48000", "-ac", "2"])
        .arg("-t")
        .arg(format!("{:.3}", duration))
        .arg(&segment_path)
        .output()
        .context("Failed to run ffmpeg")?;

    if !output.status.success() {
        bail!(
            "Segment {} failed: {}",
            segment.index,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(segment_path)
}

fn mix_and_concat(
    plan: &EditPlan,
    segments: &[PathBuf],
    ranges: &[(f64, f64)],
    has_audio: bool,
    work_dir: &Path,
    output_path: &Path,
) -> Result<()> {
    let concat_file = work_dir.join("concat_list.txt");
    let mut file = fs::File::create(&concat_file)?;
    for seg in segments {
        let absolute = seg.canonicalize().unwrap_or_else(|_| seg.clone());
        writeln!(
            file,
            "file '{}'",
            absolute.display().to_string().replace('\'', "'\\''")
        )?;
    }
    drop(file);

    let total = ranges
        .iter()
        .map(|(s, e)| e - s)
        .sum::<f64>()
        .min(plan.output_duration());

    // 1) Mix voice, ducked BGM and sound effects into a lossless WAV.
    let mixed = work_dir.join("mix.wav");
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
    ])
    .arg(&concat_file);

    let mood = plan.editing_style.as_ref().and_then(|s| s["mood"].as_str());
    let bgm = if plan.options.bgm {
        assets::bgm(plan.bgm_path.as_deref(), mood)
    } else {
        None
    };

    let mut sound_effects = Vec::new();
    if plan.options.sound_effects {
        for se in &plan.se_events {
            let Some(at) = parse_time(&se.timestamp)
                .ok()
                .and_then(|t| source_to_output(ranges, t))
            else {
                continue;
            };
            match assets::sound_effect(&se.event_type) {
                Some(path) => sound_effects.push((at, path)),
                None => log_json(
                    "WARN",
                    "No sound effect assets in data/se",
                    Some("se_missing"),
                    None,
                ),
            }
            if sound_effects.len() >= MAX_SOUND_EFFECTS {
                break;
            }
        }
    }

    let mut filters = Vec::new();
    let mut mix_inputs = vec!["[voice]".to_string()];
    let mut next_input = 1;
    // Clean up speech: rumble cut, gentle denoise, and compression for even levels.
    let voice_chain = if has_audio {
        "highpass=f=80,afftdn=nf=-25:tn=1,\
         acompressor=threshold=0.089:ratio=3:attack=15:release=250:makeup=1.5"
    } else {
        "anull"
    };

    if let Some(bgm) = &bgm {
        log_json("INFO", "Mixing BGM", Some("bgm"), bgm.to_str());
        cmd.args(["-stream_loop", "-1", "-i"]).arg(bgm);
        if has_audio {
            // Duck the music under speech.
            filters.push(format!("[0:a]{voice_chain},asplit=2[voice][sc]"));
            filters.push(format!(
                "[{next_input}:a]aresample=48000,aformat=channel_layouts=stereo,volume=0.25[bgmraw];\
                 [bgmraw][sc]sidechaincompress=threshold=0.02:ratio=8:attack=20:release=400[bgm]"
            ));
        } else {
            filters.push("[0:a]anull[voice]".to_string());
            filters.push(format!(
                "[{next_input}:a]aresample=48000,aformat=channel_layouts=stereo,volume=0.4[bgm]"
            ));
        }
        mix_inputs.push("[bgm]".to_string());
        next_input += 1;
    } else {
        filters.push(format!("[0:a]{voice_chain}[voice]"));
    }

    for (at, path) in &sound_effects {
        cmd.arg("-i").arg(path);
        let ms = (at * 1000.0).round() as i64;
        filters.push(format!(
            "[{next_input}:a]aresample=48000,aformat=channel_layouts=stereo,adelay={ms}:all=1,volume=0.7[se{next_input}]"
        ));
        mix_inputs.push(format!("[se{next_input}]"));
        next_input += 1;
    }

    let fade_start = (total - 1.5).max(0.0);
    filters.push(format!(
        "{}amix=inputs={}:duration=first:dropout_transition=0:normalize=0,\
         afade=t=out:st={fade_start:.3}:d=1.5[aout]",
        mix_inputs.join(""),
        mix_inputs.len()
    ));
    let output = cmd
        .arg("-filter_complex")
        .arg(filters.join(";"))
        .args(["-map", "[aout]", "-c:a", "pcm_f32le", "-ar", "48000", "-t"])
        .arg(format!("{:.3}", total))
        .arg(&mixed)
        .output()
        .context("Failed to run ffmpeg")?;
    if !output.status.success() {
        bail!(
            "Audio mix failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // 2) Measure loudness, 3) normalize to the platform target while muxing.
    let loudnorm = loudness_filter(&mixed);
    let output = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(&concat_file)
        .arg("-i")
        .arg(&mixed)
        .args(["-map", "0:v", "-map", "1:a", "-c:v", "copy", "-af"])
        .arg(format!("{loudnorm}aresample=48000"))
        .args([
            "-c:a",
            "aac",
            "-b:a",
            "256k",
            "-ac",
            "2",
            "-movflags",
            "+faststart",
            "-t",
        ])
        .arg(format!("{:.3}", total))
        .arg(output_path)
        .output()
        .context("Failed to run ffmpeg")?;
    if !output.status.success() {
        bail!(
            "Concatenation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Integrated loudness target in LUFS (YouTube, TikTok and Instagram use about -14).
fn loudness_target() -> f64 {
    env::var("NUE_LOUDNESS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(-14.0)
}

/// Second-pass `loudnorm` filter built from a first-pass measurement, or an
/// empty string for silent audio (nothing to normalize).
fn loudness_filter(audio: &Path) -> String {
    let target = loudness_target();
    let measured = Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(audio)
        .args(["-af"])
        .arg(format!(
            "loudnorm=I={target}:TP=-1:LRA=11:print_format=json"
        ))
        .args(["-f", "null", "-"])
        .output()
        .ok()
        .and_then(|out| parse_loudnorm(&String::from_utf8_lossy(&out.stderr)));
    match measured {
        Some(m) => format!(
            "loudnorm=I={target}:TP=-1:LRA=11:measured_I={}:measured_TP={}:measured_LRA={}:\
             measured_thresh={}:offset={}:linear=true,",
            m.input_i, m.input_tp, m.input_lra, m.input_thresh, m.target_offset
        ),
        None => String::new(),
    }
}

#[derive(Debug, PartialEq)]
struct LoudnessStats {
    input_i: f64,
    input_tp: f64,
    input_lra: f64,
    input_thresh: f64,
    target_offset: f64,
}

/// Reads the JSON block `loudnorm` prints; `None` for silence (-inf) or garbage.
fn parse_loudnorm(log: &str) -> Option<LoudnessStats> {
    let json: serde_json::Value =
        serde_json::from_str(&log[log.rfind('{')?..=log.rfind('}')?]).ok()?;
    let get = |key: &str| {
        json[key]
            .as_str()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite())
    };
    let stats = LoudnessStats {
        input_i: get("input_i")?,
        input_tp: get("input_tp")?,
        input_lra: get("input_lra")?,
        input_thresh: get("input_thresh")?,
        target_offset: get("target_offset")?,
    };
    (stats.input_i > -70.0).then_some(stats)
}

fn render_thumbnail(
    plan: &EditPlan,
    video_path: &Path,
    video: &VideoSettings,
    work_dir: &Path,
    out: &Path,
) -> Result<()> {
    let frame = video.frame;
    let Some(thumb) = &plan.thumbnail else {
        bail!("No thumbnail in plan");
    };
    let at = parse_time(&thumb.timestamp).unwrap_or(0.0);
    let (w, h) = (frame.width, frame.height);
    let mut filter = format!(
        "{}scale={w}:{h}:force_original_aspect_ratio=increase:flags=lanczos,crop={w}:{h},\
         eq=saturation=1.25:contrast=1.08",
        video.source_filter
    );
    if !thumb.text.trim().is_empty() {
        let font_size = frame.font(140.0);
        let max_units = (w as f64 * 0.9 / (font_size as f64 * 0.5)) as usize;
        let layer = TextLayer {
            text: wrap_text(&thumb.text, max_units, 3),
            font_size,
            color: color(thumb.color.as_deref()),
            y: "(h-text_h)/2".to_string(),
            boxed: false,
            border: (font_size / 9).max(4),
            window: None,
        };
        let text_file = work_dir.join("thumbnail.txt");
        fs::write(&text_file, &layer.text)?;
        filter.push(',');
        filter.push_str(&drawtext(&layer, &text_file));
    }
    let output = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-ss"])
        .arg(format!("{:.3}", at))
        .arg("-i")
        .arg(video_path)
        .arg("-vf")
        .arg(filter)
        .args(["-frames:v", "1", "-q:v", "2"])
        .arg(out)
        .output()?;
    if !output.status.success() {
        bail!(
            "Thumbnail failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Renders a plan whose source video lives in `data/raw`. Outputs land in
/// `data/output` as `<stem>_edited.mp4`, `<stem>_thumb.jpg`, `<stem>_edited.srt`.
pub fn render_plan(plan: &EditPlan) -> Result<RenderOutput> {
    let stem = Path::new(&plan.original_filename)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(&plan.original_filename)
        .to_string();
    let job_id = plan.job_id.clone().unwrap_or_else(|| stem.clone());
    let result = render_plan_inner(plan, &stem, &job_id);
    if let Err(error) = &result {
        log_json(
            "ERROR",
            &format!("Render failed: {:#}", error),
            Some("render_failed"),
            None,
        );
        let _ = update_job_status(
            &job_id,
            "failed",
            &format!("{:#}", error),
            100,
            JobUrls::default(),
        );
    }
    result
}

fn render_plan_inner(plan: &EditPlan, stem: &str, job_id: &str) -> Result<RenderOutput> {
    let video_path = data_path("raw").join(&plan.original_filename);
    let output_dir = data_path("output");
    let work_dir = data_path("temp").join(job_id);
    fs::create_dir_all(&output_dir)?;
    fs::create_dir_all(&work_dir)?;

    let info = media::probe(&video_path)?;
    let frame = Frame::for_format(plan.format.as_deref());
    let video = VideoSettings::new(&info, frame);
    let ranges: Vec<(f64, f64)> = plan
        .cut_ranges()
        .into_iter()
        .map(|(s, e)| (s.min(info.duration), e.min(info.duration)))
        .filter(|(s, e)| e - s > 0.05)
        .collect();
    if ranges.is_empty() {
        bail!("Edit plan has no usable cuts");
    }

    log_json(
        "INFO",
        &format!(
            "Rendering {} cuts ({}x{})",
            ranges.len(),
            frame.width,
            frame.height
        ),
        Some("render_start"),
        video_path.to_str(),
    );

    let cuts: Vec<_> = plan
        .cuts
        .iter()
        .filter(|cut| {
            matches!((parse_time(&cut.start_time), parse_time(&cut.end_time)), (Ok(s), Ok(e)) if (e.min(info.duration) - s.min(info.duration)) > 0.05)
        })
        .collect();
    let mut segments = Vec::new();
    for (index, (cut, range)) in cuts.iter().zip(&ranges).enumerate() {
        let progress = 70 + (index * 20 / ranges.len()) as i32;
        update_job_status(
            job_id,
            "rendering",
            &format!("Rendering cut {}/{}", index + 1, ranges.len()),
            progress,
            JobUrls::default(),
        )?;
        let segment = Segment {
            index,
            range: *range,
            focus: cut.focus_point.unwrap_or(0.5).clamp(0.0, 1.0),
            caption: cut
                .caption
                .as_deref()
                .filter(|c| !c.trim().is_empty())
                .map(|c| (c, cut.caption_style.as_ref())),
        };
        segments.push(render_segment(
            plan,
            &segment,
            &video_path,
            info.has_audio,
            &video,
            &work_dir,
        )?);
    }

    update_job_status(job_id, "rendering", "Mixing audio", 92, JobUrls::default())?;
    let output_name = format!("{}_edited.mp4", stem);
    let output_path = output_dir.join(&output_name);
    mix_and_concat(
        plan,
        &segments,
        &ranges,
        info.has_audio,
        &work_dir,
        &output_path,
    )?;

    let thumb_name = format!("{}_thumb.jpg", stem);
    let thumb_path = output_dir.join(&thumb_name);
    let thumbnail = match render_thumbnail(plan, &video_path, &video, &work_dir, &thumb_path) {
        Ok(()) => Some(thumb_path),
        Err(error) => {
            log_json(
                "WARN",
                &format!("{:#}", error),
                Some("thumbnail_skipped"),
                None,
            );
            None
        }
    };

    let srt_name = format!("{}_edited.srt", stem);
    let srt = subtitles::to_output_srt(&plan.subtitles, &ranges);
    let subtitles_path = if srt.is_empty() {
        None
    } else {
        let path = output_dir.join(&srt_name);
        fs::write(&path, srt)?;
        Some(path)
    };

    let plan_name = format!("{}_plan.json", stem);
    let plan_path = output_dir.join(&plan_name);
    fs::write(&plan_path, serde_json::to_string_pretty(plan)?)?;

    let output_url = format!("/output/{}", output_name);
    let thumb_url = format!("/output/{}", thumb_name);
    let srt_url = format!("/output/{}", srt_name);
    let plan_url = format!("/output/{}", plan_name);
    update_job_status(
        job_id,
        "completed",
        "Your video is ready.",
        100,
        JobUrls {
            output_url: Some(&output_url),
            thumbnail_url: thumbnail.as_ref().map(|_| thumb_url.as_str()),
            subtitle_url: subtitles_path.as_ref().map(|_| srt_url.as_str()),
            plan_url: Some(&plan_url),
        },
    )?;
    log_json(
        "INFO",
        "Render complete",
        Some("render_complete"),
        output_path.to_str(),
    );

    let _ = fs::remove_dir_all(&work_dir);
    Ok(RenderOutput {
        video: output_path,
        thumbnail,
        subtitles: subtitles_path,
        plan: Some(plan_path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_cjk_and_latin() {
        assert_eq!(
            wrap_text("あいうえおかきくけこ", 10, 3),
            "あいうえお\nかきくけこ"
        );
        assert_eq!(
            wrap_text("hello brave new world", 11, 3),
            "hello brave\nnew world"
        );
        // Closing punctuation stays on the previous line.
        assert_eq!(wrap_text("あいうえお。かき", 10, 3), "あいうえお。\nかき");
        assert_eq!(
            wrap_text("あいうえおかきくけこさしすせそ", 10, 2),
            "あいうえお\nかきくけこ…"
        );
    }

    #[test]
    fn clips_windows_to_cuts() {
        assert_eq!(
            local_window("00:00:05", Some("00:00:08"), 1.0, (6.0, 10.0)),
            Some((0.0, 2.0))
        );
        assert_eq!(local_window("00:00:01", None, 1.0, (6.0, 10.0)), None);
    }

    #[test]
    fn quotes_filter_values() {
        assert_eq!(quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn snaps_output_frame_rates() {
        assert_eq!(output_fps(59.94), "60000/1001");
        assert_eq!(output_fps(60.0), "60");
        assert_eq!(output_fps(120.0), "60");
        assert_eq!(output_fps(29.97), "30000/1001");
        assert_eq!(output_fps(24.0), "24");
        assert_eq!(output_fps(0.0), "30");
    }

    #[test]
    fn parses_loudnorm_json() {
        let log = "[Parsed_loudnorm_0 @ 0x1]\n{\n\t\"input_i\" : \"-24.80\",\n\t\"input_tp\" : \"-6.20\",\n\t\"input_lra\" : \"3.10\",\n\t\"input_thresh\" : \"-35.00\",\n\t\"target_offset\" : \"0.30\"\n}\n";
        let stats = parse_loudnorm(log).unwrap();
        assert_eq!(stats.input_i, -24.8);
        assert_eq!(stats.target_offset, 0.3);
        let silent = log.replace("-24.80", "-inf");
        assert_eq!(parse_loudnorm(&silent), None);
    }

    #[test]
    fn picks_frame_sizes() {
        assert_eq!(
            Frame::for_format(None),
            Frame {
                width: 1080,
                height: 1920
            }
        );
        assert_eq!(Frame::for_format(Some("horizontal")).font(84.0), 84);
    }
}
