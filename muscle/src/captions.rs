//! On-screen text as an ASS script rendered by libass: animated captions
//! (line or word-by-word with the spoken word highlighted), telops, opening
//! titles and vlog title cards, styled by the editing format.
//!
//! Scripts are written per segment with segment-local times, so text burns in
//! during the single video encode. Text that continues from the previous
//! segment is shown without replaying its entrance animation.

use crate::plan::{parse_time, EditPlan, TimedText, WordTime};
use crate::render::{max_units, wrap_text};
use crate::style::{Animation, CaptionMode, Style, TextStyle, TitleKind};
use std::fmt::Write as _;

/// Window within which consecutive words share one caption chunk (Remotion's
/// TikTok template default).
const CHUNK_WINDOW: f64 = 1.2;

/// `#RRGGBB[AA]` (AA = opacity) -> ASS `&HAABBGGRR` (AA = transparency).
pub fn ass_color(hex: &str) -> String {
    let h = hex.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("FF"), 16).unwrap_or(255);
    let (r, g, b) = (byte(0), byte(2), byte(4));
    let alpha = if h.len() >= 8 { 255 - byte(6) } else { 0 };
    format!("&H{alpha:02X}{b:02X}{g:02X}{r:02X}")
}

/// Inline colour override form `&HBBGGRR&` (alpha set separately).
fn inline_color(hex: &str) -> String {
    let full = ass_color(hex);
    format!("&H{}&", &full[4..])
}

fn ass_time(seconds: f64) -> String {
    let cs = (seconds.max(0.0) * 100.0).round() as u64;
    format!(
        "{}:{:02}:{:02}.{:02}",
        cs / 360_000,
        (cs / 6000) % 60,
        (cs / 100) % 60,
        cs % 100
    )
}

fn escape(text: &str) -> String {
    text.replace('\\', "＼")
        .replace('{', "｛")
        .replace('}', "｝")
}

/// Pixel metrics of a text style at a given frame size.
struct Metrics {
    size: f64,
    outline: f64,
    outer: f64,
    shadow: f64,
    spacing: f64,
}

fn metrics(style: &TextStyle, frame: (u32, u32)) -> Metrics {
    let short = frame.0.min(frame.1) as f64;
    let size = (style.size * short).round().max(8.0);
    Metrics {
        size,
        outline: (style.outline_width * size).round(),
        outer: (style.outer_outline_width * size).round(),
        shadow: (style.shadow * size).round(),
        spacing: (style.spacing * size).round(),
    }
}

fn style_line(name: &str, style: &TextStyle, frame: (u32, u32)) -> String {
    let m = metrics(style, frame);
    let (border_style, outline, outline_color) = match &style.background {
        // Opaque box: libass draws it in the outline colour, padded by the outline width.
        Some(bg) => (3, (m.size * 0.22).round(), ass_color(bg)),
        None => (1, m.outline, ass_color(&style.outline)),
    };
    format!(
        "Style: {name},{font},{size},{primary},{secondary},{outline_color},{back},{bold},{italic},0,0,100,100,{spacing},0,{border_style},{outline},{shadow},5,0,0,0,1",
        font = style.font,
        size = m.size,
        primary = ass_color(&style.color),
        secondary = ass_color(&style.highlight),
        back = ass_color(&style.shadow_color),
        bold = if style.bold { -1 } else { 0 },
        italic = if style.italic { -1 } else { 0 },
        spacing = m.spacing,
        shadow = m.shadow,
    )
}

/// Anchor point and `\an` alignment for a style.
fn placement(style: &TextStyle, frame: (u32, u32)) -> (u8, f64, f64) {
    let (w, h) = (frame.0 as f64, frame.1 as f64);
    let position = if frame.1 > frame.0 {
        style.position_portrait.unwrap_or(style.position)
    } else {
        style.position
    };
    let row = if position < 0.3 {
        7
    } else if position > 0.7 {
        1
    } else {
        4
    };
    let (col, x) = match style.align.as_str() {
        "left" => (0, w * 0.06),
        "right" => (2, w * 0.94),
        _ => (1, w / 2.0),
    };
    (row + col, x, position * h)
}

