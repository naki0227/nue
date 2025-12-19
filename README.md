# Nue (鵺) - AI Video Alchemy Platform 🧪

![GitHub License](https://img.shields.io/github/license/naki0227/nue)
![Rust](https://img.shields.io/badge/Rust-Muscle-orange?logo=rust)
![Python](https://img.shields.io/badge/Python-Brain-blue?logo=python)
![Go](https://img.shields.io/badge/Go-Gateway-cyan?logo=go)
![Docker](https://img.shields.io/badge/Docker-Container-blue?logo=docker)
![Gemini](https://img.shields.io/badge/AI-Gemini_2.5-magenta?logo=google-gemini)

**Nue (鵺)** is an automated video processing platform that transmutes raw footage into "YouTube-ready" content using AI. It combines the speed of Go, the cognitive power of Google's Gemini 2.5, and the raw performance of Rust (FFmpeg) to solve the "editing bottleneck" for creators.

---

## 📖 The "Why"
Video editing is the single biggest friction point in content creation. 
- **The Problem**: 10 minutes of footage often requires 2 hours of cutting, captioning, and sound design.
- **The Solution**: Nue acts as an "AI Editor" that watches your footage, understands the context (funny, serious, shocking), and autonomously applies professional editing techniques—including BGM matching, digital zooms, and vertical cropping for Shorts.

## 🚀 Key Features

### 🧠 Trend-Aware Editing (Brain)
- **Style Cloning**: analyzing trending Vlogs on YouTube/TikTok to extract pacing, filter usage, and music choices.
- **Semantic Analysis**: Uses **Gemini 2.5 Flash** to understand "what is happening" rather than just visual changes.
- **Smart Thumbnails**: Automatically identifies the most "clickbaity" frame and imposes catchy titles.

### 💪 High-Performance Rendering (Muscle)
- **Rust + FFmpeg**: Built on a highly concurrent Rust architecture that constructs complex FFmpeg filter graphs.
- **Visual Polish**: Applies **Digital Zooms** (Ken Burns effect) and **Pans** to static shots to retain viewer retention.
- **Audio Engineering**:
    - **Auto-Ducking**: Automatically lowers BGM volume when speech is detected.
    - **Tsukkomi SE**: Inserts sound effects ("Vine Boom", "Laugh") at precise "punchline" moments.

### 📱 Multi-Format Native
- **Auto-Shorts**: Detects the subject's position (`focus_point`) and intelligently crops 16:9 footage into 9:16 Vertical Video for TikTok/Reels.

---

## 🛠 Architecture

Nue adopts a microservices architecture optimized for local execution via Docker Compose, scalable to Cloud (AWS/GCP).

```mermaid
graph TD
    User[User / Camera] -->|Upload .mp4| Gateway
    
    subgraph "Nue Platform (Docker)"
        Gateway[Gateway (Go)] -->|Save to Disk| Volume[(Shared Volume)]
        
        Brain[Brain (Python)] -->|Watch New Files| Volume
        Brain <-->|Video Analysis| Gemini[Google Gemini API]
        Brain -->|Instructions JSON| Volume
        
        Muscle[Muscle (Rust)] -->|Watch JSON| Volume
        Muscle -->|Render FFmpeg| Volume
        
        Trend[Trend Watcher] -->|Crawl Styles| YouTube[YouTube/TikTok]
        Trend -->|Update Style DB| Brain
    end
    
    Muscle -->|Output .mp4/.jpg| Output[Final Content]
```

## 💻 Tech Stack

| Service | Technology | Role |
|:---|:---|:---|
| **Gateway** | **Go (Gin)** | High-throughput upload handler using minimal RAM. |
| **Brain** | **Python 3.11** | AI logic hub. Handles Gemini SDK, Watchdog, and Trend Analysis. |
| **Muscle** | **Rust** | Heavy lifting. Generates complex FFmpeg filter chains (`xfade`, `drawtext`, `sidechaincompress`). |
| **Infra** | **Docker Compose** | Orchestrates services with shared volumes (`/data`). |

---

## ⚡ Quick Start

### Prerequisites
- Docker Desktop
- Google Gemini API Key

### Installation

1. **Clone the repository**
   ```bash
   git clone https://github.com/naki0227/nue.git
   cd nue
   ```

2. **Configure Environment**
   Create a `.env` file:
   ```bash
   GEMINI_API_KEY=your_actual_api_key_here
   GEMINI_MODEL=gemini-2.5-flash
   ```

3. **Launch Services**
   ```bash
   docker compose up -d --build
   ```

4. **Transmute Video**
   Drop a video file into `data/raw`, or use cURL:
   ```bash
   curl -F "file=@/path/to/my_vlog.mp4" http://localhost:8080/upload
   ```

5. **Get Result**
   Check `data/output/` for your edited video and thumbnail!

---

## 📈 Roadmap

- [x] **Phase 1-2**: MVP Architecture & Editing (Cuts, Transitions)
- [x] **Phase 3**: Trend Analysis Engine
- [x] **Phase 6**: Audio Engineering (Ducking, SE)
- [x] **Phase 7**: Visual Polish (Zoom/Pan)
- [x] **Phase 8**: Thumbnail Generation
- [x] **Phase 9**: CI/CD (GitHub Actions)
- [x] **Phase 10**: Vertical Video Support (Smart Crop)
- [ ] **Phase 11**: Cloud Deployment (AWS ECS/Lambda)

---

## 🤝 Contribution

Contributions are welcome! Please read `CONTRIBUTING.md` (coming soon) for details on our code of conduct, and the process for submitting pull requests.

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
