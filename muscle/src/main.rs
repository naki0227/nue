use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use log::{info, LevelFilter};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use reqwest::blocking::Client;
use reqwest::header::CONTENT_LENGTH;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::channel;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_DATA_DIR: &str = "/app/data";
const INLINE_VIDEO_LIMIT_BYTES: u64 = 18 * 1024 * 1024;

fn data_dir() -> PathBuf {
    if let Ok(path) = env::var("NUE_DATA_DIR") {
        return PathBuf::from(path);
    }

    let docker_path = PathBuf::from(DEFAULT_DATA_DIR);
    if docker_path.exists() {
        docker_path
    } else {
        PathBuf::from("data")
    }
}

fn data_path(name: &str) -> PathBuf {
    data_dir().join(name)
}

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
}

#[derive(Debug, Deserialize)]
struct Cut {
    start_time: String,
    end_time: String,
    caption: Option<String>,
    caption_style: Option<CaptionStyle>,
    focus_point: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct SeEvent {
    timestamp: String,
    #[serde(rename = "type")]
    event_type: String,
}

#[derive(Debug, Deserialize)]
struct VisualEffect {
    start: String,
    #[serde(rename = "type")]
    effect_type: String,
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
    job_id: Option<String>,
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

fn generate_thumbnail(
    video_path: &Path,
    thumbnail: &Thumbnail,
    output_dir: &str,
    filename: &str,
) -> Result<()> {
    // timestamp format HH:MM:SS
    // output: output_dir/filename_thumb.jpg

    let out_path = PathBuf::from(output_dir).join(format!("{}_thumb.jpg", filename));
    let filter = get_thumbnail_filter(
        &thumbnail.text,
        thumbnail.color.as_deref().unwrap_or("white"),
    );

    log_json(
        "INFO",
        &format!("Generating thumbnail at {}", thumbnail.timestamp),
        Some("thumbnail_gen"),
        None,
    );

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
        log_json(
            "INFO",
            "Thumbnail generated",
            Some("thumbnail_success"),
            Some(out_path.to_str().unwrap_or("")),
        );
    } else {
        log_json(
            "ERROR",
            "Thumbnail generation failed",
            Some("thumbnail_failed"),
            None,
        );
    }

    Ok(())
}

fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::new()
        .filter_level(LevelFilter::Info)
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .init();

    let args: Vec<String> = env::args().collect();
    if args.len() >= 3 && args[1] == "render" {
        let content = fs::read_to_string(&args[2])?;
        let analysis = serde_json::from_str::<Analysis>(&content)?;
        process_instruction(analysis)?;
        return Ok(());
    }

    if args.len() >= 3 && args[1] == "analyze" {
        let output_path = parse_out_arg(&args).unwrap_or_else(|| {
            data_path("json").join(format!("{}.json", file_stem(&args[2]).unwrap_or("job")))
        });
        analyze_video(Path::new(&args[2]), &output_path)?;
        return Ok(());
    }

    if args.len() >= 3 && args[1] == "run" {
        return run_video(Path::new(&args[2]), parse_out_arg(&args).as_deref());
    }

    if args.len() >= 2 && args[1] == "worker" {
        return watch_raw_jobs();
    }

    if args.len() >= 2 && matches!(args[1].as_str(), "help" | "--help" | "-h") {
        println!("Usage:");
        println!("  muscle                 Watch data/json for render jobs");
        println!("  muscle analyze <video> [--out <plan>]   Create an edit-plan JSON file");
        println!("  muscle render <plan>   Render a single edit-plan JSON file");
        println!("  muscle run <video> [--out <video>]      Analyze and render in one command");
        println!("  muscle worker          Watch data/raw and run analyze -> render");
        return Ok(());
    }

    watch_json_jobs()
}

