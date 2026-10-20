mod assets;
mod captions;
mod gemini;
mod jobs;
mod local;
mod look;
mod media;
mod meta;
mod motion;
mod plan;
mod render;
mod style;
mod subtitles;

use anyhow::{bail, Context, Result};
use jobs::{data_path, log_json, read_job_status, update_job_status, JobUrls};
use meta::{JobMetadata, JobOptions};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use plan::EditPlan;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::Duration;

const USAGE: &str = "\
Nue - automatic video editor

Usage:
  muscle analyze <video> [--out <plan.json>] [options]   Create an edit-plan JSON file
  muscle render <plan.json> [--out <video.mp4>]          Render an edit plan
  muscle run <video> [--out <video.mp4>] [options]       Analyze and render in one command
  muscle worker                                          Watch data/raw and run analyze -> render
  muscle watch                                           Watch data/json and render new plans
  muscle styles                                          List editing formats

Options:
  --analyzer <auto|local|gemini>   auto uses Gemini only when GEMINI_API_KEY is set (default: auto)
  --format <vertical|horizontal|square>
  --style <shorts|vlog|variety|cinematic|business|name>   Editing format (data/styles/<name>.json for custom)
  --meta <file.json>               Job metadata (script, manual_instructions, options)
  --script <file.txt>              Notes; lines like \"00:00:15 - text\" become telops
  --no-bgm  --no-se  --no-subtitles  --no-jump-cut

Environment:
  NUE_DATA_DIR        data directory (default /app/data or ./data)
  NUE_ANALYZER        default analyzer
  NUE_STYLE           default editing format (shorts)
  NUE_WHISPER_MODEL   whisper.cpp ggml model for subtitles (or put ggml-*.bin in data/models)
  NUE_WHISPER_BIN     whisper.cpp CLI binary (default whisper-cli)
  NUE_LANGUAGE        speech language for subtitles (default auto)
  GEMINI_API_KEY      enables the Gemini analyzer
";

#[derive(Default)]
struct Cli {
    positional: Vec<String>,
    out: Option<PathBuf>,
    meta: Option<PathBuf>,
    script: Option<PathBuf>,
    options: JobOptions,
}

fn parse_cli(args: &[String]) -> Result<Cli> {
    let mut cli = Cli::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| {
            iter.next()
                .cloned()
                .with_context(|| format!("{} needs a value", name))
        };
        match arg.as_str() {
            "--out" | "-o" => cli.out = Some(PathBuf::from(value(arg)?)),
            "--meta" => cli.meta = Some(PathBuf::from(value(arg)?)),
            "--script" => cli.script = Some(PathBuf::from(value(arg)?)),
            "--analyzer" => cli.options.analyzer = Some(value(arg)?),
            "--format" => cli.options.format = Some(value(arg)?),
            "--style" => cli.options.style = Some(value(arg)?),
            "--no-bgm" => cli.options.generate_bgm = Some(false),
            "--no-se" => cli.options.auto_sound_effects = Some(false),
            "--no-subtitles" => cli.options.subtitles = Some(false),
            "--no-jump-cut" => cli.options.jump_cut = Some(false),
            flag if flag.starts_with("--") => bail!("Unknown option: {}\n\n{}", flag, USAGE),
            _ => cli.positional.push(arg.clone()),
        }
    }
    Ok(cli)
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        print!("{}", USAGE);
        return Ok(());
    };
    let cli = parse_cli(&args[1..])?;
    let first = || {
        cli.positional
            .first()
            .map(PathBuf::from)
            .with_context(|| format!("Missing file argument\n\n{}", USAGE))
    };

    match command {
        "analyze" => {
            let input = first()?;
            let out = cli
                .out
                .clone()
                .unwrap_or_else(|| data_path("json").join(format!("{}.json", file_stem(&input))));
            let metadata = load_cli_metadata(&cli, &input)?;
            analyze_video(&input, &out, &metadata)?;
            println!("{}", out.display());
        }
        "render" => {
            let content = fs::read_to_string(first()?)?;
            let plan: EditPlan = serde_json::from_str(&content).context("Invalid edit plan")?;
            let output = render::render_plan(&plan)?;
            print_output(&output, cli.out.as_deref())?;
        }
        "run" => {
            let input = first()?;
            let metadata = load_cli_metadata(&cli, &input)?;
            let plan_path = data_path("json").join(format!("{}.json", file_stem(&input)));
            let plan = analyze_video(&input, &plan_path, &metadata)?;
            let output = render::render_plan(&plan)?;
            print_output(&output, cli.out.as_deref())?;
        }
        "worker" => watch_raw_jobs()?,
        "watch" => watch_json_jobs()?,
        "styles" => {
            for s in style::list() {
                println!("{}\t{}\t{}", s.name, s.label, s.description);
            }
        }
        "help" | "--help" | "-h" => print!("{}", USAGE),
        other => bail!("Unknown command: {}\n\n{}", other, USAGE),
    }
    Ok(())
}

