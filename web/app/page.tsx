"use client";

import { useState, useRef } from "react";
import PlanEditor from "./PlanEditor";
import { Upload, CheckCircle, Loader2, Play, ChevronDown, ChevronUp, Plus, X, Settings, Download, Pencil } from "lucide-react";

const API_BASE_URL = process.env.NEXT_PUBLIC_API_URL ?? "";
const uploadUrl = API_BASE_URL ? `${API_BASE_URL}/upload` : "/api/upload";
const jobUrl = (jobId: string) => API_BASE_URL ? `${API_BASE_URL}/jobs/${jobId}` : `/api/jobs/${jobId}`;
const resolveOutputUrl = (url: string) => {
  if (url.startsWith("http://") || url.startsWith("https://")) {
    return url;
  }
  if (API_BASE_URL) {
    return `${API_BASE_URL}${url}`;
  }
  if (url.startsWith("/output/")) {
    return `/api${url}`;
  }
  return url;
};

interface ManualCut {
  start: string;
  end: string;
  action: "remove" | "keep";
}

interface ManualCaption {
  timestamp: string;
  text: string;
  style: string;
}

interface ManualEffect {
  timestamp: string;
  type: string;
}

interface JobStatus {
  job_id: string;
  status: "uploaded" | "analyzing" | "analyzed" | "rendering" | "completed" | "failed";
  message?: string;
  progress?: number;
  output_url?: string;
  thumbnail_url?: string;
  subtitle_url?: string;
  plan_url?: string;
  updated_at?: string | number;
}

interface OutputLinks {
  video: string;
  thumbnail?: string;
  subtitles?: string;
  plan?: string;
}

type Analyzer = "auto" | "local" | "gemini";
type VideoFormat = "vertical" | "horizontal" | "square";