/// Override tags for the entrance (and exit fade) of an event `length_ms` long.
fn animation_tags(
    style: &TextStyle,
    enter: bool,
    exit_fade: bool,
    length_ms: u32,
    x: f64,
    y: f64,
    frame_h: f64,
) -> String {
    let ms = style.enter_ms.min(length_ms / 2).max(1);
    let exit = if exit_fade && style.exit == Animation::Fade {
        style.exit_ms.min(length_ms / 3)
    } else {
        0
    };
    let p = |f: f64| (ms as f64 * f).round() as u32;
    if !enter {
        return format!("\\pos({x:.0},{y:.0})\\fad(0,{exit})");
    }
    match style.enter {
        Animation::None => format!("\\pos({x:.0},{y:.0})\\fad(0,{exit})"),
        Animation::Fade => format!("\\pos({x:.0},{y:.0})\\fad({ms},{exit})"),
        // easeOutBack-like: ~18% overshoot at 57% of the duration.
        Animation::Pop => format!(
            "\\pos({x:.0},{y:.0})\\fad(40,{exit})\\fscx0\\fscy0\\t(0,{},0.5,\\fscx118\\fscy118)\\t({},{ms},1.8,\\fscx100\\fscy100)",
            p(0.57),
            p(0.57)
        ),
        Animation::Bounce => format!(
            "\\pos({x:.0},{y:.0})\\fad(40,{exit})\\fscx0\\fscy0\\t(0,{},0.5,\\fscx120\\fscy120)\\t({},{},\\fscx92\\fscy92)\\t({},{ms},\\fscx100\\fscy100)",
            p(0.45),
            p(0.45),
            p(0.72),
            p(0.72)
        ),
        Animation::Grow => format!(
            "\\pos({x:.0},{y:.0})\\fad(40,{exit})\\fscx80\\fscy80\\t(0,{ms},0.5,\\fscx100\\fscy100)"
        ),
        Animation::SlideUp => format!(
            "\\move({x:.0},{:.0},{x:.0},{y:.0},0,{ms})\\fad({},{exit})",
            y + frame_h * 0.03,
            p(0.7)
        ),
        Animation::BlurIn => format!(
            "\\pos({x:.0},{y:.0})\\fad(0,{exit})\\alpha&HFF&\\blur10\\fscx106\\fscy106\\t(0,{ms},0.4,\\alpha&H00&\\blur0\\fscx100\\fscy100)"
        ),
        // Typewriter reveals per character in `text_body`; position only here.
        Animation::Typewriter => format!("\\pos({x:.0},{y:.0})\\fad(0,{exit})"),
    }
}

/// Text body; typewriter reveals one character at a time.
fn text_body(style: &TextStyle, text: &str, enter: bool) -> String {
    if style.enter == Animation::Typewriter && enter {
        let chars: Vec<char> = text.chars().collect();
        let step = (style.enter_ms as f64 / chars.len().max(1) as f64).clamp(25.0, 90.0);
        let mut out = String::new();
        for (i, c) in chars.iter().enumerate() {
            if *c == '\n' {
                out.push_str("\\N");
                continue;
            }
            let at = (i as f64 * step).round() as u32;
            let _ = write!(
                out,
                "{{\\alpha&HFF&\\t({at},{},\\alpha&H00&)}}{}",
                at + 1,
                escape(&c.to_string())
            );
        }
        out
    } else {
        escape(text).replace('\n', "\\N")
    }
}

struct Writer {
    frame: (u32, u32),
    events: String,
}

