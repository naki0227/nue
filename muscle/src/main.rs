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

const RAW_DIR: &str = "/app/data/raw";
const JSON_DIR: &str = "/app/data/json";
const OUTPUT_DIR: &str = "/app/data/output";
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
    let font = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf";
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
    let default_font = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf";
    
    if let Some(s) = style {
        let font = match s.font.as_deref().unwrap_or("sans") {
            "serif" => "/usr/share/fonts/truetype/dejavu/DejaVuSerif-Bold.ttf", 
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
    // Simple mapping or strict file lookup
    let base = PathBuf::from("/app/data/se");
    // For now, map generic types to specific dummy files if they exist, or use a default
    // In a real scenario, this would look up the specific file downloaded by Trend Watcher
    // We assume Trend Watcher names them by ID, but we might have a map.
    // For this implementation, we will try to find a file containing the tag name.
    
    // Fallback logic: check if specific file exists
    let candidate = base.join(format!("{}.mp3", tag));
    if candidate.exists() {
        return candidate;
    }
    
    // Default fallback
    base.join("default_se.mp3")
}

fn process_instruction(analysis: Analysis) -> Result<()> {
    let video_path = PathBuf::from(RAW_DIR).join(&analysis.original_filename);
    let output_path = PathBuf::from(OUTPUT_DIR).join(&analysis.original_filename); 
    
    let bgm_path_str = analysis.bgm_path.clone().unwrap_or(BGM_PATH.to_string());
    let bgm_path_buf = PathBuf::from(&bgm_path_str);
    let has_bgm = bgm_path_buf.exists();

    log_json("INFO", &format!("Processing video: {:?}", video_path), Some("process_start"), None);
    if has_bgm {
        log_json("INFO", &format!("Using BGM: {:?}", bgm_path_buf), Some("bgm_selected"), None);
    } else {
        log_json("INFO", "No BGM found or specified", Some("bgm_missing"), None);
    }
    
    let has_audio = check_audio_stream(&video_path).unwrap_or(false);

    let mut filter_complex = String::new();
    let mut last_video_label = String::new();
    let mut last_audio_label = String::new();
    let mut accumulated_duration_for_chaining = 0.0; // Tracks the end time of the last stitched segment
    
    // Track valid timeline mapping for SEs
    // (source_start, source_end, output_start_offset)
    let mut time_map: Vec<(f64, f64, f64)> = Vec::new();
    let mut current_output_time = 0.0; // Tracks the current position in the output timeline for mapping

    // Step 1: Prepare segments and build time_map
    for (i, cut) in analysis.cuts.iter().enumerate() {
        let start_seconds = parse_time(&cut.start_time)?;
        let end_seconds = parse_time(&cut.end_time)?;

        if end_seconds <= start_seconds {
            continue;
        }

        let duration = end_seconds - start_seconds;
        let transition_duration = if i > 0 { 0.5 } else { 0.0 };

        // The output start time for this cut's content
        // For the first cut, it's 0. For subsequent cuts, it's the end of the previous cut minus transition.
        let output_start_for_this_cut = if i == 0 {
            0.0
        } else {
            current_output_time - transition_duration
        };
        
        time_map.push((start_seconds, end_seconds, output_start_for_this_cut));
        
        // Update current_output_time to reflect the end of this cut's content in the output
        current_output_time = output_start_for_this_cut + duration;

        let filter_effect = match cut.filter.to_lowercase().as_str() {
            "sepia" => ",colorchannelmixer=.393:.769:.189:0:.349:.686:.168:0:.272:.534:.131",
            "grayscale" => ",hue=s=0",
            "vivid" => ",eq=saturation=1.5",
            "vintage" => ",curves=vintage",
            _ => "", 
        };

        // VERTICAL CROP LOGIC (Phase 10)
        // Target: 1080x1920 (9:16)
        // 1. Scale height to 1920, keep aspect ratio (width will be ~3413 for 16:9 source)
        // 2. Crop 1080 width based on focus_point (0.0=left, 0.5=center, 1.0=right)
        let focus = cut.focus_point.unwrap_or(0.5);
        let vertical_pipeline = format!(",scale=-2:1920,crop=1080:1920:(iw-1080)*{}:0", focus);

        // Determine Visual Effects (Zoom) - Adapted for Vertical
        let mut visual_effect_str = String::new();
        if let Some(effects) = &analysis.visual_effects {
            for effect in effects {
                if let Ok(effect_start) = parse_time(&effect.start) {
                    if effect_start >= start_seconds - 0.5 && effect_start < end_seconds {
                        match effect.effect_type.as_str() {
                            "zoom_in" => {
                                // For vertical, we zoom in by cropping closer (e.g., 1.25x equivalent)
                                // Since we are already 1080x1920, we crop to 864x1536 and scale back up
                                visual_effect_str = ",crop=iw/1.25:ih/1.25:(iw-out_w)/2:(ih-out_h)/2,scale=1080:1920".to_string();
                            },
                             "zoom_out" => {
                                visual_effect_str = ",crop=iw/1.1:ih/1.1:(iw-out_h)/2:(ih-out_h)/2,scale=1080:1920".to_string();
                            },
                            _ => {}
                        }
                        break;
                    }
                }
            }
        }

        let drawtext_effect = if let Some(cap) = &cut.caption {
            if !cap.is_empty() {
                let valid_text = cap.replace("'", "").replace(":", "\\:");
                let (font, color, box_conf, y) = get_drawtext_config(&cut.caption_style);
                
                let mut bg_filter = String::new();
                
                // BACKGROUND STRATEGY (Phase 11): Use `drawbox` for simplified shapes
                if let Some(style) = &cut.caption_style {
                     if let Some(bg_asset) = &style.background_asset {
                        // Simple logic: if bg_asset is "simple_box" or "news_ticker", draw a box.
                        // We can't easily overlay external images in this linear chain without complex refactoring.
                        // So we use `drawbox` as a robust alternative.
                        if bg_asset.contains("box") || bg_asset.contains("ticker") {
                            // Draw a semi-transparent box behind text area
                            // y = y position of text. height = 120 (approx font size 80 + padding)
                            // width = video width (1080)
                            // color = black@0.6 or blue@0.8
                            let box_color = if bg_asset.contains("ticker") { "blue@0.8" } else { "black@0.6" };
                            let box_y = if y.contains("h*") { 
                                // Parse approximation? Hard to parse calc string.
                                // Fallback: hardcode based on known positions
                                if y.contains("0.1") { "h*0.1-20" } else { "h*0.85-20" }
                            } else {
                                "h-140"
                            };
                            
                            bg_filter = format!(",drawbox=x=0:y={}:w=iw:h=160:color={}:t=fill", box_y, box_color);
                        }
                     }
                }

                // Adjust font size for vertical (needs to be readable on mobile) -> 80
                format!("{},drawtext=text='{}':fontfile={}:fontsize=80:fontcolor={}:x=(w-text_w)/2:y={}{}:borderw=4:bordercolor=black", 
                    bg_filter, valid_text, font, color, y, box_conf)
            } else {
                "".to_string()
            }
        } else {
            "".to_string()
        };

        let v_label = format!("v{}", i);
        let a_label = format!("a{}", i);
        
        // Order: trim -> filter (color) -> vertical_pipeline -> visual (zoom) -> setpts -> drawtext
        // Note: visual_effect (zoom) usually expects the "canonical" resolution. 
        // vertical_pipeline sets it to 1080x1920.
        // So visual_effect logic was updated to output 1080x1920.
        filter_complex.push_str(&format!(
            "[0:v]trim=start={}:end={}{}{}{},setpts=PTS-STARTPTS{}[{}];",
            start_seconds, end_seconds, filter_effect, vertical_pipeline, visual_effect_str, drawtext_effect, v_label
        ));

        if has_audio {
            filter_complex.push_str(&format!(
                "[0:a]atrim=start={}:end={},asetpts=PTS-STARTPTS[{}];",
                start_seconds, end_seconds, a_label
            ));
        } else {
            // Null audio source
            filter_complex.push_str(&format!(
                "anullsrc=channel_layout=stereo:sample_rate=44100:d={}[{}];",
                duration, a_label
            ));
        }
    }
    
    // Reset accumulated for chaining
    accumulated_duration_for_chaining = 0.0;
    
    // Step 2: Chain them
    for i in 0..analysis.cuts.len() {
        let cut = &analysis.cuts[i];
        let duration = parse_time(&cut.end_time)? - parse_time(&cut.start_time)?;
        
        if i == 0 {
            last_video_label = "v0".to_string();
            last_audio_label = "a0".to_string();
            accumulated_duration_for_chaining += duration;
        } else {
            let transition_name = cut.transition_type.as_deref().unwrap_or("fade");
            let transition_expr = get_transition_filter(transition_name);

            let transition_duration = 0.5;

            let next_v_label = format!("v{}", i);
            let next_a_label = format!("a{}", i);
            let result_v_label = format!("vm{}", i);
            let result_a_label = format!("am{}", i);
            
            let offset = accumulated_duration_for_chaining - transition_duration;
            
            filter_complex.push_str(&format!(
                "[{}][{}]xfade=transition={}:duration={}:offset={}[{}];",
                last_video_label, next_v_label, transition_expr, transition_duration, offset, result_v_label
            ));

            filter_complex.push_str(&format!(
                "[{}][{}]acrossfade=d={}[{}];",
                last_audio_label, next_a_label, transition_duration, result_a_label
            ));
            
            last_video_label = result_v_label;
            last_audio_label = result_a_label;
            accumulated_duration_for_chaining += duration - transition_duration;
        }
    }

    // Step 3: Prepare BGM and SE mixing
    // Current audio is 'last_audio_label'.
    // We need to mix in BGM (looped) and SE inputs.
    
    // Auto-Ducking Strategy:
    // 1. Prepare BGM (looped)
    // 2. Use `sidechaincompress` to duck BGM based on Main Audio (last_audio_label)
    // 3. Mix Ducked BGM + Main Audio + SEs

    let mut final_mix_inputs = Vec::new();

    // 1. Main Audio
    final_mix_inputs.push(format!("[{}]", last_audio_label));

    // 2. BGM (Ducked)
    if has_bgm {
        // Loop BGM
        filter_complex.push_str(&format!(
            "[1:a]aloop=loop=-1:size=2e+9,atrim=duration={},volume=0.2[bgm_raw];",
            accumulated_duration_for_chaining
        ));

        // Sidechain Compress: Duck [bgm_raw] using [last_audio_label] as trigger
        // threshold=0.05: Trigger easily
        // ratio=4: Compress heavily (1/4 volume)
        // attack=100: Slow attack for smooth fade out
        // release=300: Slow release for smooth fade in
        filter_complex.push_str(&format!(
            "[bgm_raw][{}]sidechaincompress=threshold=0.05:ratio=4:attack=100:release=300[bgm_ducked];",
            last_audio_label
        ));
        
        final_mix_inputs.push("[bgm_ducked]".to_string());
    }

    // 3. Add SEs
    if let Some(events) = &analysis.se_events {
        for (idx, event) in events.iter().enumerate() {
            let ts = parse_time(&event.timestamp)?;
            
            // Map timestamp to output time
            let mut output_ts = -1.0;
            for (src_start, src_end, out_start) in &time_map {
                if ts >= *src_start && ts <= *src_end {
                    output_ts = out_start + (ts - src_start);
                    break;
                }
            }
            
            if output_ts >= 0.0 {
                let se_file = get_se_file(&event.event_type);
                if se_file.exists() {
                     // Use amovie to load file
                    let delay_ms = (output_ts * 1000.0) as u64;
                    filter_complex.push_str(&format!(
                        "amovie={}[se{}raw];[se{}raw]adelay={}|{}[se{}];",
                        se_file.to_string_lossy(), idx, idx, delay_ms, delay_ms, idx
                    ));
                    
                    final_mix_inputs.push(format!("[se{}]", idx));
                }
            }
        }
    }
    
    // Final Mix
    if final_mix_inputs.len() > 1 {
        let mixed_label = "audio_final";
        let inputs_str = final_mix_inputs.join("");
        filter_complex.push_str(&format!(
            "{}amix=inputs={}:duration=first:dropout_transition=2[{}]",
            inputs_str, final_mix_inputs.len(), mixed_label
        ));
        last_audio_label = mixed_label.to_string();
    } else {
        if filter_complex.ends_with(';') {
            filter_complex.pop();
        }
    }

    log_json("INFO", &format!("Executing FFmpeg with complex filter: {}", filter_complex), Some("ffmpeg_debug"), None);

    let mut command = Command::new("ffmpeg");
    command
        .arg("-y")
        .arg("-i")
        .arg(&video_path);

    if has_bgm {
        command.arg("-i").arg(&bgm_path_buf);
    }
    // Note: We are NOT adding SEs as -i inputs because we use amovie filter. 
    // This assumes SE files are accessible inside the container filesystem.

    let status = command
        .arg("-filter_complex")
        .arg(&filter_complex)
        .arg("-map")
        .arg(format!("[{}]", last_video_label))
        .arg("-map")
        .arg(format!("[{}]", last_audio_label)) 
        .arg("-c:v")
        .arg("libx264")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-c:a")
        .arg("aac")
        .arg(&output_path)
        .status()?;

    if status.success() {
        log_json("INFO", "Stitching success", Some("transcode_complete"), Some(output_path.to_str().unwrap_or("")));
    } else {
        log_json("ERROR", "FFmpeg stitching failed", Some("transcode_failed"), Some(output_path.to_str().unwrap_or("")));
    }

    // Step 4: Generate Thumbnail
    if let Some(thumb) = &analysis.thumbnail {
       if let Err(e) = generate_thumbnail(&video_path, thumb, OUTPUT_DIR, &analysis.original_filename) {
            log_json("ERROR", &format!("Thumbnail generation failed: {}", e), Some("thumbnail_error"), None);
       }
    }

    Ok(())
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
        let h: f64 = parts[0].parse()?;
        let m: f64 = parts[1].parse()?;
        let s: f64 = parts[2].parse()?;
        Ok(h * 3600.0 + m * 60.0 + s)
    } else {
        Ok(time_str.parse()?)
    }
}