fn watch_json_jobs() -> Result<()> {
    log_json("INFO", "Muscle service started", Some("startup"), None);

    let raw_dir = data_path("raw");
    let json_dir = data_path("json");
    let output_dir = data_path("output");
    let job_dir = data_path("jobs");

    // Create directories
    fs::create_dir_all(&raw_dir)?;
    fs::create_dir_all(&json_dir)?;
    fs::create_dir_all(&output_dir)?;
    fs::create_dir_all(&job_dir)?;

    // Setup watcher
    let (tx, rx) = channel();
    let config = Config::default().with_poll_interval(Duration::from_secs(2));
    let mut watcher: RecommendedWatcher = Watcher::new(tx, config)?;

    watcher.watch(&json_dir, RecursiveMode::NonRecursive)?;
    log_json(
        "INFO",
        "Watching directory",
        Some("watch_start"),
        json_dir.to_str(),
    );

    for res in rx {
        match res {
            Ok(event) => {
                if let notify::EventKind::Create(_) = event.kind {
                    for path in event.paths {
                        if path.extension().map_or(false, |ext| ext == "json") {
                            log_json(
                                "INFO",
                                "New analysis detected",
                                Some("file_detected"),
                                Some(path.to_str().unwrap_or("")),
                            );

                            std::thread::sleep(Duration::from_secs(1));

                            if let Ok(content) = fs::read_to_string(&path) {
                                match serde_json::from_str::<Analysis>(&content) {
                                    Ok(analysis) => {
                                        // V14 DEBUG: Check deserialization of SE events
                                        if let Some(events) = &analysis.se_events {
                                            log_json(
                                                "INFO",
                                                &format!("Deserialized {} SE events", events.len()),
                                                Some("debug_se_count"),
                                                None,
                                            );
                                        } else {
                                            log_json(
                                                "WARN",
                                                "Deserialized SE events is NONE",
                                                Some("debug_se_count"),
                                                None,
                                            );
                                        }

                                        if let Err(e) = process_instruction(analysis) {
                                            log_json(
                                                "ERROR",
                                                &format!("Processing failed: {}", e),
                                                Some("process_error"),
                                                Some(path.to_str().unwrap_or("")),
                                            );
                                        }
                                    }
                                    Err(e) => log_json(
                                        "ERROR",
                                        &format!("JSON parse failed: {}", e),
                                        Some("parse_error"),
                                        Some(path.to_str().unwrap_or("")),
                                    ),
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => log_json(
                "ERROR",
                &format!("Watch error: {}", e),
                Some("watch_error"),
                None,
            ),
        }
    }

    Ok(())
}

fn watch_raw_jobs() -> Result<()> {
    log_json(
        "INFO",
        "Muscle worker started",
        Some("worker_startup"),
        None,
    );

    let raw_dir = data_path("raw");
    let json_dir = data_path("json");
    let output_dir = data_path("output");
    let job_dir = data_path("jobs");

    fs::create_dir_all(&raw_dir)?;
    fs::create_dir_all(&json_dir)?;
    fs::create_dir_all(&output_dir)?;
    fs::create_dir_all(&job_dir)?;

    for entry in fs::read_dir(&raw_dir)? {
        let path = entry?.path();
        if is_video_file(&path) {
            process_raw_job(&path);
        }
    }

    let (tx, rx) = channel();
    let config = Config::default().with_poll_interval(Duration::from_secs(2));
    let mut watcher: RecommendedWatcher = Watcher::new(tx, config)?;

    watcher.watch(&raw_dir, RecursiveMode::NonRecursive)?;
    log_json(
        "INFO",
        "Watching raw directory",
        Some("watch_raw_start"),
        raw_dir.to_str(),
    );

    for res in rx {
        match res {
            Ok(event) => {
                if matches!(
                    event.kind,
                    notify::EventKind::Create(_)
                        | notify::EventKind::Modify(_)
                        | notify::EventKind::Any
                ) {
                    for path in event.paths {
                        if is_video_file(&path) {
                            std::thread::sleep(Duration::from_secs(1));
                            process_raw_job(&path);
                        }
                    }
                }
            }
            Err(e) => log_json(
                "ERROR",
                &format!("Raw watch error: {}", e),
                Some("watch_raw_error"),
                None,
            ),
        }
    }

    Ok(())
}

fn parse_out_arg(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|pair| pair[0] == "--out" || pair[0] == "-o")
        .map(|pair| PathBuf::from(&pair[1]))
}

fn file_stem(path: &str) -> Option<&str> {
    Path::new(path).file_stem().and_then(|name| name.to_str())
}

fn is_video_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_lowercase().as_str(),
                "mp4" | "mov" | "avi" | "mkv" | "webm"
            )
        })
        .unwrap_or(false)
        && !path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.ends_with(".part"))
            .unwrap_or(false)
}

fn process_raw_job(input_path: &Path) {
    if let Err(error) = run_raw_job(input_path) {
        log_json(
            "ERROR",
            &format!("Raw job failed: {}", error),
            Some("raw_job_failed"),
            input_path.to_str(),
        );
    }
}