impl Writer {
    /// Adds one text event (two layers when the style has an outer outline).
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        name: &str,
        style: &TextStyle,
        text: &str,
        start: f64,
        end: f64,
        enter: bool,
        color: Option<&str>,
    ) {
        if end - start < 0.05 || text.trim().is_empty() {
            return;
        }
        let text = if style.uppercase {
            text.to_uppercase()
        } else {
            text.to_string()
        };
        // Long text shrinks (down to 60%) instead of being cut off with "…".
        let width = self.frame.0 as f64 * 0.86;
        let lines = style.max_lines.max(1);
        let fit = fit_scale(&text, width, metrics(style, self.frame).size, lines);
        let scaled = TextStyle {
            size: style.size * fit,
            ..style.clone()
        };
        let m = metrics(&scaled, self.frame);
        let wrapped = wrap_text(&text, max_units(width, m.size as u32, &text), lines);
        let (an, x, y) = placement(style, self.frame);
        let length_ms = ((end - start) * 1000.0).round() as u32;
        let anim = animation_tags(style, enter, true, length_ms, x, y, self.frame.1 as f64);
        let body = text_body(style, &wrapped, enter);
        let mut fill = String::new();
        if fit < 1.0 {
            let border = if style.background.is_some() {
                (m.size * 0.22).round()
            } else {
                m.outline
            };
            let _ = write!(fill, "\\fs{}\\bord{border}", m.size);
        }
        if let Some(c) = color {
            let _ = write!(fill, "\\1c{}", inline_color(c));
        }
        if style.blur > 0.0 {
            fill.push_str(&format!("\\blur{:.1}", style.blur * m.size));
        }
        if let Some(outer) = &style.outer_outline {
            let _ = writeln!(
                self.events,
                "Dialogue: 0,{},{},{name},,0,0,0,,{{\\an{an}{anim}\\fs{fs}\\1c{c}\\3c{c}\\bord{b}\\shad0}}{body}",
                ass_time(start),
                ass_time(end),
                fs = m.size,
                c = inline_color(outer),
                b = m.outline + m.outer,
            );
        }
        let _ = writeln!(
            self.events,
            "Dialogue: 1,{},{},{name},,0,0,0,,{{\\an{an}{anim}{fill}}}{body}",
            ass_time(start),
            ass_time(end),
        );
    }
}

/// Largest font scale (down to 0.6) at which `text` wraps into `lines`
/// lines of `width` px without being cut off.
fn fit_scale(text: &str, width: f64, size: f64, lines: usize) -> f64 {
    let mut fit = 1.0;
    while fit > 0.6
        && wrap_text(text, max_units(width, (size * fit) as u32, text), lines).ends_with('…')
    {
        fit -= 0.05;
    }
    fit
}

impl Writer {
    /// Word-by-word caption chunk: one event per spoken word, the active word
    /// in the highlight colour. Only colour changes (no per-word scaling, which
    /// would make neighbouring words jump).
    fn chunk(
        &mut self,
        style: &TextStyle,
        words: &[(String, f64, f64, bool)],
        end: f64,
        enter: bool,
    ) {
        let (an, x, y) = placement(style, self.frame);
        let text_of = |active: Option<usize>| {
            let mut out = String::new();
            for (i, (word, _, _, space)) in words.iter().enumerate() {
                if i > 0 && *space {
                    out.push(' ');
                }
                let word = if style.uppercase {
                    word.to_uppercase()
                } else {
                    word.clone()
                };
                if Some(i) == active {
                    let _ = write!(
                        out,
                        "{{\\1c{}}}{}{{\\1c{}}}",
                        inline_color(&style.highlight),
                        escape(&word),
                        inline_color(&style.color)
                    );
                } else {
                    out.push_str(&escape(&word));
                }
            }
            out
        };
        for i in 0..words.len() {
            let start = words[i].1;
            let stop = if i + 1 < words.len() {
                words[i + 1].1
            } else {
                end
            };
            if stop - start < 0.02 {
                continue;
            }
            // Entrance on the chunk's first word, exit fade on its last.
            let length_ms = ((stop - start) * 1000.0).round().max(1.0) as u32;
            let anim = animation_tags(
                style,
                i == 0 && enter,
                i + 1 == words.len(),
                length_ms.max(style.enter_ms * 2),
                x,
                y,
                self.frame.1 as f64,
            );
            let _ = writeln!(
                self.events,
                "Dialogue: 1,{},{},Cap,,0,0,0,,{{\\an{an}{anim}}}{}",
                ass_time(start),
                ass_time(stop),
                text_of(Some(i))
            );
        }
    }
}

