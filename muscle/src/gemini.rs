//! Optional cloud analyzer: asks Gemini to write the edit plan.

use crate::assets;
use crate::jobs::log_json;
use crate::plan::{EditPlan, TimedText};
use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use reqwest::blocking::Client;
use reqwest::header::CONTENT_LENGTH;
use std::env;
use std::fs;
use std::path::Path;
use std::time::Duration;

const INLINE_VIDEO_LIMIT_BYTES: u64 = 18 * 1024 * 1024;

/// API root; overridable so tests can point at a local mock server.
fn base_url() -> String {
    env::var("NUE_GEMINI_BASE_URL")
        .unwrap_or_else(|_| "https://generativelanguage.googleapis.com".to_string())
        .trim_end_matches('/')
        .to_string()
}

pub fn api_key() -> Option<String> {
    env::var("GEMINI_API_KEY")
        .or_else(|_| env::var("GOOGLE_API_KEY"))
        .ok()
        .filter(|key| !key.trim().is_empty() && key != "your_api_key_here")
}

pub fn generate_edit_plan(
    input_path: &Path,
    file_name: &str,
    job_id: &str,
    transcript: &[TimedText],
    script: Option<&str>,
) -> Result<EditPlan> {
    let api_key = api_key().context("GEMINI_API_KEY is not set")?;
    let metadata = fs::metadata(input_path)?;
    let model = env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-2.5-flash".to_string());
    let mime_type = video_mime_type(input_path);
    let prompt = edit_plan_prompt(transcript, script);

    let client = Client::builder()
        .timeout(Duration::from_secs(600))
        .build()?;
    let video_part = if metadata.len() <= INLINE_VIDEO_LIMIT_BYTES {
        log_json(
            "INFO",
            "Using Gemini inline video input",
            Some("gemini_inline"),
            input_path.to_str(),
        );
        serde_json::json!({
            "inline_data": {
                "mime_type": mime_type,
                "data": general_purpose::STANDARD.encode(fs::read(input_path)?)
            }
        })
    } else {
        log_json(
            "INFO",
            "Using Gemini Files API video input",
            Some("gemini_files_api"),
            input_path.to_str(),
        );
        let file_uri = upload_file(&client, &api_key, input_path, mime_type)?;
        serde_json::json!({ "file_data": { "mime_type": mime_type, "file_uri": file_uri } })
    };
    let parts = vec![video_part, serde_json::json!({ "text": prompt })];

    let mut value = generate_from_parts(&client, &api_key, &model, parts)?;
    value["original_filename"] = serde_json::json!(file_name);
    value["job_id"] = serde_json::json!(job_id);
    value["analyzer"] = serde_json::json!("gemini");
    serde_json::from_value(value).context("Gemini edit plan did not match the schema")
}

fn upload_file(
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

    let start_response = client
        .post(format!("{}/upload/v1beta/files", base_url()))
        .header("x-goog-api-key", api_key)
        .header("X-Goog-Upload-Protocol", "resumable")
        .header("X-Goog-Upload-Command", "start")
        .header("X-Goog-Upload-Header-Content-Length", video_bytes.len())
        .header("X-Goog-Upload-Header-Content-Type", mime_type)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "file": { "display_name": display_name } }))
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

    let uri = file_info["file"]["uri"]
        .as_str()
        .map(|uri| uri.to_string())
        .context("Gemini file upload response did not include file.uri")?;
    if let Some(name) = file_info["file"]["name"].as_str() {
        wait_for_file_active(client, api_key, name)?;
    }
    Ok(uri)
}

fn wait_for_file_active(client: &Client, api_key: &str, file_name: &str) -> Result<()> {
    let url = format!("{}/v1beta/{}", base_url(), file_name);
    for attempt in 1..=60 {
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
                    &format!("Waiting for Gemini file processing ({}/60)", attempt),
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

fn generate_from_parts(
    client: &Client,
    api_key: &str,
    model: &str,
    parts: Vec<serde_json::Value>,
) -> Result<serde_json::Value> {
    let request_body = serde_json::json!({
        "contents": [{ "parts": parts }],
        "generationConfig": { "response_mime_type": "application/json" }
    });
    let url = format!("{}/v1beta/models/{}:generateContent", base_url(), model);
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

fn edit_plan_prompt(transcript: &[TimedText], script: Option<&str>) -> String {
    let sounds = assets::sound_effect_names().join(", ");
    let icons = assets::icon_names().join(", ");
    let mut prompt = format!(
        r#"You are a short-form video editor. Analyze this video and output strict JSON only.
Timestamps are positions in the source video, formatted "HH:MM:SS.mmm".

Use this shape:
{{
  "cuts": [
    {{ "start_time": "00:00:00.000", "end_time": "00:00:04.500",
       "description": "Short description", "focus_point": 0.5,
       "caption": null }}
  ],
  "telops": [
    {{ "start": "00:00:01.000", "end": "00:00:03.000", "text": "Short punchy emphasis",
       "style": {{ "color": "yellow", "position": "top", "box": true }} }}
  ],
  "editing_style": {{ "tempo": "fast", "mood": "exciting" }},
  "se_events": [ {{ "timestamp": "00:00:02.000", "type": "impact" }} ],
  "visual_effects": [ {{ "start": "00:00:02.000", "end": "00:00:03.000", "type": "zoom_in" }} ],
  "overlays": [ {{ "asset": "exclaim", "start": "00:00:02.000", "end": "00:00:03.000", "position": "top_right" }} ],
  "thumbnail": {{ "timestamp": "00:00:02.000", "text": "SHORT TITLE", "color": "yellow" }}
}}

Rules:
- Cuts are the parts to KEEP, in order. Drop dead air, filler words (えー, あのー, um), false starts and repeated takes.
- focus_point is the horizontal center (0 left .. 1 right) of the speaker or subject; the vertical crop is centered on it.
- Open with a hook: the first telop (0-2.5 s of the first cut) is the video's most intriguing line, style {{"position": "center", "size": "large"}}.
- Alternate framing across jump cuts: give every other cut a "punch_in" effect spanning the cut; give long static cuts a "push_in".
- Sound effect "type" must be one of: {sounds}.
- Visual effect "type" is one of: zoom_in (short emphasis), zoom_out, pan_left, pan_right, punch_in (whole cut, 1.12x), push_in (slow 6% push over the cut).
- Overlay "asset" must be one of: {icons}.
- Be selective: fewer, stronger moments beat many weak ones. Telops are 4-14 characters when Japanese.
- Write telops and the thumbnail title in the language spoken in the video."#
    );
    if !transcript.is_empty() {
        prompt.push_str("\n\nTranscript (source timestamps):\n");
        for line in transcript.iter().take(400) {
            prompt.push_str(&format!("{} - {}: {}\n", line.start, line.end, line.text));
        }
    }
    if let Some(script) = script.filter(|s| !s.trim().is_empty()) {
        prompt.push_str("\n\nCreator's notes / script:\n");
        prompt.push_str(script);
    }
    prompt
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_markdown_fences() {
        assert_eq!(strip_json_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_json_fence(" {\"a\":1} "), "{\"a\":1}");
    }
}