fn run_raw_job(input_path: &Path) -> Result<()> {
    if !input_path.exists() {
        return Ok(());
    }

    let file_name = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Raw video must have a valid UTF-8 filename")?;
    let job_id = file_stem(file_name).unwrap_or("job");
    let plan_path = data_path("json").join(format!("{}.json", file_name));

    if job_already_completed_or_rendering(job_id) {
        log_json(
            "INFO",
            "Skipping already handled raw job",
            Some("raw_job_skip"),
            input_path.to_str(),
        );
        return Ok(());
    }

    log_json(
        "INFO",
        "Processing raw upload with Rust worker",
        Some("raw_job_start"),
        input_path.to_str(),
    );

    analyze_video(input_path, &plan_path)?;
    let content = fs::read_to_string(&plan_path)?;
    let analysis = serde_json::from_str::<Analysis>(&content)?;
    process_instruction(analysis)?;
    Ok(())
}

fn job_already_completed_or_rendering(job_id: &str) -> bool {
    let path = data_path("jobs").join(format!("{}.json", job_id));
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        .and_then(|payload| payload["status"].as_str().map(|status| status.to_string()))
        .map(|status| matches!(status.as_str(), "analyzing" | "rendering" | "completed"))
        .unwrap_or(false)
}

fn analyze_video(input_path: &Path, output_path: &Path) -> Result<()> {
    let input_path = input_path
        .canonicalize()
        .with_context(|| format!("Video not found: {}", input_path.display()))?;
    let file_name = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Input video must have a valid UTF-8 filename")?
        .to_string();
    let job_id = file_stem(&file_name).unwrap_or("job").to_string();

    fs::create_dir_all(data_path("raw"))?;
    fs::create_dir_all(data_path("json"))?;
    fs::create_dir_all(data_path("jobs"))?;

    let raw_path = data_path("raw").join(&file_name);
    if raw_path.canonicalize().ok().as_ref() != Some(&input_path) {
        fs::copy(&input_path, &raw_path)
            .with_context(|| format!("Failed to copy video to {}", raw_path.display()))?;
    }

    update_job_status(
        &job_id,
        "analyzing",
        "Rust CLI is analyzing the video.",
        30,
        None,
        None,
    )?;

    let plan = match generate_edit_plan_with_gemini(&input_path, &file_name, &job_id) {
        Ok(plan) => plan,
        Err(error) => {
            log_json(
                "WARN",
                &format!("Gemini analyze failed; writing fallback plan: {}", error),
                Some("analyze_fallback"),
                None,
            );
            fallback_edit_plan(&file_name, &job_id, estimate_duration(&input_path))
        }
    };

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, serde_json::to_string_pretty(&plan)?)?;

    update_job_status(&job_id, "rendering", "Edit plan is ready.", 60, None, None)?;
    log_json(
        "INFO",
        "Edit plan saved",
        Some("analysis_saved"),
        output_path.to_str(),
    );

    Ok(())
}

fn run_video(input_path: &Path, requested_output_path: Option<&Path>) -> Result<()> {
    let plan_path = data_path("json").join(format!(
        "{}.json",
        input_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("job")
    ));
    analyze_video(input_path, &plan_path)?;

    let content = fs::read_to_string(&plan_path)?;
    let analysis = serde_json::from_str::<Analysis>(&content)?;
    let rendered_path = process_instruction(analysis)?;

    if let Some(requested_output_path) = requested_output_path {
        if let Some(parent) = requested_output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&rendered_path, requested_output_path).with_context(|| {
            format!(
                "Failed to copy rendered video from {} to {}",
                rendered_path.display(),
                requested_output_path.display()
            )
        })?;
        log_json(
            "INFO",
            "Copied rendered video to requested output path",
            Some("run_output_saved"),
            requested_output_path.to_str(),
        );
    }

    Ok(())
}