export default function Home() {
  const [dragActive, setDragActive] = useState(false);
  const [file, setFile] = useState<File | null>(null);
  const [status, setStatus] = useState<"idle" | "uploading" | "processing" | "done" | "failed">("idle");
  const [progress, setProgress] = useState(0);
  const [jobMessage, setJobMessage] = useState("");
  const [output, setOutput] = useState<OutputLinks | null>(null);
  const [jobId, setJobId] = useState("");
  const [editing, setEditing] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  // Advanced options state
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [script, setScript] = useState("");
  const [autoSoundEffects, setAutoSoundEffects] = useState(true);
  const [generateBGM, setGenerateBGM] = useState(true);
  const [subtitles, setSubtitles] = useState(true);
  const [jumpCut, setJumpCut] = useState(true);
  const [analyzer, setAnalyzer] = useState<Analyzer>("auto");
  const [videoFormat, setVideoFormat] = useState<VideoFormat>("vertical");

  // Manual instructions state
  const [manualCuts, setManualCuts] = useState<ManualCut[]>([]);
  const [manualCaptions, setManualCaptions] = useState<ManualCaption[]>([]);
  const [manualEffects, setManualEffects] = useState<ManualEffect[]>([]);

  const handleDrag = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (e.type === "dragenter" || e.type === "dragover") {
      setDragActive(true);
    } else if (e.type === "dragleave") {
      setDragActive(false);
    }
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setDragActive(false);
    if (e.dataTransfer.files && e.dataTransfer.files[0]) {
      handleFile(e.dataTransfer.files[0]);
    }
  };

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    e.preventDefault();
    if (e.target.files && e.target.files[0]) {
      handleFile(e.target.files[0]);
    }
  };

  const handleFile = (file: File) => {
    setFile(file);
    uploadFile(file);
  };

  const uploadFile = async (file: File) => {
    setStatus("uploading");
    setProgress(0);
    setJobMessage("");
    setOutput(null);
    const formData = new FormData();
    formData.append("file", file);

    // Add metadata
    const metadata = {
      script: script || undefined,
      manual_instructions: {
        cuts: manualCuts.length > 0 ? manualCuts : undefined,
        captions: manualCaptions.length > 0 ? manualCaptions : undefined,
        effects: manualEffects.length > 0 ? manualEffects : undefined,
      },
      options: {
        auto_sound_effects: autoSoundEffects,
        generate_bgm: generateBGM,
        subtitles,
        jump_cut: jumpCut,
        analyzer,
        format: videoFormat,
      },
    };

    formData.append("metadata", JSON.stringify(metadata));

    try {
      const xhr = new XMLHttpRequest();
      xhr.open("POST", uploadUrl);

      xhr.upload.onprogress = (event) => {
        if (event.lengthComputable) {
          const percent = Math.round((event.loaded / event.total) * 100);
          setProgress(percent);
        }
      };

      xhr.onload = () => {
        if (xhr.status === 202) {
          setStatus("processing");
          const response = JSON.parse(xhr.responseText) as { job_id?: string };
          if (response.job_id) {
            setJobId(response.job_id);
            pollJob(response.job_id);
          }
        } else {
          alert("Upload failed.");
          setStatus("idle");
        }
      };

      xhr.onerror = () => {
        alert("Upload error.");
        setStatus("idle");
      };

      xhr.send(formData);
    } catch (error) {
      console.error(error);
      setStatus("idle");
    }
  };

  const pollJob = async (jobId: string) => {
    try {
      const response = await fetch(jobUrl(jobId));
      if (!response.ok) {
        throw new Error("Job status unavailable.");
      }
      const job = (await response.json()) as JobStatus;
      setJobMessage(job.message ?? "");
      setProgress(job.progress ?? 0);

      if (job.status === "completed" && job.output_url) {
        // Re-renders overwrite the same file names, so bust the browser cache.
        const version = `?v=${job.updated_at ?? Date.now()}`;
        const link = (url?: string) => (url ? `${resolveOutputUrl(url)}${version}` : undefined);
        setOutput({
          video: link(job.output_url)!,
          thumbnail: link(job.thumbnail_url),
          subtitles: link(job.subtitle_url),
          plan: link(job.plan_url),
        });
        setStatus("done");
        return;
      }

      if (job.status === "failed") {
        setStatus("failed");
        return;
      }

      window.setTimeout(() => pollJob(jobId), 2000);
    } catch (error) {
      console.error(error);
      setJobMessage("Waiting for status...");
      window.setTimeout(() => pollJob(jobId), 3000);
    }
  };

  const addCut = () => {
    setManualCuts([...manualCuts, { start: "00:00:00", end: "00:00:05", action: "remove" }]);
  };

  const addCaption = () => {
    setManualCaptions([...manualCaptions, { timestamp: "00:00:00", text: "", style: "yellow" }]);
  };

  const addEffect = () => {
    setManualEffects([...manualEffects, { timestamp: "00:00:00", type: "zoom_in" }]);
  };

  return (
    <div className="min-h-screen bg-black text-white selection:bg-purple-500/30 flex flex-col items-center justify-center p-4 relative overflow-hidden">
      {/* Dynamic Background */}
      <div className="absolute top-0 left-0 w-full h-full z-0 opacity-20 pointer-events-none">
        <div className="absolute top-[-20%] left-[-10%] w-[60%] h-[60%] bg-purple-600 rounded-full blur-[150px] animate-pulse" />
        <div className="absolute bottom-[-20%] right-[-10%] w-[60%] h-[60%] bg-blue-600 rounded-full blur-[150px] animate-pulse" style={{ animationDelay: "2s" }} />
      </div>

      <div className="z-10 w-full max-w-3xl">
        <header className="mb-12 text-center space-y-4">
          <h1 className="text-6xl font-bold tracking-tighter bg-clip-text text-transparent bg-gradient-to-r from-purple-400 to-blue-400">
            Nue.
          </h1>
          <p className="text-gray-400 text-lg">AI Video Alchemy Platform</p>
        </header>

        {/* Upload Area */}
        <main
          className={`
            relative group border-2 border-dashed rounded-3xl p-12 transition-all duration-300 ease-out
            flex flex-col items-center justify-center gap-6 min-h-[400px] backdrop-blur-xl bg-white/5
            ${dragActive ? "border-purple-500 bg-purple-500/10 scale-105" : "border-white/10 hover:border-white/20"}
          `}
          onDragEnter={handleDrag}
          onDragLeave={handleDrag}
          onDragOver={handleDrag}
          onDrop={handleDrop}
          onClick={(e) => {
            // Only trigger file input if clicking on the upload area itself, not on any child elements
            if (e.target === e.currentTarget && status === "idle") {
              inputRef.current?.click();
            }
          }}
        >
          <input
            ref={inputRef}
            type="file"
            className="hidden"
            accept="video/*"
            onChange={handleChange}
            disabled={status !== "idle"}
          />

          {status === "idle" && (
            <>
              <div className="p-6 rounded-full bg-white/5 group-hover:bg-white/10 transition-colors">
                <Upload className="w-10 h-10 text-gray-300" />
              </div>
              <div className="text-center space-y-2">
                <p className="text-xl font-medium text-gray-200">Drop video here</p>
                <p className="text-sm text-gray-500">or click to browse</p>
              </div>
            </>
          )}

          {status === "uploading" && (
            <div className="w-full max-w-xs space-y-4 text-center">
              <div className="text-5xl font-mono font-bold text-gray-200">{progress}%</div>
              <p className="text-gray-400 text-sm">Uploading...</p>
              {file && <p className="text-gray-500 text-xs truncate">{file.name}</p>}
              <div className="w-full h-1 bg-white/10 rounded-full overflow-hidden">
                <div
                  className="h-full bg-gradient-to-r from-purple-500 to-blue-500 transition-all duration-300"
                  style={{ width: `${progress}%` }}
                />
              </div>
            </div>
          )}

          {status === "processing" && (
            <div className="text-center space-y-4 animate-pulse">
              <Loader2 className="w-12 h-12 text-purple-400 animate-spin mx-auto" />
              <div className="space-y-1">
                <p className="text-xl font-medium text-gray-200">Processing</p>
                <p className="text-sm text-gray-500">{jobMessage || "Analyzing your video..."}</p>
                {file && <p className="text-gray-500 text-xs truncate">{file.name}</p>}
                <p className="text-sm text-gray-500">{progress}%</p>
              </div>
            </div>
          )}

          {status === "done" && (
            <div className="text-center space-y-6">
              <div className="w-20 h-20 bg-green-500/20 rounded-full flex items-center justify-center mx-auto mb-2">
                <CheckCircle className="w-10 h-10 text-green-400" />
              </div>
              <div className="space-y-2">
                <p className="text-2xl font-bold text-white">Ready!</p>
                <p className="text-gray-400">Your video has been transmuted.</p>
              </div>

              {output && (
                <video
                  src={output.video}
                  poster={output.thumbnail}
                  controls
                  playsInline
                  className="w-full max-h-[520px] rounded-lg bg-black"
                />
              )}

              {output && (
                <div className="flex flex-wrap gap-2 justify-center text-sm">
                  {[
                    { href: output.thumbnail, label: "Thumbnail" },
                    { href: output.subtitles, label: "Subtitles (.srt)" },
                    { href: output.plan, label: "Edit plan (.json)" },
                  ]
                    .filter((link): link is { href: string; label: string } => Boolean(link.href))
                    .map((link) => (
                      <a
                        key={link.label}
                        href={link.href}
                        download
                        onClick={(e) => e.stopPropagation()}
                        className="px-4 py-1.5 rounded-full bg-white/10 hover:bg-white/20 transition flex items-center gap-1"
                      >
                        <Download className="w-3.5 h-3.5" /> {link.label}
                      </a>
                    ))}
                </div>
              )}

              <div className="flex gap-4 justify-center">
                {output && (
                  <a
                    href={output.video}
                    target="_blank"
                    rel="noreferrer"
                    onClick={(e) => e.stopPropagation()}
                    className="px-6 py-2 rounded-full bg-white text-black font-semibold hover:bg-gray-200 transition"
                  >
                    <Play className="w-4 h-4 inline mr-2" fill="currentColor" /> Watch
                  </a>
                )}
                {output?.plan && (
                  <button
                    onClick={(e) => { e.stopPropagation(); setEditing(!editing); }}
                    className="px-6 py-2 rounded-full bg-white/10 text-white font-semibold hover:bg-white/20 transition flex items-center gap-2"
                  >
                    <Pencil className="w-4 h-4" /> {editing ? "Close editor" : "Edit"}
                  </button>
                )}
                <button
                  onClick={(e) => { e.stopPropagation(); setStatus("idle"); setFile(null); setOutput(null); setProgress(0); setEditing(false); }}
                  className="px-6 py-2 rounded-full bg-white/10 text-white font-semibold hover:bg-white/20 transition"
                >
                  New Upload
                </button>
              </div>

              {editing && output?.plan && jobId && (
                <div className="w-full pt-6 border-t border-white/10">
                  <PlanEditor
                    jobId={jobId}
                    planUrl={output.plan}
                    onQueued={() => {
                      setEditing(false);
                      setStatus("processing");
                      setProgress(65);
                      setJobMessage("Re-render queued.");
                      window.setTimeout(() => pollJob(jobId), 1500);
                    }}
                  />
                </div>
              )}
            </div>
          )}

          {status === "failed" && (
            <div className="text-center space-y-6">
              <div className="space-y-2">
                <p className="text-2xl font-bold text-white">Render failed</p>
                <p className="text-gray-400">{jobMessage || "Check the service logs for details."}</p>
              </div>
              <button
                onClick={(e) => { e.stopPropagation(); setStatus("idle"); setFile(null); setOutput(null); setProgress(0); }}
                className="px-6 py-2 rounded-full bg-white/10 text-white font-semibold hover:bg-white/20 transition"
              >
                New Upload
              </button>
            </div>
          )}
        </main>

        {/* Advanced Options */}
        {status === "idle" && (
          <div className="mt-6">
            <button
              onClick={() => setShowAdvanced(!showAdvanced)}
              className="w-full flex items-center justify-between px-6 py-4 rounded-2xl bg-white/5 hover:bg-white/10 transition-colors border border-white/10"
            >
              <div className="flex items-center gap-3">
                <Settings className="w-5 h-5 text-purple-400" />
                <span className="font-medium">Advanced Options</span>
              </div>
              {showAdvanced ? <ChevronUp className="w-5 h-5" /> : <ChevronDown className="w-5 h-5" />}
            </button>

            {showAdvanced && (
              <div className="mt-4 p-6 rounded-2xl bg-white/5 border border-white/10 space-y-6">
                {/* Script Input */}
                <div>
                  <label className="block text-sm font-medium text-gray-300 mb-2">
                    Script / Transcript (Optional)
                  </label>
                  <textarea
                    value={script}
                    onChange={(e) => setScript(e.target.value)}
                    placeholder={"00:00:02 - 今日のテーマ\n00:00:15 - ここがポイント！\n00:00:30 - まとめ"}
                    className="w-full h-32 px-4 py-3 bg-black/50 border border-white/10 rounded-xl text-white placeholder-gray-500 focus:outline-none focus:border-purple-500 transition-colors resize-none"
                  />
                </div>

                {/* Feature Toggles */}
                <div className="space-y-3">
                  <label className="flex items-center gap-3 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={autoSoundEffects}
                      onChange={(e) => setAutoSoundEffects(e.target.checked)}
                      className="w-5 h-5 rounded bg-black/50 border-white/10 text-purple-500 focus:ring-purple-500"
                    />
                    <span className="text-sm text-gray-300">Auto Sound Effects (AI-synced)</span>
                  </label>
                  <label className="flex items-center gap-3 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={generateBGM}
                      onChange={(e) => setGenerateBGM(e.target.checked)}
                      className="w-5 h-5 rounded bg-black/50 border-white/10 text-purple-500 focus:ring-purple-500"
                    />
                    <span className="text-sm text-gray-300">Add Background Music</span>
                  </label>
                  <label className="flex items-center gap-3 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={subtitles}
                      onChange={(e) => setSubtitles(e.target.checked)}
                      className="w-5 h-5 rounded bg-black/50 border-white/10 text-purple-500 focus:ring-purple-500"
                    />
                    <span className="text-sm text-gray-300">Subtitles from speech (on-device whisper)</span>
                  </label>
                  <label className="flex items-center gap-3 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={jumpCut}
                      onChange={(e) => setJumpCut(e.target.checked)}
                      className="w-5 h-5 rounded bg-black/50 border-white/10 text-purple-500 focus:ring-purple-500"
                    />
                    <span className="text-sm text-gray-300">Jump cut (remove silent pauses)</span>
                  </label>
                </div>

                <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <label className="block">
                    <span className="block text-sm font-medium text-gray-300 mb-2">Format</span>
                    <select
                      value={videoFormat}
                      onChange={(e) => setVideoFormat(e.target.value as VideoFormat)}
                      className="w-full px-3 py-2 bg-black/50 border border-white/10 rounded-lg text-sm"
                    >
                      <option value="vertical">Vertical 9:16 (Shorts / Reels / TikTok)</option>
                      <option value="horizontal">Horizontal 16:9 (YouTube)</option>
                      <option value="square">Square 1:1</option>
                    </select>
                  </label>
                  <label className="block">
                    <span className="block text-sm font-medium text-gray-300 mb-2">AI</span>
                    <select
                      value={analyzer}
                      onChange={(e) => setAnalyzer(e.target.value as Analyzer)}
                      className="w-full px-3 py-2 bg-black/50 border border-white/10 rounded-lg text-sm"
                    >
                      <option value="auto">Auto (Gemini if an API key is set)</option>
                      <option value="local">Local only (no API, free)</option>
                      <option value="gemini">Gemini</option>
                    </select>
                  </label>
                </div>

                {/* Manual Cuts */}
                <div>
                  <div className="flex items-center justify-between mb-3">
                    <label className="text-sm font-medium text-gray-300">Manual Cuts</label>
                    <button onClick={addCut} className="px-3 py-1 bg-purple-500/20 hover:bg-purple-500/30 rounded-lg text-xs flex items-center gap-1">
                      <Plus className="w-3 h-3" /> Add
                    </button>
                  </div>
                  {manualCuts.map((cut, idx) => (
                    <div key={idx} className="flex gap-2 mb-2">
                      <input
                        type="text"
                        value={cut.start}
                        onChange={(e) => {
                          const updated = [...manualCuts];
                          updated[idx].start = e.target.value;
                          setManualCuts(updated);
                        }}
                        placeholder="00:00:00"
                        className="flex-1 px-3 py-2 bg-black/50 border border-white/10 rounded-lg text-sm"
                      />
                      <input
                        type="text"
                        value={cut.end}
                        onChange={(e) => {
                          const updated = [...manualCuts];
                          updated[idx].end = e.target.value;
                          setManualCuts(updated);
                        }}
                        placeholder="00:00:05"
                        className="flex-1 px-3 py-2 bg-black/50 border border-white/10 rounded-lg text-sm"
                      />
                      <select
                        value={cut.action}
                        onChange={(e) => {
                          const updated = [...manualCuts];
                          updated[idx].action = e.target.value as "remove" | "keep";
                          setManualCuts(updated);
                        }}
                        className="px-3 py-2 bg-black/50 border border-white/10 rounded-lg text-sm"
                      >
                        <option value="remove">Remove</option>
                        <option value="keep">Keep</option>
                      </select>
                      <button
                        onClick={() => setManualCuts(manualCuts.filter((_, i) => i !== idx))}
                        className="px-2 py-2 bg-red-500/20 hover:bg-red-500/30 rounded-lg"
                      >
                        <X className="w-4 h-4" />
                      </button>
                    </div>
                  ))}
                </div>

                {/* Manual Captions */}
                <div>
                  <div className="flex items-center justify-between mb-3">
                    <label className="text-sm font-medium text-gray-300">Manual Captions</label>
                    <button onClick={addCaption} className="px-3 py-1 bg-purple-500/20 hover:bg-purple-500/30 rounded-lg text-xs flex items-center gap-1">
                      <Plus className="w-3 h-3" /> Add
                    </button>
                  </div>
                  {manualCaptions.map((caption, idx) => (
                    <div key={idx} className="flex gap-2 mb-2">
                      <input
                        type="text"
                        value={caption.timestamp}
                        onChange={(e) => {
                          const updated = [...manualCaptions];
                          updated[idx].timestamp = e.target.value;
                          setManualCaptions(updated);
                        }}
                        placeholder="00:00:00"
                        className="w-24 px-3 py-2 bg-black/50 border border-white/10 rounded-lg text-sm"
                      />
                      <input
                        type="text"
                        value={caption.text}
                        onChange={(e) => {
                          const updated = [...manualCaptions];
                          updated[idx].text = e.target.value;
                          setManualCaptions(updated);
                        }}
                        placeholder="Caption text"
                        className="flex-1 px-3 py-2 bg-black/50 border border-white/10 rounded-lg text-sm"
                      />
                      <select
                        value={caption.style}
                        onChange={(e) => {
                          const updated = [...manualCaptions];
                          updated[idx].style = e.target.value;
                          setManualCaptions(updated);
                        }}
                        className="px-3 py-2 bg-black/50 border border-white/10 rounded-lg text-sm"
                      >
                        <option value="yellow">Yellow</option>
                        <option value="white">White</option>
                        <option value="cyan">Cyan</option>
                      </select>
                      <button
                        onClick={() => setManualCaptions(manualCaptions.filter((_, i) => i !== idx))}
                        className="px-2 py-2 bg-red-500/20 hover:bg-red-500/30 rounded-lg"
                      >
                        <X className="w-4 h-4" />
                      </button>
                    </div>
                  ))}
                </div>

                {/* Manual Effects */}
                <div>
                  <div className="flex items-center justify-between mb-3">
                    <label className="text-sm font-medium text-gray-300">Manual Effects</label>
                    <button onClick={addEffect} className="px-3 py-1 bg-purple-500/20 hover:bg-purple-500/30 rounded-lg text-xs flex items-center gap-1">
                      <Plus className="w-3 h-3" /> Add
                    </button>
                  </div>
                  {manualEffects.map((effect, idx) => (
                    <div key={idx} className="flex gap-2 mb-2">
                      <input
                        type="text"
                        value={effect.timestamp}
                        onChange={(e) => {
                          const updated = [...manualEffects];
                          updated[idx].timestamp = e.target.value;
                          setManualEffects(updated);
                        }}
                        placeholder="00:00:00"
                        className="w-24 px-3 py-2 bg-black/50 border border-white/10 rounded-lg text-sm"
                      />
                      <select
                        value={effect.type}
                        onChange={(e) => {
                          const updated = [...manualEffects];
                          updated[idx].type = e.target.value;
                          setManualEffects(updated);
                        }}
                        className="flex-1 px-3 py-2 bg-black/50 border border-white/10 rounded-lg text-sm"
                      >
                        <option value="zoom_in">Zoom In</option>
                        <option value="zoom_out">Zoom Out</option>
                        <option value="pan_left">Pan Left</option>
                        <option value="pan_right">Pan Right</option>
                      </select>
                      <button
                        onClick={() => setManualEffects(manualEffects.filter((_, i) => i !== idx))}
                        className="px-2 py-2 bg-red-500/20 hover:bg-red-500/30 rounded-lg"
                      >
                        <X className="w-4 h-4" />
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        )}

        <footer className="mt-12 flex justify-between text-xs text-gray-600 uppercase tracking-widest font-mono">
          <span>Nue v2.0.0</span>
          <span>Status: Online</span>
        </footer>
      </div>
    </div>
  );
}
