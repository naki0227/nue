"use client";

import { useState, useRef } from "react";
import { Upload, FileVideo, CheckCircle, Loader2, Play } from "lucide-react";

export default function Home() {
  const [dragActive, setDragActive] = useState(false);
  const [file, setFile] = useState<File | null>(null);
  const [status, setStatus] = useState<"idle" | "uploading" | "processing" | "done">("idle");
  const [progress, setProgress] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

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
    const formData = new FormData();
    formData.append("file", file);

    try {
      // Step 1: Upload
      const xhr = new XMLHttpRequest();
      xhr.open("POST", "http://localhost:8080/upload"); // Direct to Gateway

      xhr.upload.onprogress = (event) => {
        if (event.lengthComputable) {
          const percent = Math.round((event.loaded / event.total) * 100);
          setProgress(percent);
        }
      };

      xhr.onload = () => {
        if (xhr.status === 202) {
          setStatus("processing");
          // Ideally poll for status, but for MVP we assume "processing" then "done" after a delay
          // In real app, we'd poll an endpoint like GET /status/:id
          setTimeout(() => setStatus("done"), 5000); // Fake processing delay for demo
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

  return (
    <div className="min-h-screen bg-black text-white selection:bg-purple-500/30 flex flex-col items-center justify-center p-4 relative overflow-hidden">
      {/* Dynamic Background */}
      <div className="absolute top-0 left-0 w-full h-full z-0 opacity-20 pointer-events-none">
        <div className="absolute top-[-20%] left-[-10%] w-[60%] h-[60%] bg-purple-600 rounded-full blur-[150px] animate-pulse" />
        <div className="absolute bottom-[-20%] right-[-10%] w-[60%] h-[60%] bg-blue-600 rounded-full blur-[150px] animate-pulse" style={{ animationDelay: "2s" }} />
      </div>

      <div className="z-10 w-full max-w-xl">
        <header className="mb-12 text-center space-y-4">
          <h1 className="text-6xl font-bold tracking-tighter bg-clip-text text-transparent bg-gradient-to-r from-purple-400 to-blue-400">
            Nue.
          </h1>
          <p className="text-gray-400 text-lg">AI Video Alchemy Platform</p>
        </header>

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
          onClick={() => status === "idle" && inputRef.current?.click()}
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
                <p className="text-sm text-gray-500">Brain is analyzing your video...</p>
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

              <div className="flex gap-4 justify-center">
                <button
                  onClick={(e) => { e.stopPropagation(); alert("Preview not implemented in MVP"); }}
                  className="px-6 py-2 rounded-full bg-white text-black font-semibold hover:bg-gray-200 transition"
                >
                  <Play className="w-4 h-4 inline mr-2" fill="currentColor" /> Watch
                </button>
                <button
                  onClick={(e) => { e.stopPropagation(); setStatus("idle"); setFile(null); }}
                  className="px-6 py-2 rounded-full bg-white/10 text-white font-semibold hover:bg-white/20 transition"
                >
                  New Upload
                </button>
              </div>
            </div>
          )}
        </main>

        <footer className="mt-12 flex justify-between text-xs text-gray-600 uppercase tracking-widest font-mono">
          <span>Nue v1.0.0</span>
          <span>Status: Online</span>
        </footer>
      </div>
    </div>
  );
}
