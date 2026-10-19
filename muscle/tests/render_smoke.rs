//! End-to-end: synthesize a clip with ffmpeg, run `muscle run`, check outputs.
//! Skipped when ffmpeg is not installed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn has_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            fs::copy(&path, to.join(path.file_name().unwrap())).unwrap();
        }
    }
}

fn workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nue-test-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let repo_data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
    for sub in ["se", "icons", "bgm"] {
        copy_dir(&repo_data.join(sub), &dir.join("data").join(sub));
    }
    dir
}

fn ffprobe(path: &Path, entries: &str) -> String {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            entries,
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn renders_talking_clip_with_jump_cuts() {
    if !has_ffmpeg() {
        eprintln!("ffmpeg not installed; skipping");
        return;
    }
    let dir = workspace("talk");
    let input = dir.join("clip.mp4");
    // 2s tone, 3s silence, 2s louder tone: the silent middle should be cut.
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=640x360:r=30:d=7",
        ])
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:d=2",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=44100:cl=mono:d=3",
        ])
        .args(["-f", "lavfi", "-i", "sine=f=660:d=2"])
        .args([
            "-filter_complex",
            "[1:a][2:a][3:a]concat=n=3:v=0:a=1[a]",
            "-map",
            "0:v",
            "-map",
            "[a]",
        ])
        .args(["-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac"])
        .arg(&input)
        .status()
        .unwrap();
    assert!(status.success());
    fs::write(
        dir.join("clip.srt"),
        "1\n00:00:00,200 --> 00:00:01,800\nテスト字幕\n\n2\n00:00:05,200 --> 00:00:06,800\nsecond line\n",
    )
    .unwrap();
    let script = dir.join("script.txt");
    fs::write(&script, "00:00:00.5 - ポイント\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_muscle"))
        .args(["run"])
        .arg(&input)
        .args(["--analyzer", "local", "--script"])
        .arg(&script)
        .env("NUE_DATA_DIR", dir.join("data"))
        .env_remove("GEMINI_API_KEY")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let out_dir = dir.join("data/output");
    let video = out_dir.join("clip_edited.mp4");
    assert_eq!(
        ffprobe(&video, "stream=width,height")
            .split_whitespace()
            .collect::<Vec<_>>(),
        ["1080", "1920"]
    );
    let duration: f64 = ffprobe(&video, "format=duration").trim().parse().unwrap();
    assert!(
        duration > 3.5 && duration < 5.5,
        "silence not cut: {duration}"
    );
    assert!(out_dir.join("clip_thumb.jpg").exists());

    let srt = fs::read_to_string(out_dir.join("clip_edited.srt")).unwrap();
    assert!(srt.contains("テスト字幕"));
    assert!(srt.contains("second line"));

    let plan: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out_dir.join("clip_plan.json")).unwrap()).unwrap();
    assert_eq!(plan["analyzer"], "local");
    assert_eq!(plan["cuts"].as_array().unwrap().len(), 2);
    assert_eq!(plan["telops"][0]["text"], "ポイント");

    let job: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("data/jobs/clip.json")).unwrap())
            .unwrap();
    assert_eq!(job["status"], "completed");
    assert_eq!(job["subtitle_url"], "/output/clip_edited.srt");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn renders_silent_clip_horizontally() {
    if !has_ffmpeg() {
        eprintln!("ffmpeg not installed; skipping");
        return;
    }
    let dir = workspace("silent");
    let input = dir.join("silent.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=640x360:r=25:d=4",
        ])
        .args(["-c:v", "libx264", "-preset", "ultrafast"])
        .arg(&input)
        .status()
        .unwrap();
    assert!(status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_muscle"))
        .args(["run"])
        .arg(&input)
        .args(["--format", "horizontal", "--no-bgm"])
        .env("NUE_DATA_DIR", dir.join("data"))
        .env_remove("GEMINI_API_KEY")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let video = dir.join("data/output/silent_edited.mp4");
    let streams = ffprobe(&video, "stream=codec_type");
    assert!(streams.contains("video") && streams.contains("audio"));
    assert_eq!(
        ffprobe(&video, "stream=width").split_whitespace().next(),
        Some("1920")
    );
    let _ = fs::remove_dir_all(&dir);
}
