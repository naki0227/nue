# Nue (鵺)

**Nue** is an AI-powered automated video processing platform that analyzes video content and applies editing effects based on the analysis.

It is composed of three microservices working in a pipeline:
1.  **Gateway (Go)**: Entry point for video uploads.
2.  **Brain (Python)**: Analyzes video content using Google Gemini 2.5 Flash.
3.  **Muscle (Rust)**: Processes video using FFmpeg based on AI instructions.

## Architecture

```mermaid
graph LR
    User[User] -- Upload --> Gateway[Gateway (Go)]
    Gateway -- Save --> Raw[data/raw]
    Brain[Brain (Python)] -- Watch --> Raw
    Brain -- Upload & Analyze --> Gemini[Google Gemini 2.5 Flash]
    Gemini -- JSON Instructions --> Brain
    Brain -- Save --> JSON[data/json]
    Muscle[Muscle (Rust)] -- Watch --> JSON
    Muscle -- Read --> Raw
    Muscle -- Edit & Transcode (FFmpeg) --> Output[data/output]
```

## Prerequisites

- Docker Desktop
- Google Gemini API Key

## Setup

1.  Clone the repository:
    ```bash
    git clone https://github.com/naki0227/nue.git
    cd nue
    ```

2.  Create a `.env` file in the root directory:
    ```bash
    GEMINI_API_KEY=your_api_key_here
    GEMINI_MODEL=gemini-2.5-flash
    ```

3.  Build and start the services:
    ```bash
    docker compose up -d --build
    ```

## Usage

Upload a video file (`.mp4`, `.mov`, etc.) to the Gateway:

```bash
curl -F "file=@/path/to/your/video.mp4" http://localhost:8080/upload
```

The system will automatically:
1.  Receive the file.
2.  Upload it to Google Gemini for analysis.
3.  Generate editing instructions (JSON) determining "excitement points" and "editing style".
4.  Process the video using FFmpeg to apply filters (e.g., Sepia, Vivid, Grayscale) and cut scenes.
5.  Save the processed videos in `data/output/`.

## Directory Structure

- `gateway/`: Go API server (Gin framework).
- `brain/`: Python service (watchdog, google.genai).
- `muscle/`: Rust service (notify, FFmpeg wrapper).
- `data/`: Shared volume for inter-service communication.

## Future Roadmap

- [ ] Video Stitching (Concatenate clips into one video)
- [ ] Transitions (Crossfade, etc.)
- [ ] BGM/Audio Integration
- [ ] Text Overlays (AI-generated captions)