fn generate_edit_plan_with_gemini(
    input_path: &Path,
    file_name: &str,
    job_id: &str,
) -> Result<serde_json::Value> {
    let api_key = gemini_api_key()?;
    let metadata = fs::metadata(input_path)?;
    let model = env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-2.5-flash".to_string());
    let mime_type = video_mime_type(input_path);
    let prompt = edit_plan_prompt();

    let client = Client::new();
    let parts = if metadata.len() <= INLINE_VIDEO_LIMIT_BYTES {
        log_json(
            "INFO",
            "Using Gemini inline video input",
            Some("gemini_inline"),
            input_path.to_str(),
        );
        gemini_inline_parts(input_path, mime_type, prompt)?
    } else {
        log_json(
            "INFO",
            "Using Gemini Files API video input",
            Some("gemini_files_api"),
            input_path.to_str(),
        );
        let file_uri = upload_gemini_file(&client, &api_key, input_path, mime_type)?;
        vec![
            serde_json::json!({
                "file_data": {
                    "mime_type": mime_type,
                    "file_uri": file_uri
                }
            }),
            serde_json::json!({ "text": prompt }),
        ]
    };

    let mut plan = generate_edit_plan_from_parts(&client, &api_key, &model, parts)?;
    plan["original_filename"] = serde_json::json!(file_name);
    plan["job_id"] = serde_json::json!(job_id);

    Ok(plan)
}

fn gemini_api_key() -> Result<String> {
    env::var("GEMINI_API_KEY")
        .or_else(|_| env::var("GOOGLE_API_KEY"))
        .context("GEMINI_API_KEY is not set")
}

fn gemini_inline_parts(
    input_path: &Path,
    mime_type: &str,
    prompt: &str,
) -> Result<Vec<serde_json::Value>> {
    let video_bytes = fs::read(input_path)?;
    let encoded_video = general_purpose::STANDARD.encode(video_bytes);

    Ok(vec![
        serde_json::json!({
            "inline_data": {
                "mime_type": mime_type,
                "data": encoded_video
            }
        }),
        serde_json::json!({ "text": prompt }),
    ])
}

fn upload_gemini_file(
    client: &Client,
    api_key: &str,
    input_path: &Path,
    mime_type: &str,
) -> Result<String> {
    let video_bytes = fs::read(input_path)?;
    let display_name = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("video");
    let start_body = serde_json::json!({
        "file": {
            "display_name": display_name
        }
    });

    let start_response = client
        .post("https://generativelanguage.googleapis.com/upload/v1beta/files")
        .header("x-goog-api-key", api_key)
        .header("X-Goog-Upload-Protocol", "resumable")
        .header("X-Goog-Upload-Command", "start")
        .header("X-Goog-Upload-Header-Content-Length", video_bytes.len())
        .header("X-Goog-Upload-Header-Content-Type", mime_type)
        .header("Content-Type", "application/json")
        .json(&start_body)
        .send()
        .context("Failed to start Gemini file upload")?
        .error_for_status()
        .context("Gemini file upload start returned an error")?;

    let upload_url = start_response
        .headers()
        .get("x-goog-upload-url")
        .context("Gemini upload response did not include x-goog-upload-url")?
        .to_str()
        .context("Gemini upload URL header was not valid UTF-8")?
        .to_string();

    let file_info: serde_json::Value = client
        .post(upload_url)
        .header(CONTENT_LENGTH, video_bytes.len())
        .header("X-Goog-Upload-Offset", "0")
        .header("X-Goog-Upload-Command", "upload, finalize")
        .body(video_bytes)
        .send()
        .context("Failed to upload video bytes to Gemini")?
        .error_for_status()
        .context("Gemini file upload returned an error")?
        .json()
        .context("Failed to parse Gemini file upload response")?;

    file_info["file"]["uri"]
        .as_str()
        .map(|uri| uri.to_string())
        .context("Gemini file upload response did not include file.uri")
        .and_then(|uri| {
            if let Some(name) = file_info["file"]["name"].as_str() {
                wait_for_gemini_file_active(client, api_key, name)?;
            }
            Ok(uri)
        })
}

fn wait_for_gemini_file_active(client: &Client, api_key: &str, file_name: &str) -> Result<()> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/{}",
        file_name
    );
    for attempt in 1..=30 {
        let file: serde_json::Value = client
            .get(&url)
            .header("x-goog-api-key", api_key)
            .send()
            .context("Failed to poll Gemini file state")?
            .error_for_status()
            .context("Gemini file state polling returned an error")?
            .json()
            .context("Failed to parse Gemini file state response")?;

        match file["state"].as_str() {
            Some("ACTIVE") => return Ok(()),
            Some("FAILED") => anyhow::bail!("Gemini file processing failed"),
            Some("PROCESSING") | Some("STATE_UNSPECIFIED") | None => {
                log_json(
                    "INFO",
                    &format!("Waiting for Gemini file processing ({}/30)", attempt),
                    Some("gemini_file_processing"),
                    Some(file_name),
                );
                std::thread::sleep(Duration::from_secs(2));
            }
            Some(other) => anyhow::bail!("Unexpected Gemini file state: {}", other),
        }
    }

    anyhow::bail!("Timed out waiting for Gemini file processing")
}

