//! Shared data directory layout, structured logging and job status files.

use anyhow::Result;
use serde::Serialize;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_DATA_DIR: &str = "/app/data";

pub fn data_dir() -> PathBuf {
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

pub fn data_path(name: &str) -> PathBuf {
    data_dir().join(name)
}

#[derive(Serialize)]
struct LogEntry<'a> {
    severity: &'a str,
    message: &'a str,
    event: Option<&'a str>,
    path: Option<&'a str>,
}

pub fn log_json(level: &str, message: &str, event: Option<&str>, path: Option<&str>) {
    let entry = LogEntry {
        severity: level,
        message,
        event,
        path,
    };
    if let Ok(json) = serde_json::to_string(&entry) {
        eprintln!("{}", json);
    }
}

#[derive(Default)]
pub struct JobUrls<'a> {
    pub output_url: Option<&'a str>,
    pub thumbnail_url: Option<&'a str>,
    pub subtitle_url: Option<&'a str>,
    pub plan_url: Option<&'a str>,
}

pub fn update_job_status(
    job_id: &str,
    status: &str,
    message: &str,
    progress: i32,
    urls: JobUrls,
) -> Result<()> {
    let job_dir = data_path("jobs");
    fs::create_dir_all(&job_dir)?;
    let path = job_dir.join(format!("{}.json", job_id));
    let mut payload = fs::read_to_string(&path)
        .ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        .filter(|value| value.is_object())
        .unwrap_or_else(|| serde_json::json!({}));

    payload["job_id"] = serde_json::json!(job_id);
    payload["status"] = serde_json::json!(status);
    payload["message"] = serde_json::json!(message);
    payload["progress"] = serde_json::json!(progress);
    payload["updated_at"] = serde_json::json!(unix_timestamp());
    for (key, value) in [
        ("output_url", urls.output_url),
        ("thumbnail_url", urls.thumbnail_url),
        ("subtitle_url", urls.subtitle_url),
        ("plan_url", urls.plan_url),
    ] {
        if let Some(value) = value {
            payload[key] = serde_json::json!(value);
        }
    }

    // Write then rename so the web UI never reads a half-written file.
    let tmp_path = job_dir.join(format!(".{}.json.tmp", job_id));
    fs::write(&tmp_path, serde_json::to_string_pretty(&payload)?)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

pub fn read_job_status(job_id: &str) -> Option<String> {
    let path = data_path("jobs").join(format!("{}.json", job_id));
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        .and_then(|payload| payload["status"].as_str().map(|status| status.to_string()))
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
