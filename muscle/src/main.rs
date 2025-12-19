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

#[derive(Debug, Deserialize)]
struct Cut {
    start_time: String,
    end_time: String,
    filter: String,
    // description field ignored for now
}

#[derive(Debug, Deserialize)]
struct Analysis {
    cuts: Vec<Cut>,
    original_filename: String,
}

#[derive(Serialize)]
struct LogEntry {
    level: String,
    message: String,
    event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
}

fn log_json(level: &str, message: &str, event: Option<&str>, target: Option<&str>) {
    let entry = LogEntry {
        level: level.to_string(),
        message: message.to_string(),
        event: event.map(|s| s.to_string()),
        target: target.map(|s| s.to_string()),
    };
    if let Ok(json) = serde_json::to_string(&entry) {
        println!("{}", json);
    }
}

fn main() -> Result<()> {
    // Custom logger could be implemented, but for simplicity will just use println! with JSON
    // for standard messages.
    
    log_json("INFO", "Muscle service starting...", Some("startup"), None);

    fs::create_dir_all(OUTPUT_DIR).ok();

    fs::create_dir_all(RAW_DIR).ok();
    fs::create_dir_all(JSON_DIR).ok();
    fs::create_dir_all(OUTPUT_DIR).ok();

    let (tx, rx) = channel();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
    watcher.watch(Path::new(JSON_DIR), RecursiveMode::NonRecursive)?;

    log_json("INFO", "Watching directory", Some("watch_start"), Some(JSON_DIR));

    for res in rx {
        match res {
            Ok(event) => {
                match event.kind {
                    notify::EventKind::Create(_) | notify::EventKind::Modify(_) => {
                        for path in event.paths {
                            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                                std::thread::sleep(Duration::from_millis(500)); // slightly longer wait
                                if let Err(e) = process_instruction(&path) {
                                    log_json("ERROR", &format!("Failed to process: {:?}", e), Some("process_error"), Some(path.to_str().unwrap_or("")));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Err(e) => log_json("ERROR", &format!("Watch error: {:?}", e), Some("watch_error"), None),
        }
    }

    Ok(())
}

fn process_instruction(json_path: &PathBuf) -> Result<()> {
    log_json("INFO", "Processing instruction", Some("process_start"), Some(json_path.to_str().unwrap_or("")));

    let content = fs::read_to_string(json_path).context("Failed to read JSON file")?;
    let analysis: Analysis = serde_json::from_str(&content).context("Failed to parse JSON")?;

    let video_path = Path::new(RAW_DIR).join(&analysis.original_filename);
    if !video_path.exists() {
        return Err(anyhow::anyhow!("Raw video file not found: {:?}", video_path));
    }

    log_json("INFO", &format!("Found video with {} cuts", analysis.cuts.len()), Some("video_found"), Some(video_path.to_str().unwrap_or("")));

    for (i, cut) in analysis.cuts.iter().enumerate() {
        let output_filename = format!(
            "{}_cut{}_{}.mp4",
            Path::new(&analysis.original_filename)
                .file_stem()
                .unwrap()
                .to_string_lossy(),
            i,
            cut.filter.replace(" ", "_")
        );
        let output_path = Path::new(OUTPUT_DIR).join(output_filename);

        let filter_complex = match cut.filter.to_lowercase().as_str() {
            "sepia" => "colorchannelmixer=.393:.769:.189:0:.349:.686:.168:0:.272:.534:.131",
            "grayscale" => "hue=s=0",
            "vivid" => "eq=saturation=1.5",
            "none" | "" => "null",
            _ => "null", 
        };

        log_json("INFO", &format!("Transcoding cut {}", i), Some("transcode_start"), Some(output_path.to_str().unwrap_or("")));

        let status = Command::new("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(&video_path)
            .arg("-ss")
            .arg(&cut.start_time)
            .arg("-to")
            .arg(&cut.end_time)
            .arg("-vf")
            .arg(filter_complex)
            .arg(&output_path)
            .status()?;

        if status.success() {
            log_json("INFO", "Transcode success", Some("transcode_complete"), Some(output_path.to_str().unwrap_or("")));
        } else {
            log_json("ERROR", "FFmpeg failed", Some("transcode_failed"), Some(output_path.to_str().unwrap_or("")));
        }
    }

    Ok(())
}