fn generate_edit_plan_from_parts(
    client: &Client,
    api_key: &str,
    model: &str,
    parts: Vec<serde_json::Value>,
) -> Result<serde_json::Value> {
    let request_body = serde_json::json!({
        "contents": [{
            "parts": parts
        }],
        "generationConfig": {
            "response_mime_type": "application/json"
        }
    });

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        model
    );
    let response: serde_json::Value = client
        .post(url)
        .header("x-goog-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .context("Failed to call Gemini")?
        .error_for_status()
        .context("Gemini returned an error")?
        .json()
        .context("Failed to parse Gemini response")?;

    let text = response["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .context("Gemini response did not contain JSON text")?;

    serde_json::from_str(&strip_json_fence(text)).context("Gemini edit plan was not valid JSON")
}

fn edit_plan_prompt() -> &'static str {
    r#"Analyze this video and output strict JSON only.

Use this shape:
{
  "cuts": [
    {
      "start_time": "HH:MM:SS",
      "end_time": "HH:MM:SS",
      "description": "Short description",
      "filter": "none",
      "transition_type": "fade",
      "focus_point": 0.5,
      "caption": "Short, punchy text overlay",
      "caption_style": {
        "font": "sans",
        "color": "white",
        "position": "bottom",
        "box": true,
        "background_asset": "none"
      }
    }
  ],
  "editing_style": {
    "tempo": "fast",
    "mood": "exciting"
  },
  "se_events": [
    {
      "timestamp": "HH:MM:SS",
      "type": "impact",
      "tag": "serious"
    }
  ],
  "visual_effects": [
    {
      "start": "HH:MM:SS",
      "end": "HH:MM:SS",
      "type": "zoom_in",
      "speed": "fast"
    }
  ],
  "thumbnail": {
    "timestamp": "HH:MM:SS",
    "text": "SHORT TITLE",
    "color": "yellow"
  }
}

Be selective. Prefer fewer stronger cuts over many weak cuts. Captions should be short and readable."#
}

fn strip_json_fence(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(body) = trimmed.strip_prefix("```json") {
        return body.trim_end_matches("```").trim().to_string();
    }
    if let Some(body) = trimmed.strip_prefix("```") {
        return body.trim_end_matches("```").trim().to_string();
    }
    trimmed.to_string()
}

fn fallback_edit_plan(file_name: &str, job_id: &str, duration_seconds: f64) -> serde_json::Value {
    serde_json::json!({
        "job_id": job_id,
        "original_filename": file_name,
        "bgm_path": null,
        "cuts": [{
            "start_time": "00:00:00",
            "end_time": seconds_to_hhmmss(duration_seconds),
            "description": "Fallback full-length clip",
            "filter": "none",
            "transition_type": "fade",
            "focus_point": 0.5,
            "caption": null,
            "caption_style": {
                "font": "sans",
                "color": "white",
                "position": "bottom",
                "box": false,
                "background_asset": "none"
            }
        }],
        "editing_style": {
            "tempo": "slow",
            "mood": "fallback"
        },
        "se_events": [],
        "visual_effects": [],
        "thumbnail": null
    })
}

fn estimate_duration(input_path: &Path) -> f64 {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(input_path)
        .output();

    output
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|duration| duration.trim().parse::<f64>().ok())
        .filter(|duration| *duration > 0.0)
        .unwrap_or(60.0)
}

fn seconds_to_hhmmss(seconds: f64) -> String {
    let total = seconds.ceil().max(1.0) as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn video_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        _ => "video/mp4",
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

        (
            font.to_string(),
            color.to_string(),
            box_conf.to_string(),
            y.to_string(),
        )
    } else {
        (
            default_font.to_string(),
            "white".to_string(),
            "".to_string(),
            "h*0.85".to_string(),
        )
    }
}

fn get_se_file(tag: &str) -> PathBuf {
    let base = data_path("se");
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
    // V14 FORCE: Blindly return path. Do not check exists().
    // Docker bind mounts sometimes confuse Rust's exists() check.
    // FFmpeg will error if file is missing, which is better than silence.
    log_json(
        "INFO",
        &format!("Selected SAFE SE for '{}': {:?}", tag, candidate),
        Some("se_selection"),
        None,
    );
    candidate
}

