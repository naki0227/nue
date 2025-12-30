use anyhow::{Context, Result};
use log::{error, info, LevelFilter};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::channel;
use std::time::Duration;
use rand::seq::SliceRandom; // Added missing import

const RAW_DIR: &str = "/app/data/raw";
const JSON_DIR: &str = "/app/data/json";
const OUTPUT_DIR: &str = "/app/data/output";
const TEMP_DIR: &str = "/app/data/temp";
const BGM_PATH: &str = "/app/data/bgm/default_bgm.mp3";

#[derive(Serialize)]
struct LogEntry<'a> {
    severity: &'a str,
    message: &'a str,
    event: Option<&'a str>,
    path: Option<&'a str>,
}

fn log_json(level: &str, message: &str, event: Option<&str>, path: Option<&str>) {
    let entry = LogEntry {
        severity: level,
        message,
        event,
        path,
    };
    if let Ok(json) = serde_json::to_string(&entry) {
        println!("{}", json);
    }
}

#[derive(Debug, Deserialize)]
struct CaptionStyle {
    font: Option<String>,
    color: Option<String>,
    position: Option<String>,
    #[serde(rename = "box")]
    start_box: Option<bool>,
    background_asset: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Cut {
    start_time: String,
    end_time: String,
    filter: String,
    transition_type: Option<String>,
    caption: Option<String>,
    caption_style: Option<CaptionStyle>,
    focus_point: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct SeEvent {
    timestamp: String,
    #[serde(rename = "type")]
    event_type: String, 
    tag: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VisualEffect {
    start: String,
    end: String,
    #[serde(rename = "type")]
    effect_type: String,
    speed: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Thumbnail {
    timestamp: String,
    text: String,
    color: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Analysis {
    cuts: Vec<Cut>,
    original_filename: String,
    bgm_path: Option<String>,
    se_events: Option<Vec<SeEvent>>,
    visual_effects: Option<Vec<VisualEffect>>,
    thumbnail: Option<Thumbnail>,
}

// ... main ...

fn get_thumbnail_filter(text: &str, color: &str) -> String {
    let font = "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc";
    let font_color = match color.to_lowercase().as_str() {
        "yellow" => "yellow",
        "red" => "red",
        "cyan" => "cyan",
        _ => "white",
    };
    
    // Saturation boost + Contrast boost + Big Text
    format!(
        "eq=saturation=1.5:contrast=1.2,drawtext=text='{}':fontfile={}:fontsize=120:fontcolor={}:x=(w-text_w)/2:y=(h-text_h)/2:borderw=5:bordercolor=black:shadowx=5:shadowy=5",
        text.replace("'", "").replace(":", "\\:"), font, font_color
    )
}

fn generate_thumbnail(video_path: &Path, thumbnail: &Thumbnail, output_dir: &str, filename: &str) -> Result<()> {
    // timestamp format HH:MM:SS
    // output: output_dir/filename_thumb.jpg
    
    let out_path = PathBuf::from(output_dir).join(format!("{}_thumb.jpg", filename));
    let filter = get_thumbnail_filter(&thumbnail.text, thumbnail.color.as_deref().unwrap_or("white"));
    
    log_json("INFO", &format!("Generating thumbnail at {}", thumbnail.timestamp), Some("thumbnail_gen"), None);

    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-ss")
        .arg(&thumbnail.timestamp)
        .arg("-i")
        .arg(video_path)
        .arg("-vf")
        .arg(filter)
        .arg("-vframes")
        .arg("1")
        .arg(&out_path)
        .status()?;

    if status.success() {
        log_json("INFO", "Thumbnail generated", Some("thumbnail_success"), Some(out_path.to_str().unwrap_or("")));
    } else {
        log_json("ERROR", "Thumbnail generation failed", Some("thumbnail_failed"), None);
    }

    Ok(())
}



fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::new()
        .filter_level(LevelFilter::Info)
        .format(|buf, record| {
            writeln!(buf, "{}", record.args())
        })
        .init();

    log_json("INFO", "Muscle service started", Some("startup"), None);

    // Create directories
    fs::create_dir_all(RAW_DIR)?;
    fs::create_dir_all(JSON_DIR)?;
    fs::create_dir_all(OUTPUT_DIR)?;

    // Setup watcher
    let (tx, rx) = channel();
    let config = Config::default()
        .with_poll_interval(Duration::from_secs(2));
    let mut watcher: RecommendedWatcher = Watcher::new(tx, config)?;
    
    watcher.watch(Path::new(JSON_DIR), RecursiveMode::NonRecursive)?;
    log_json("INFO", "Watching directory", Some("watch_start"), Some(JSON_DIR));

    for res in rx {
        match res {
            Ok(event) => {
                if let notify::EventKind::Create(_) = event.kind {
                    for path in event.paths {
                        if path.extension().map_or(false, |ext| ext == "json") {
                            log_json("INFO", "New analysis detected", Some("file_detected"), Some(path.to_str().unwrap_or("")));
                            
                            std::thread::sleep(Duration::from_secs(1));
                            
                            if let Ok(content) = fs::read_to_string(&path) {
                                match serde_json::from_str::<Analysis>(&content) {
                                    Ok(analysis) => {
                                        // V14 DEBUG: Check deserialization of SE events
                                        if let Some(events) = &analysis.se_events {
                                            log_json("INFO", &format!("Deserialized {} SE events", events.len()), Some("debug_se_count"), None);
                                        } else {
                                            log_json("WARN", "Deserialized SE events is NONE", Some("debug_se_count"), None);
                                        }

                                        if let Err(e) = process_instruction(analysis) {
                                            log_json("ERROR", &format!("Processing failed: {}", e), Some("process_error"), Some(path.to_str().unwrap_or("")));
                                        }
                                    },
                                    Err(e) => log_json("ERROR", &format!("JSON parse failed: {}", e), Some("parse_error"), Some(path.to_str().unwrap_or(""))),
                                }
                            }
                        }
                    }
                }
            },
            Err(e) => log_json("ERROR", &format!("Watch error: {}", e), Some("watch_error"), None),
        }
    }

    Ok(())
}

fn get_transition_filter(name: &str) -> &str {
    match name.to_lowercase().as_str() {
        "wipeleft" => "wipeleft",
        "wiperight" => "wiperight",
        "slideup" => "slideup",
        "circleopen" => "circleopen",
        _ => "fade", 
    }
}

fn get_drawtext_config(style: &Option<CaptionStyle>) -> (String, String, String, String) {
    let default_font = "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc";
    
    if let Some(s) = style {
        let font = match s.font.as_deref().unwrap_or("sans") {
            "serif" => "/usr/share/fonts/opentype/noto/NotoSerifCJK-Bold.ttc", 
            _ => default_font,
        };
        
        let color = match s.color.as_deref().unwrap_or("white") {
            "yellow" => "yellow",
            "cyan" => "cyan",
            _ => "white",
        };
        
        let box_conf = if s.start_box.unwrap_or(false) {
            ":box=1:boxcolor=black@0.5:boxborderw=5"
        } else {
            ""
        };
        
        let y = match s.position.as_deref().unwrap_or("bottom") {
            "top" => "h*0.1",
            "center" => "(h-text_h)/2",
            _ => "h*0.85", // Safer bottom for vertical video UI
        };
        
        (font.to_string(), color.to_string(), box_conf.to_string(), y.to_string())
    } else {
        (default_font.to_string(), "white".to_string(), "".to_string(), "h*0.85".to_string())
    }
}

fn get_se_file(tag: &str) -> PathBuf {
    let base = PathBuf::from("/app/data/se");
    let tag_lower = tag.to_lowercase();
    
    // V13 LOGIC: SYNTHETIC SAFE SOUNDS
    // User Feedback: "Get safe sounds".
    // Action: We generated pure synthetic WAVs (Pink Noise Swoosh, Sine Wave Don).
    // Zero artifacts guaranteed.
    
    info!("Selecting SE for tag: {}", tag_lower);

    let filename = if tag_lower.contains("serious") {
        "SYNTH_DON.wav" 
    } else if tag_lower.contains("funny") {
        "SYNTH_WHOOSH.wav" 
    } else if tag_lower.contains("whoosh") {
        "SYNTH_WHOOSH.wav"
    } else if tag_lower.contains("correct") {
        "SYNTH_DON.wav"
    } else if tag_lower.contains("impact") {
        "SYNTH_DON.wav"
    } else {
        "SYNTH_DON.wav"
    };
    
    let candidate = base.join(filename);
    let candidate = base.join(filename);
    // V14 FORCE: Blindly return path. Do not check exists().
    // Docker bind mounts sometimes confuse Rust's exists() check.
    // FFmpeg will error if file is missing, which is better than silence.
    log_json("INFO", &format!("Selected SAFE SE for '{}': {:?}", tag, candidate), Some("se_selection"), None);
    return candidate;
    
    // Fallback unreachable due to V14 FORCE logic
    // base.join("default_se.mp3")
}

// NEW SIMPLIFIED IMPLEMENTATION
// Process video using segment-based approach to avoid filter_complex limitations

fn process_instruction(analysis: Analysis) -> Result<()> {
    let video_path = PathBuf::from(RAW_DIR).join(&analysis.original_filename);
    let output_path = PathBuf::from(OUTPUT_DIR).join(&analysis.original_filename);
    let temp_dir = PathBuf::from(TEMP_DIR);
    
    // Create temp directory
    fs::create_dir_all(&temp_dir)?;
    
    // BGM path with fallback to default
    let bgm_path_str = analysis.bgm_path.clone().unwrap_or(BGM_PATH.to_string());
    let mut bgm_path_buf = PathBuf::from(&bgm_path_str);
    
    // If BGM file doesn't exist, try default_bgm.mp3
    if !bgm_path_buf.exists() {
        let default_bgm = PathBuf::from(BGM_PATH).parent().unwrap().join("default_bgm.mp3");
        if default_bgm.exists() {
            bgm_path_buf = default_bgm;
        }
    }
    
    let has_bgm = bgm_path_buf.exists();

    log_json("INFO", &format!("Processing video: {:?}", video_path), Some("process_start"), None);
    log_json("INFO", &format!("BGM: {:?}, exists: {}", bgm_path_buf, has_bgm), Some("bgm_check"), None);
    
    // Step 1: Process each cut as individual segment
    let mut segment_paths = Vec::new();
    
    for (i, cut) in analysis.cuts.iter().enumerate() {
        let segment_path = process_single_segment(i, cut, &video_path, &temp_dir, &analysis.visual_effects)?;
        segment_paths.push(segment_path);
    }
    
    log_json("INFO", &format!("Processed {} segments", segment_paths.len()), Some("segments_complete"), None);
    
    // Step 2: Create concat file list
    let concat_file = temp_dir.join("concat_list.txt");
    let mut file = fs::File::create(&concat_file)?;
    for seg in &segment_paths {
        writeln!(file, "file '{}'", seg.display())?;
    }
    drop(file);
    
    
    // Step 3: Concatenate all segments and add BGM/SE
    let mut concat_cmd = Command::new("ffmpeg");
    concat_cmd
        .arg("-y")
        .arg("-f").arg("concat")
        .arg("-safe").arg("0")
        .arg("-i").arg(&concat_file);
    
    // Build audio filter for BGM and sound effects
    let se_events = analysis.se_events.as_ref();
    let has_se = se_events.map(|se| !se.is_empty()).unwrap_or(false);
    
    if has_bgm || has_se {
        let mut input_index = 1;
        let mut filter_parts = Vec::new();
        let mut input_labels = vec!["[v_in]".to_string()];
        
        // Boost Video Audio (Standardized to 1.3 - safe boost)
        filter_parts.push(format!("[0:a]volume=1.3[v_in]"));

        // Add BGM input with volume adjustment
        if has_bgm {
            concat_cmd.arg("-i").arg(&bgm_path_buf);
            // Apply volume filter to BGM (volume=0.08 - subtle background)
            filter_parts.push(format!("[{}:a]volume=0.08[bgm]", input_index));
            input_labels.push("[bgm]".to_string());
            input_index += 1;
        }
        
        // Add SE inputs with adelay and volume adjustment
        if has_se {
            for se in se_events.unwrap() {
                let se_file = get_se_file(&se.event_type);
                // V14 DEBUG: Log every SE attempt
                log_json("INFO", &format!("Processing SE: type='{}', path='{:?}'", se.event_type, se_file), Some("se_debug"), None);
                
                // V14 FORCE: Skipping exists() check. Trust get_se_file.
                let delay_ms = parse_time(&se.timestamp).unwrap_or(0.0) * 1000.0;
                concat_cmd.arg("-i").arg(&se_file);
                // Add delay and volume adjustment for SE
                // V14 ADJUSTMENT: Boost synth SE volume to 0.8 (was 0.2)
                // Synthetic assets are quieter/unmastered, so they need more gain.
                let filter_part = format!("[{}:a]adelay={}|{},volume=0.8[se{}]", 
                    input_index, delay_ms as i64, delay_ms as i64, input_index);
                filter_parts.push(filter_part);
                input_labels.push(format!("[se{}]", input_index));
                input_index += 1;
            }
        }
        
        // Build amix filter
        // Note: inputs=N includes video audio [0:a] + bgm + SEs
        let num_inputs = input_labels.len();
        
        // Calculate fade out start (total duration - 2s)
        // We need total_duration here early.
        let mut early_total_duration = 0.0;
        for cut in &analysis.cuts {
            if let (Ok(start), Ok(end)) = (parse_time(&cut.start_time), parse_time(&cut.end_time)) {
                early_total_duration += end - start;
            }
        }
        let fade_start = if early_total_duration > 2.0 { early_total_duration - 2.0 } else { 0.0 };

        let filter_str = if filter_parts.is_empty() {
            format!("{}amix=inputs={}:duration=first,afade=t=out:st={:.3}:d=2[aout]", 
                input_labels.join(""), num_inputs, fade_start)
        } else {
            format!("{};{}amix=inputs={}:duration=first,afade=t=out:st={:.3}:d=2[aout]",
                filter_parts.join(";"), input_labels.join(""), num_inputs, fade_start)
        };
        
        log_json("INFO", &format!("Audio filter: {}", filter_str), Some("filter_debug"), None);
        
        concat_cmd
            .arg("-filter_complex").arg(&filter_str)
            .arg("-map").arg("0:v")
            .arg("-map").arg("[aout]");
    }
    
    // Calculate total duration to strictly limit output
    let mut total_duration = 0.0;
    for cut in &analysis.cuts {
        if let (Ok(start), Ok(end)) = (parse_time(&cut.start_time), parse_time(&cut.end_time)) {
             total_duration += end - start;
        }
    }
    
    let output = concat_cmd
        .arg("-c:v").arg("copy")  // Copy video (already encoded)
        .arg("-c:a").arg("aac")
        .arg("-t").arg(format!("{:.3}", total_duration)) // Force output duration to match video content
        .arg(&output_path)
        .output()?;
    
    if output.status.success() {
        log_json("INFO", "Video processing complete", Some("transcode_complete"), Some(output_path.to_str().unwrap_or("")));
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log_json("ERROR", &format!("Concatenation failed: {}", stderr), Some("transcode_failed"), Some(output_path.to_str().unwrap_or("")));
    }
    
    // Step 4: Generate thumbnail
    if let Some(thumb) = &analysis.thumbnail {
        if let Err(e) = generate_thumbnail(&video_path, thumb, OUTPUT_DIR, &analysis.original_filename) {
            log_json("ERROR", &format!("Thumbnail generation failed: {}", e), Some("thumbnail_error"), None);
        }
    }
    
    // Cleanup temp files
    for seg in &segment_paths {
        let _ = fs::remove_file(seg);
    }
    let _ = fs::remove_file(&concat_file);
    
    Ok(())
}

// Process a single segment with filters and effects
fn process_single_segment(
    index: usize,
    cut: &Cut,
    video_path: &Path,
    temp_dir: &Path,
    visual_effects: &Option<Vec<VisualEffect>>,
) -> Result<PathBuf> {
    let start_seconds = parse_time(&cut.start_time)?;
    let end_seconds = parse_time(&cut.end_time)?;
    let duration = end_seconds - start_seconds;
    
    if duration <= 0.0 {
        return Err(anyhow::anyhow!("Invalid segment duration"));
    }
    
    let segment_path = temp_dir.join(format!("seg_{:04}.mp4", index));
    
    // Build video filter chain
    let mut filters = Vec::new();
    
    // 1. Vertical crop and scale
    let focus = cut.focus_point.unwrap_or(0.5);
    filters.push(format!("scale=-2:1920,crop=1080:1920:(iw-1080)*{}:0", focus));
    
    // 2. Apply color filter (DISABLED - vintage filter inappropriate for modern content per user feedback)
    // match cut.filter.to_lowercase().as_str() {
    //     "sepia" => filters.push("colorchannelmixer=.393:.769:.189:0:.349:.686:.168:0:.272:.534:.131".to_string()),
    //     "grayscale" => filters.push("hue=s=0".to_string()),
    //     "vivid" => filters.push("eq=saturation=1.5".to_string()),
    //     "vintage" => filters.push("curves=vintage".to_string()),
    //     _ => {}
    // }
    
    // 3. Visual effects (zoom)
    if let Some(effects) = visual_effects {
        for effect in effects {
            if let Ok(effect_start) = parse_time(&effect.start) {
                if effect_start >= start_seconds && effect_start < end_seconds {
                    match effect.effect_type.as_str() {
                        "zoom_in" => filters.push("crop=iw/1.25:ih/1.25:(iw-out_w)/2:(ih-out_h)/2,scale=1080:1920".to_string()),
                        "zoom_out" => filters.push("crop=iw/1.1:ih/1.1:(iw-out_w)/2:(ih-out_h)/2,scale=1080:1920".to_string()),
                        _ => {}
                    }
                    break;
                }
            }
        }
    }
    
    // 4. Caption
    if let Some(cap) = &cut.caption {
        if let Some(cap) = &cut.caption {
            let valid_text = cap.replace("'", "").replace(":", "\\:");
            let (font, color, box_conf, y) = get_drawtext_config(&cut.caption_style);
            
            // Show caption for the entire segment duration
            let drawtext = format!(
                "drawtext=fontfile={}:text='{}':fontcolor={}:fontsize=80:x=(w-text_w)/2:y={}{}:enable='between(t,0,{})'",
                font, valid_text, color, y, box_conf, duration
            );
            filters.push(drawtext);
        }
    }
    
    let video_filter = filters.join(",");
    
    // Run ffmpeg to extract and process this segment
    // CRITICAL: -ss BEFORE -i for accurate seeking
    let output = Command::new("ffmpeg")
        .arg("-y")
        .arg("-ss").arg(format!("{:.3}", start_seconds))  // Seek BEFORE input
        .arg("-i").arg(video_path)
        .arg("-t").arg(format!("{:.3}", duration))  // Duration after input
        .arg("-vf").arg(&video_filter)
        .arg("-c:v").arg("libx264")
        .arg("-preset").arg("fast")
        .arg("-crf").arg("23")
        .arg("-pix_fmt").arg("yuv420p")
        .arg("-c:a").arg("aac")
        .arg("-b:a").arg("128k")
        .arg(&segment_path)
        .output()?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Segment {} failed: {}", index, stderr));
    }
    
    log_json("INFO", &format!("Segment {} complete", index), Some("segment_done"), None);
    Ok(segment_path)
}


fn check_audio_stream(path: &Path) -> Result<bool> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("a")
        .arg("-show_entries")
        .arg("stream=codec_type")
        .arg("-of")
        .arg("csv=p=0")
        .arg(path)
        .output()?;
    
    Ok(!output.stdout.is_empty())
}

fn parse_time(time_str: &str) -> Result<f64> {
    let parts: Vec<&str> = time_str.split(':').collect();
    if parts.len() == 3 {
        let first: f64 = parts[0].parse()?;
        let second: f64 = parts[1].parse()?;
        let third: f64 = parts[2].parse()?;
        
        // Intelligently detect format:
        // If third field > 59, it's milliseconds (MM:SS:MMM format)
        // If third field <= 59, it's seconds (HH:MM:SS format)
        if third > 59.0 {
            // MM:SS:MMM format: minutes:seconds:milliseconds
            Ok(first * 60.0 + second + third / 1000.0)
        } else {
            // HH:MM:SS format: hours:minutes:seconds
            Ok(first * 3600.0 + second * 60.0 + third)
        }
    } else {
        Ok(time_str.parse()?)
    }
}