/// Source time until which a telop already shows this line (so the subtitle
/// would only repeat it), or `None` when no telop does.
pub fn repeated_until(plan: &EditPlan, line: &TimedText) -> Option<f64> {
    let (Ok(start), Ok(end)) = (parse_time(&line.start), parse_time(&line.end)) else {
        return None;
    };
    plan.telops
        .iter()
        .filter_map(|telop| {
            let shown = telop.text.trim_end_matches('…');
            let (s, e) = (parse_time(&telop.start).ok()?, parse_time(&telop.end).ok()?);
            (s < end && e > start && !shown.is_empty() && line.text.trim().starts_with(shown))
                .then_some(e)
        })
        .reduce(f64::max)
}

/// Splits a line into timed words: real word timings when the transcript has
/// them, otherwise spread over the line by character count.
fn line_words(line: &TimedText) -> Vec<(String, f64, f64, bool)> {
    if !line.words.is_empty() {
        return line
            .words
            .iter()
            .map(|w: &WordTime| (w.text.clone(), w.start, w.end, w.space))
            .collect();
    }
    let (Ok(start), Ok(end)) = (parse_time(&line.start), parse_time(&line.end)) else {
        return Vec::new();
    };
    let spaced = line.text.contains(' ');
    let tokens: Vec<String> = if spaced {
        line.text.split_whitespace().map(str::to_string).collect()
    } else {
        // Japanese without word boundaries: 4-character pieces.
        line.text
            .chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect())
            .collect()
    };
    let total: usize = tokens
        .iter()
        .map(|t| t.chars().count())
        .sum::<usize>()
        .max(1);
    let mut t = start;
    tokens
        .into_iter()
        .map(|token| {
            let len = (end - start) * token.chars().count() as f64 / total as f64;
            let word = (token, t, t + len, spaced);
            t += len;
            word
        })
        .collect()
}