// NEW SIMPLIFIED IMPLEMENTATION
// Process video using segment-based approach to avoid filter_complex limitations

fn process_instruction(analysis: Analysis) -> Result<PathBuf> {
    let job_id = analysis.job_id.clone().unwrap_or_else(|| {
        Path::new(&analysis.original_filename)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or(&analysis.original_filename)
            .to_string()
    });
    let video_path = data_path("raw").join(&analysis.original_filename);
    let output_filename = format!(
        "{}_edited.mp4",
        Path::new(&analysis.original_filename)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or(&analysis.original_filename)
    );
    let output_path = data_path("output").join(&output_filename);
    let temp_dir = data_path("temp");

    // Create temp directory
    fs::create_dir_all(&temp_dir)?;

    // BGM path with fallback to default
    let default_bgm_path = data_path("bgm").join("default_bgm.mp3");
    let bgm_path_str = analysis
        .bgm_path
        .clone()
        .unwrap_or_else(|| default_bgm_path.to_string_lossy().to_string());
    let mut bgm_path_buf = PathBuf::from(&bgm_path_str);

    // If BGM file doesn't exist, try default_bgm.mp3
    if !bgm_path_buf.exists() {
        if default_bgm_path.exists() {
            bgm_path_buf = default_bgm_path;
        }
    }

    let has_bgm = bgm_path_buf.exists();

    log_json(
        "INFO",
        &format!("Processing video: {:?}", video_path),
        Some("process_start"),
        None,
    );
    log_json(
        "INFO",
        &format!("BGM: {:?}, exists: {}", bgm_path_buf, has_bgm),
        Some("bgm_check"),
        None,
    );
    update_job_status(
        &job_id,
        "rendering",
        "Muscle is rendering the final video.",
        70,
        None,
        None,
    )?;

    // Step 1: Process each cut as individual segment
    let mut segment_paths = Vec::new();

    for (i, cut) in analysis.cuts.iter().enumerate() {
        let segment_path =
            process_single_segment(i, cut, &video_path, &temp_dir, &analysis.visual_effects)?;
        segment_paths.push(segment_path);
    }

    log_json(
        "INFO",
        &format!("Processed {} segments", segment_paths.len()),
        Some("segments_complete"),
        None,
    );

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
        .arg("-f")
        .arg("concat")
        .arg("-safe")
        .arg("0")
        .arg("-i")
        .arg(&concat_file);

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
                log_json(
                    "INFO",
                    &format!(
                        "Processing SE: type='{}', path='{:?}'",
                        se.event_type, se_file
                    ),
                    Some("se_debug"),
                    None,
                );

                // V14 FORCE: Skipping exists() check. Trust get_se_file.
                let delay_ms = parse_time(&se.timestamp).unwrap_or(0.0) * 1000.0;
                concat_cmd.arg("-i").arg(&se_file);
                // Add delay and volume adjustment for SE
                // V14 ADJUSTMENT: Boost synth SE volume to 0.8 (was 0.2)
                // Synthetic assets are quieter/unmastered, so they need more gain.
                let filter_part = format!(
                    "[{}:a]adelay={}|{},volume=0.8[se{}]",
                    input_index, delay_ms as i64, delay_ms as i64, input_index
                );
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
        let fade_start = if early_total_duration > 2.0 {
            early_total_duration - 2.0
        } else {
            0.0
        };

        let filter_str = if filter_parts.is_empty() {
            format!(
                "{}amix=inputs={}:duration=first,afade=t=out:st={:.3}:d=2[aout]",
                input_labels.join(""),
                num_inputs,
                fade_start
            )
        } else {
            format!(
                "{};{}amix=inputs={}:duration=first,afade=t=out:st={:.3}:d=2[aout]",
                filter_parts.join(";"),
                input_labels.join(""),
                num_inputs,
                fade_start
            )
        };

        log_json(
            "INFO",
            &format!("Audio filter: {}", filter_str),
            Some("filter_debug"),
            None,
        );

        concat_cmd
            .arg("-filter_complex")
            .arg(&filter_str)
            .arg("-map")
            .arg("0:v")
            .arg("-map")
            .arg("[aout]");
    }

    // Calculate total duration to strictly limit output
    let mut total_duration = 0.0;
    for cut in &analysis.cuts {
        if let (Ok(start), Ok(end)) = (parse_time(&cut.start_time), parse_time(&cut.end_time)) {
            total_duration += end - start;
        }
    }

    let output = concat_cmd
        .arg("-c:v")
        .arg("copy") // Copy video (already encoded)
        .arg("-c:a")
        .arg("aac")
        .arg("-t")
        .arg(format!("{:.3}", total_duration)) // Force output duration to match video content
        .arg(&output_path)
        .output()?;

    if output.status.success() {
        log_json(
            "INFO",
            "Video processing complete",
            Some("transcode_complete"),
            Some(output_path.to_str().unwrap_or("")),
        );
        update_job_status(
            &job_id,
            "completed",
            "Your video is ready.",
            100,
            Some(&format!("/output/{}", output_filename)),
            None,
        )?;
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log_json(
            "ERROR",
            &format!("Concatenation failed: {}", stderr),
            Some("transcode_failed"),
            Some(output_path.to_str().unwrap_or("")),
        );
        update_job_status(&job_id, "failed", &stderr, 100, None, None)?;
        return Err(anyhow::anyhow!("Concatenation failed: {}", stderr));
    }

    // Step 4: Generate thumbnail
    if let Some(thumb) = &analysis.thumbnail {
        if let Err(e) = generate_thumbnail(
            &video_path,
            thumb,
            &data_path("output").to_string_lossy(),
            &analysis.original_filename,
        ) {
            log_json(
                "ERROR",
                &format!("Thumbnail generation failed: {}", e),
                Some("thumbnail_error"),
                None,
            );
        } else {
            update_job_status(
                &job_id,
                "completed",
                "Your video and thumbnail are ready.",
                100,
                Some(&format!("/output/{}", output_filename)),
                Some(&format!("/output/{}_thumb.jpg", analysis.original_filename)),
            )?;
        }
    }

    // Cleanup temp files
    for seg in &segment_paths {
        let _ = fs::remove_file(seg);
    }
    let _ = fs::remove_file(&concat_file);

    Ok(output_path)
}

