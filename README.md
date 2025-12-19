# Nue (鵺) - AI Video Alchemy Platform

Nue is an advanced automated video processing platform that turns raw footage into "YouTube-ready" content using AI. It combines the speed of Go, the intelligence of Python (Gemini), and the raw power of Rust (FFmpeg).

## Features

- **🧠 Brain (Python + Gemini 2.5)**: 
    - Analyzes video content to understand context, mood, and key moments.
    - Suggests cuts, filters, and captions.
    - **Trend Analysis**: Fetches trending styles (tempo, filters) from the web to keep content fresh.
    - **Smart Crop**: Detects subject location for automatic Vertical Video generation.
    - **Thumbnail AI**: Identifies the most "clickbaity" frame and catchphrase.

- **💪 Muscle (Rust + FFmpeg)**:
    - **High-Performant Editing**: Trims, stitches, and applies effects with single-pass FFmpeg filtering.
    - **Visual Polish**: Applies digital zooms, pans, and color grading.
    - **Smart Cropping**: Generates 9:16 Vertical Videos automatically.
    - **Audio Engineering**: 
        - Auto-ducking for BGM.
        - Sound Effect (SE) mixing at precise "tsukkomi" moments.

- **🚪 Gateway (Go)**:
    - Simple API entry point for uploading footage.

## Architecture

- **Microservices**: Docker Compose based architecture.
- **GitOps Ready**: GitHub Actions for CI/CD included.

## Getting Started

### Prerequisites
- Docker & Docker Compose
- Google Gemini API Key

### Setup
1. Clone the repository.
   ```bash
   git clone https://github.com/yourname/nue.git
   cd nue
   ```
2. Create `.env` file from example (or just set `GEMINI_API_KEY`).
   ```bash
   echo "GEMINI_API_KEY=your_key_here" > .env
   ```
3. Start the services.
   ```bash
   docker compose up -d
   ```

### Usage
1. Place raw video files in `./data/raw`.
2. **Brain** will automatically detect, analyze, and generate a JSON plan.
3. **Muscle** will execute the plan and output the final video to `./data/output`.

## Roadmap
- [x] Basic Cut & Stitch
- [x] Trend Analysis (Style Transfer)
- [x] Audio Eng (SE/Ducking)
- [x] Visual Polish (Zoom)
- [x] Vertical Video Support
- [x] Thumbnail Generation
- [ ] Cloud Deployment (AWS)

## License
MIT