/// Groups timed words into chunks that fit `max_units` and `CHUNK_WINDOW`.
fn chunks(
    words: &[(String, f64, f64, bool)],
    max_units: usize,
) -> Vec<Vec<(String, f64, f64, bool)>> {
    let units = |s: &str| {
        s.chars()
            .map(|c| if c.is_ascii() { 1 } else { 2 })
            .sum::<usize>()
    };
    let mut out: Vec<Vec<(String, f64, f64, bool)>> = Vec::new();
    let mut current: Vec<(String, f64, f64, bool)> = Vec::new();
    for w in words {
        let width: usize = current.iter().map(|c| units(&c.0) + 1).sum();
        let full = !current.is_empty()
            && (width + units(&w.0) > max_units || w.1 - current[0].1 > CHUNK_WINDOW);
        if full {
            out.push(std::mem::take(&mut current));
        }
        let ends_sentence = w.0.ends_with(['。', '！', '？', '.', '!', '?']);
        current.push(w.clone());
        if ends_sentence {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Formats `YYYY-MM-DD` as `2026.10.20 TUE`.
pub fn date_label(date: &str) -> Option<String> {
    let mut parts = date.get(..10)?.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    // Days since 1970-01-01 (Howard Hinnant's algorithm); 1970-01-01 was a Thursday.
    let (yy, mm) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * mm + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let weekday = ["THU", "FRI", "SAT", "SUN", "MON", "TUE", "WED"][days.rem_euclid(7) as usize];
    Some(format!("{y:04}.{m:02}.{d:02} {weekday}"))
}

fn is_title(telop: &TimedText) -> bool {
    telop.style.as_ref().and_then(|s| s.size.as_deref()) == Some("large")
}

/// The ASS script for the segment covering source `range`.
pub fn segment_script(
    plan: &EditPlan,
    style: &Style,
    range: (f64, f64),
    frame: (u32, u32),
) -> String {
    let mut w = Writer {
        frame,
        events: String::new(),
    };
    let local = |s: &str, e: &str| -> Option<(f64, f64, bool)> {
        let (s, e) = (parse_time(s).ok()?, parse_time(e).ok()?);
        let (a, b) = (s.max(range.0), e.min(range.1));
        (b - a > 0.05).then_some((a - range.0, b - range.0, s >= range.0 - 0.01))
    };

    for telop in &plan.telops {
        let Some((a, b, enter)) = local(&telop.start, &telop.end) else {
            continue;
        };
        if is_title(telop) && style.title.kind != TitleKind::None {
            w.text("Title", &style.title.text, &telop.text, a, b, enter, None);
            if style.title.kind == TitleKind::Card && style.title.show_date {
                if let Some(date) = plan.recorded_at.as_deref().and_then(date_label) {
                    w.text("TitleSub", &style.title.caption, &date, a, b, enter, None);
                }
            }
            continue;
        }
        let mut telop_style = style.telops.clone();
        if let Some(position) = telop.style.as_ref().and_then(|s| s.position.as_deref()) {
            telop_style.position = match position {
                "top" => style.telops.position.min(0.3),
                "center" | "middle" => 0.45,
                _ => style.captions.position - 0.08,
            };
        }
        let color = telop
            .style
            .as_ref()
            .and_then(|s| s.color.as_deref())
            .map(named_color);
        w.text("Telop", &telop_style, &telop.text, a, b, enter, color);
    }

    if plan.options.subtitles {
        for line in &plan.subtitles {
            // While a telop shows the same words, hold the subtitle back.
            let start = repeated_until(plan, line)
                .map(|until| {
                    crate::plan::format_time(until.max(parse_time(&line.start).unwrap_or(0.0)))
                })
                .unwrap_or_else(|| line.start.clone());
            let Some((a, b, enter)) = local(&start, &line.end) else {
                continue;
            };
            if b - a < 0.3 {
                continue;
            }
            match style.captions.mode {
                CaptionMode::Line => w.text("Cap", &style.captions, &line.text, a, b, enter, None),
                CaptionMode::Words => {
                    let words: Vec<_> = line_words(line)
                        .into_iter()
                        .filter(|(_, s, e, _)| *e > range.0 + a && *s < range.0 + b)
                        .map(|(t, s, e, sp)| (t, (s - range.0).max(a), (e - range.0).min(b), sp))
                        .collect();
                    let groups = chunks(&words, style.captions.chunk_units);
                    for (i, group) in groups.iter().enumerate() {
                        let end = groups
                            .get(i + 1)
                            .map(|next| next[0].1)
                            .unwrap_or(b)
                            .min(group.last().map(|l| l.2 + 0.4).unwrap_or(b))
                            .min(b);
                        w.chunk(&style.captions, group, end, enter || i > 0);
                    }
                }
            }
        }
    }

    script(style, frame, &w.events)
}

fn script(style: &Style, frame: (u32, u32), events: &str) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {}\nPlayResY: {}\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n\
         [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n{}\n{}\n{}\n{}\n\n\
         [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n{}",
        frame.0,
        frame.1,
        style_line("Cap", &style.captions, frame),
        style_line("Telop", &style.telops, frame),
        style_line("Title", &style.title.text, frame),
        style_line("TitleSub", &style.title.caption, frame),
        events
    );
    out
}

/// A static title over a thumbnail still: the format's title style without
/// its entrance animation.
pub fn thumbnail_script(style: &Style, text: &str, frame: (u32, u32)) -> String {
    let mut title = style.title.text.clone();
    title.enter = Animation::None;
    title.exit = Animation::None;
    let mut w = Writer {
        frame,
        events: String::new(),
    };
    w.text("Title", &title, text, 0.0, 10.0, false, None);
    script(style, frame, &w.events)
}

/// Plan colour names (manual captions) to hex.
fn named_color(name: &str) -> &str {
    match name {
        "yellow" => "#FFE14D",
        "cyan" | "blue" => "#4DE8FF",
        "red" => "#FF4D4D",
        "pink" => "#FF6FB5",
        "green" => "#5CFF8D",
        "orange" => "#FFA53D",
        "black" => "#111111",
        "white" => "#FFFFFF",
        other if other.starts_with('#') => other,
        _ => "#FFFFFF",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::CaptionStyle;
    use crate::style::shorts;

    fn line(start: f64, end: f64, text: &str) -> TimedText {
        TimedText {
            start: crate::plan::format_time(start),
            end: crate::plan::format_time(end),
            text: text.to_string(),
            style: None,
            words: Vec::new(),
        }
    }

    #[test]
    fn converts_colours() {
        assert_eq!(ass_color("#FFE600"), "&H0000E6FF");
        assert_eq!(ass_color("#000000A0"), "&H5F000000");
        assert_eq!(inline_color("#39E508"), "&H08E539&");
    }

    #[test]
    fn formats_times_and_dates() {
        assert_eq!(ass_time(3723.456), "1:02:03.46");
        assert_eq!(date_label("2026-10-20").as_deref(), Some("2026.10.20 TUE"));
        assert_eq!(
            date_label("2024-02-29T10:00:00Z").as_deref(),
            Some("2024.02.29 THU")
        );
    }

    #[test]
    fn chunks_words_by_width_and_time() {
        let words: Vec<_> = ["THIS", "IS", "A", "VERY", "LONG", "SENTENCE."]
            .iter()
            .enumerate()
            .map(|(i, w)| (w.to_string(), i as f64 * 0.3, i as f64 * 0.3 + 0.25, true))
            .collect();
        let groups = chunks(&words, 12);
        assert_eq!(
            groups.iter().map(|g| g.len()).collect::<Vec<_>>(),
            [3, 2, 1]
        );
    }

    #[test]
    fn word_mode_highlights_the_spoken_word() {
        let mut style = shorts();
        style.captions.mode = CaptionMode::Words;
        let mut plan = EditPlan::new("a.mp4", "a", "local");
        plan.subtitles = vec![line(1.0, 2.0, "hello brave world")];
        let ass = segment_script(&plan, &style, (0.5, 3.0), (1080, 1920));
        let events: Vec<&str> = ass.lines().filter(|l| l.starts_with("Dialogue")).collect();
        assert_eq!(events.len(), 3, "{ass}");
        assert!(events[1].contains(&format!(
            "{{\\1c{}}}brave",
            inline_color(&style.captions.highlight)
        )));
        assert!(events[0].starts_with("Dialogue: 1,0:00:00.50,"));
    }

    #[test]
    fn title_card_shows_the_date() {
        let mut style = shorts();
        style.title.kind = TitleKind::Card;
        style.title.show_date = true;
        let mut plan = EditPlan::new("a.mp4", "a", "local");
        plan.recorded_at = Some("2026-10-20".to_string());
        let mut title = line(0.0, 2.5, "朝のルーティン");
        title.style = Some(CaptionStyle {
            size: Some("large".to_string()),
            ..CaptionStyle::default()
        });
        plan.telops = vec![title];
        let ass = segment_script(&plan, &style, (0.0, 5.0), (1920, 1080));
        assert!(ass.contains(",Title,,") && ass.contains("朝のルーティン"));
        assert!(ass.contains("2026.10.20 TUE"));
    }

    #[test]
    fn continuing_text_skips_the_entrance() {
        let style = shorts();
        let mut plan = EditPlan::new("a.mp4", "a", "local");
        plan.telops = vec![TimedText {
            style: Some(CaptionStyle::default()),
            ..line(1.0, 4.0, "ポイント")
        }];
        let first = segment_script(&plan, &style, (0.0, 2.0), (1080, 1920));
        let second = segment_script(&plan, &style, (2.0, 5.0), (1080, 1920));
        assert!(first.contains("\\fscx0"), "pop on entry");
        assert!(!second.contains("\\fscx0"), "no replay after the cut");
    }

    #[test]
    fn long_titles_shrink_instead_of_truncating() {
        let style = shorts();
        let mut plan = EditPlan::new("a.mp4", "a", "local");
        plan.telops = vec![line(0.0, 2.0, "And so, my fellow Americans ask not what")];
        let ass = segment_script(&plan, &style, (0.0, 3.0), (1080, 1920));
        assert!(!ass.contains('…'), "{ass}");
        assert!(ass.contains("\\fs"), "{ass}");
        assert_eq!(fit_scale("短い", 900.0, 90.0, 2), 1.0);
    }

    #[test]
    fn double_outline_adds_a_back_layer() {
        let mut style = shorts();
        style.telops.outer_outline = Some("#FFFFFF".to_string());
        style.telops.outer_outline_width = 0.08;
        let mut plan = EditPlan::new("a.mp4", "a", "local");
        plan.telops = vec![line(0.0, 2.0, "ドン！")];
        let ass = segment_script(&plan, &style, (0.0, 3.0), (1080, 1920));
        assert!(ass.contains("Dialogue: 0,") && ass.contains("Dialogue: 1,"));
    }
}