fn update_job_status(
    job_id: &str,
    status: &str,
    message: &str,
    progress: i32,
    output_url: Option<&str>,
    thumbnail_url: Option<&str>,
) -> Result<()> {
    let job_dir = data_path("jobs");
    fs::create_dir_all(&job_dir)?;
    let path = job_dir.join(format!("{}.json", job_id));
    let mut payload = if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str::<serde_json::Value>(&content)
            .unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    payload["job_id"] = serde_json::json!(job_id);
    payload["status"] = serde_json::json!(status);
    payload["message"] = serde_json::json!(message);
    payload["progress"] = serde_json::json!(progress);
    payload["updated_at"] = serde_json::json!(unix_timestamp());
    if let Some(url) = output_url {
        payload["output_url"] = serde_json::json!(url);
    }
    if let Some(url) = thumbnail_url {
        payload["thumbnail_url"] = serde_json::json!(url);
    }

    fs::write(path, serde_json::to_string_pretty(&payload)?)?;
    Ok(())
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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
    filters.push(format!(
        "scale=-2:1920,crop=1080:1920:(iw-1080)*{}:0",
        focus
    ));

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
                        "zoom_in" => filters.push(
                            "crop=iw/1.25:ih/1.25:(iw-out_w)/2:(ih-out_h)/2,scale=1080:1920"
                                .to_string(),
                        ),
                        "zoom_out" => filters.push(
                            "crop=iw/1.1:ih/1.1:(iw-out_w)/2:(ih-out_h)/2,scale=1080:1920"
                                .to_string(),
                        ),
                        _ => {}
                    }
                    break;
                }
            }
        }
    }

    // 4. Caption
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

    let video_filter = filters.join(",");

    // Run ffmpeg to extract and process this segment
    // CRITICAL: -ss BEFORE -i for accurate seeking
    let output = Command::new("ffmpeg")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{:.3}", start_seconds)) // Seek BEFORE input
        .arg("-i")
        .arg(video_path)
        .arg("-t")
        .arg(format!("{:.3}", duration)) // Duration after input
        .arg("-vf")
        .arg(&video_filter)
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("fast")
        .arg("-crf")
        .arg("23")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("128k")
        .arg(&segment_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Segment {} failed: {}", index, stderr));
    }

    log_json(
        "INFO",
        &format!("Segment {} complete", index),
        Some("segment_done"),
        None,
    );
    Ok(segment_path)
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