fn print_output(output: &render::RenderOutput, requested: Option<&Path>) -> Result<()> {
    let rendered = output.video.as_path();
    let final_path = match requested {
        Some(requested) => {
            if let Some(parent) = requested.parent().filter(|p| !p.as_os_str().is_empty()) {
                fs::create_dir_all(parent)?;
            }
            fs::copy(rendered, requested).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    rendered.display(),
                    requested.display()
                )
            })?;
            requested.to_path_buf()
        }
        None => rendered.to_path_buf(),
    };
    println!("{}", final_path.display());
    for extra in [&output.thumbnail, &output.subtitles, &output.plan]
        .into_iter()
        .flatten()
    {
        println!("{}", extra.display());
    }
    Ok(())
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("job")
        .to_string()
}

fn metadata_sidecar(video: &Path) -> PathBuf {
    PathBuf::from(format!("{}_metadata.json", video.display()))
}

fn read_metadata(path: &Path) -> Result<JobMetadata> {
    let content = fs::read_to_string(path)?;
    serde_json::from_str(&content).with_context(|| format!("Invalid metadata: {}", path.display()))
}

fn load_cli_metadata(cli: &Cli, input: &Path) -> Result<JobMetadata> {
    let mut metadata = match &cli.meta {
        Some(path) => read_metadata(path)?,
        None => read_metadata(&metadata_sidecar(input)).unwrap_or_default(),
    };
    if let Some(script) = &cli.script {
        metadata.script = Some(fs::read_to_string(script)?);
    }
    let mut options = metadata.options();
    let flags = &cli.options;
    options.analyzer = flags.analyzer.clone().or(options.analyzer);
    options.format = flags.format.clone().or(options.format);
    options.style = flags.style.clone().or(options.style);
    options.generate_bgm = flags.generate_bgm.or(options.generate_bgm);
    options.auto_sound_effects = flags.auto_sound_effects.or(options.auto_sound_effects);
    options.subtitles = flags.subtitles.or(options.subtitles);
    options.jump_cut = flags.jump_cut.or(options.jump_cut);
    metadata.options = Some(options);
    Ok(metadata)
}

fn set_status(job_id: &str, status: &str, message: &str, progress: i32) -> Result<()> {
    update_job_status(job_id, status, message, progress, JobUrls::default())
}

