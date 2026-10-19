//! Exercises the Gemini analyzer against a local mock of the REST API:
//! request shape, fenced-JSON parsing, and merging with the local transcript.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

const PLAN: &str = r#"```json
{
  "cuts": [{"start_time": "00:00:00.500", "end_time": "00:00:02.500", "focus_point": 0.3}],
  "telops": [{"start": "00:00:01.000", "end": "00:00:02.000", "text": "AIテロップ",
              "style": {"color": "pink", "position": "top", "box": true}}],
  "se_events": [{"timestamp": "00:00:01.000", "type": "ding"}],
  "visual_effects": null,
  "overlays": [{"asset": "heart", "start": "00:00:01.000", "end": "00:00:02.000"}],
  "thumbnail": {"timestamp": "00:00:01.000", "text": "GEMINI"}
}
```"#;

struct Captured {
    path: String,
    api_key: String,
    body: String,
}

fn serve(listener: TcpListener, captured: Arc<Mutex<Vec<Captured>>>) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let mut length = 0usize;
        let mut api_key = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            let lower = line.to_lowercase();
            if let Some(v) = lower.strip_prefix("content-length:") {
                length = v.trim().parse().unwrap();
            }
            if lower.starts_with("x-goog-api-key:") {
                api_key = line[15..].trim().to_string();
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        let path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .to_string();
        let response = serde_json::json!({
            "candidates": [{ "content": { "parts": [{ "text": PLAN }] } }]
        })
        .to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.len(),
            response
        )
        .unwrap();
        captured.lock().unwrap().push(Captured {
            path,
            api_key,
            body: String::from_utf8_lossy(&body).to_string(),
        });
    }
}

#[test]
fn gemini_plan_is_requested_parsed_and_rendered() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("ffmpeg not installed; skipping");
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server_log = captured.clone();
    thread::spawn(move || serve(listener, server_log));

    let dir = std::env::temp_dir().join(format!("nue-gemini-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let repo_data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
    for sub in ["se", "icons"] {
        let to = dir.join("data").join(sub);
        fs::create_dir_all(&to).unwrap();
        for entry in fs::read_dir(repo_data.join(sub)).unwrap() {
            let path = entry.unwrap().path();
            fs::copy(&path, to.join(path.file_name().unwrap())).unwrap();
        }
    }
    let input = dir.join("clip.mp4");
    assert!(Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=640x360:r=30:d=4"
        ])
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:d=4",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a",
            "aac"
        ])
        .arg(&input)
        .status()
        .unwrap()
        .success());
    fs::write(
        dir.join("clip.srt"),
        "1\n00:00:01,000 --> 00:00:02,000\nlocal subtitle\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_muscle"))
        .arg("run")
        .arg(&input)
        .args(["--analyzer", "gemini", "--no-bgm"])
        .env("NUE_DATA_DIR", dir.join("data"))
        .env("GEMINI_API_KEY", "test-key")
        .env("GEMINI_MODEL", "gemini-test")
        .env("NUE_GEMINI_BASE_URL", format!("http://127.0.0.1:{port}"))
        .env("NO_PROXY", "127.0.0.1")
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .env_remove("ALL_PROXY")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(
        !stderr.contains("analyze_fallback"),
        "fell back to local: {stderr}"
    );

    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].path,
        "/v1beta/models/gemini-test:generateContent"
    );
    assert_eq!(requests[0].api_key, "test-key");
    let body: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
    let parts = body["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts[0]["inline_data"]["mime_type"], "video/mp4");
    let prompt = parts[1]["text"].as_str().unwrap();
    assert!(
        prompt.contains("local subtitle"),
        "transcript not sent to Gemini"
    );
    assert!(
        prompt.contains("ding") && prompt.contains("heart"),
        "asset names not sent"
    );

    let plan: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("data/output/clip_plan.json")).unwrap())
            .unwrap();
    assert_eq!(plan["analyzer"], "gemini");
    assert_eq!(plan["cuts"][0]["start_time"], "00:00:00.500");
    assert_eq!(plan["telops"][0]["text"], "AIテロップ");
    assert_eq!(plan["subtitles"][0]["text"], "local subtitle");

    let duration = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(dir.join("data/output/clip_edited.mp4"))
        .output()
        .unwrap();
    let duration: f64 = String::from_utf8_lossy(&duration.stdout)
        .trim()
        .parse()
        .unwrap();
    assert!(
        (duration - 2.0).abs() < 0.2,
        "expected the 2s Gemini cut, got {duration}"
    );
    let _ = fs::remove_dir_all(&dir);
}