/// Builds the edit plan for a video and writes it to `output_path`.
fn analyze_video(
    input_path: &Path,
    output_path: &Path,
    metadata: &JobMetadata,
) -> Result<EditPlan> {
    let input_path = input_path
        .canonicalize()
        .with_context(|| format!("Video not found: {}", input_path.display()))?;
    let file_name = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Input video must have a valid UTF-8 filename")?
        .to_string();
    let job_id = file_stem(&input_path);

    fs::create_dir_all(data_path("raw"))?;
    let raw_path = data_path("raw").join(&file_name);
    if raw_path.canonicalize().ok().as_ref() != Some(&input_path) {
        fs::copy(&input_path, &raw_path)
            .with_context(|| format!("Failed to copy video to {}", raw_path.display()))?;
        let sidecar = input_path.with_extension("srt");
        if sidecar.exists() {
            let _ = fs::copy(&sidecar, raw_path.with_extension("srt"));
        }
    }

    set_status(&job_id, "analyzing", "Reading the video.", 15)?;
    let info = media::probe(&raw_path)?;
    let options = metadata.options();
    let work_dir = data_path("temp").join(format!("{}_analyze", job_id));

    // Transcribe even with subtitles off: word timings also drive the cuts.
    set_status(&job_id, "analyzing", "Transcribing speech.", 25)?;
    let transcript = subtitles::transcribe(&raw_path, &work_dir, info.has_audio);

    let style_name = options
        .style
        .clone()
        .or_else(|| env::var("NUE_STYLE").ok())
        .unwrap_or_else(|| "shorts".to_string());
    let analyzer = options
        .analyzer
        .clone()
        .or_else(|| env::var("NUE_ANALYZER").ok())
        .unwrap_or_else(|| "auto".to_string());
    let use_gemini = match analyzer.as_str() {
        "gemini" => true,
        "auto" => gemini::api_key().is_some(),
        _ => false,
    };

    let mut plan = None;
    if use_gemini {
        set_status(&job_id, "analyzing", "Gemini is planning the edit.", 40)?;
        match gemini::generate_edit_plan(
            &raw_path,
            &file_name,
            &job_id,
            &transcript.lines,
            metadata.script.as_deref(),
        ) {
            Ok(gemini_plan) if gemini_plan.cut_ranges().is_empty() => log_json(
                "WARN",
                "Gemini returned no usable cuts; using the local analyzer",
                Some("analyze_fallback"),
                None,
            ),
            Ok(mut gemini_plan) => {
                if gemini_plan.subtitles.is_empty() {
                    gemini_plan.subtitles = transcript.lines.clone();
                }
                plan = Some(gemini_plan);
            }
            Err(error) => log_json(
                "WARN",
                &format!(
                    "Gemini analyze failed; using the local analyzer: {:#}",
                    error
                ),
                Some("analyze_fallback"),
                None,
            ),
        }
    }
    let mut plan = match plan {
        Some(plan) => plan,
        None => {
            set_status(
                &job_id,
                "analyzing",
                "Finding pauses, highlights and scene changes.",
                40,
            )?;
            let signals = local::collect_signals(&raw_path, &info);
            local::build_plan(
                &file_name,
                &job_id,
                &info,
                &signals,
                transcript,
                options.jump_cut != Some(false),
                &style::load(Some(&style_name)),
            )
        }
    };

    let analyzed_telops = plan.telops.len();
    meta::apply_metadata(&mut plan, metadata, info.duration);
    // The creator's own telop (script or manual caption) beats a transcript
    // fragment as the opening title and thumbnail text.
    if plan.analyzer.as_deref() == Some("local") {
        if let Some(own) = plan.telops.get(analyzed_telops).map(|t| t.text.clone()) {
            if let Some(thumb) = plan.thumbnail.as_mut() {
                thumb.text = own.clone();
            }
            let is_hook = |t: &plan::TimedText| {
                t.style.as_ref().and_then(|s| s.size.as_deref()) == Some("large")
            };
            if let Some(hook) = plan.telops.iter_mut().find(|t| is_hook(t)) {
                hook.text = own;
            }
        }
    }
    let _ = fs::remove_dir_all(&work_dir);

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, serde_json::to_string_pretty(&plan)?)?;
    set_status(&job_id, "analyzed", "Edit plan is ready.", 60)?;
    log_json(
        "INFO",
        "Edit plan saved",
        Some("analysis_saved"),
        output_path.to_str(),
    );
    Ok(plan)
}

fn is_video_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let video_ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e.to_lowercase().as_str(),
                "mp4" | "mov" | "m4v" | "avi" | "mkv" | "webm"
            )
        })
        .unwrap_or(false);
    video_ext && !name.starts_with('.') && !name.ends_with(".part")
}

fn run_raw_job(input_path: &Path) -> Result<()> {
    if !input_path.exists() {
        return Ok(());
    }
    let job_id = file_stem(input_path);
    if let Some(status) = read_job_status(&job_id) {
        if matches!(
            status.as_str(),
            "analyzing" | "analyzed" | "rendering" | "completed" | "failed"
        ) {
            return Ok(());
        }
    }
    log_json(
        "INFO",
        "Processing upload",
        Some("raw_job_start"),
        input_path.to_str(),
    );
    let metadata = read_metadata(&metadata_sidecar(input_path)).unwrap_or_default();
    let plan_path = data_path("json").join(format!("{}.json", job_id));
    let result = analyze_video(input_path, &plan_path, &metadata)
        .and_then(|plan| render::render_plan(&plan));
    if let Err(error) = result {
        log_json(
            "ERROR",
            &format!("Job failed: {:#}", error),
            Some("raw_job_failed"),
            input_path.to_str(),
        );
        let _ = set_status(&job_id, "failed", &format!("{:#}", error), 100);
    }
    Ok(())
}

fn ensure_dirs() -> Result<()> {
    for dir in ["raw", "json", "edits", "output", "jobs", "temp"] {
        fs::create_dir_all(data_path(dir))?;
    }
    Ok(())
}

fn watch_dirs(dirs: &[PathBuf], mut handle: impl FnMut(&Path)) -> Result<()> {
    let (tx, rx) = channel();
    let config = Config::default().with_poll_interval(Duration::from_secs(2));
    let mut watcher: RecommendedWatcher = Watcher::new(tx, config)?;
    for dir in dirs {
        watcher.watch(dir, RecursiveMode::NonRecursive)?;
        log_json(
            "INFO",
            "Watching directory",
            Some("watch_start"),
            dir.to_str(),
        );
    }

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
                        handle(&path);
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
    ensure_dirs()?;
    log_json("INFO", "Nue worker started", Some("worker_startup"), None);
    let raw_dir = data_path("raw");
    let mut pending: Vec<PathBuf> = fs::read_dir(&raw_dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| is_video_file(path))
        .collect();
    pending.sort();
    for path in pending {
        run_raw_job(&path)?;
    }
    let edits_dir = data_path("edits");
    let mut edits: Vec<PathBuf> = fs::read_dir(&edits_dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| is_edit_request(path))
        .collect();
    edits.sort();
    for path in edits {
        run_edit_request(&path);
    }
    watch_dirs(&[raw_dir, edits_dir], |path| {
        if is_video_file(path) {
            std::thread::sleep(Duration::from_secs(1));
            let _ = run_raw_job(path);
        } else if is_edit_request(path) {
            run_edit_request(path);
        }
    })
}

/// `data/edits/<job_id>.json`: a hand-edited plan from the web UI to re-render.
fn is_edit_request(path: &Path) -> bool {
    path.parent() == Some(data_path("edits").as_path())
        && path.extension().is_some_and(|ext| ext == "json")
        && !path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(".")
            .starts_with('.')
}

fn run_edit_request(path: &Path) {
    let Ok(content) = fs::read_to_string(path) else {
        return; // Already handled (the file is removed once picked up).
    };
    let _ = fs::remove_file(path);
    let job_id = file_stem(path);
    let result = serde_json::from_str::<EditPlan>(&content)
        .context("Invalid edited plan")
        .and_then(|mut plan| {
            plan.job_id = Some(job_id.clone());
            let plan_path = data_path("json").join(format!("{}.json", job_id));
            fs::write(&plan_path, serde_json::to_string_pretty(&plan)?)?;
            log_json(
                "INFO",
                "Re-rendering edited plan",
                Some("edit_render"),
                plan_path.to_str(),
            );
            render::render_plan(&plan)
        });
    if let Err(error) = result {
        log_json(
            "ERROR",
            &format!("Edit failed: {:#}", error),
            Some("edit_failed"),
            path.to_str(),
        );
        let _ = set_status(&job_id, "failed", &format!("{:#}", error), 100);
    }
}

fn watch_json_jobs() -> Result<()> {
    ensure_dirs()?;
    let mut rendered = std::collections::HashSet::new();
    watch_dirs(&[data_path("json")], |path| {
        if path.extension().is_some_and(|ext| ext == "json") && rendered.insert(path.to_path_buf())
        {
            std::thread::sleep(Duration::from_secs(1));
            let plan = fs::read_to_string(path)
                .map_err(anyhow::Error::from)
                .and_then(|content| serde_json::from_str::<EditPlan>(&content).map_err(Into::into));
            match plan {
                Ok(plan) => {
                    let _ = render::render_plan(&plan);
                }
                Err(e) => log_json(
                    "ERROR",
                    &format!("Invalid plan: {:#}", e),
                    Some("parse_error"),
                    path.to_str(),
                ),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_flags() {
        let args: Vec<String> = ["in.mp4", "--format", "square", "--no-bgm", "-o", "out.mp4"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let cli = parse_cli(&args).unwrap();
        assert_eq!(cli.positional, vec!["in.mp4"]);
        assert_eq!(cli.options.format.as_deref(), Some("square"));
        assert_eq!(cli.options.generate_bgm, Some(false));
        assert_eq!(cli.out, Some(PathBuf::from("out.mp4")));
        assert!(parse_cli(&["--bogus".to_string()]).is_err());
    }

    #[test]
    fn filters_video_files() {
        assert!(is_video_file(Path::new("/x/a.MP4")));
        assert!(!is_video_file(Path::new("/x/a.mp4.part")));
        assert!(!is_video_file(Path::new("/x/a.mp4_metadata.json")));
    }
}
