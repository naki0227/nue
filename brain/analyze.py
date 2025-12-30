import os
import time
import json
import logging
import sys
import subprocess
from pathlib import Path
from google import genai
from google.genai import types
from watchdog.observers.polling import PollingObserver as Observer
from watchdog.events import FileSystemEventHandler
from pythonjsonlogger import jsonlogger
from dotenv import load_dotenv
from bgm_generator import generate_bgm_with_gemini

# Load env
load_dotenv()

# Configure JSON logging
logger = logging.getLogger()
logHandler = logging.StreamHandler(sys.stdout)
formatter = jsonlogger.JsonFormatter('%(asctime)s %(levelname)s %(message)s %(filename)s')
logHandler.setFormatter(formatter)
logger.addHandler(logHandler)
logger.setLevel(logging.INFO)

# Configuration
RAW_DIR = "/app/data/raw"
JSON_DIR = "/app/data/json"
BGM_DIR = "/app/data/bgm"
API_KEY = os.environ.get("GEMINI_API_KEY")
# User requested specific model name
MODEL_NAME = os.environ.get("GEMINI_MODEL", "gemini-2.5-flash")

# Ensure BGM directory exists
os.makedirs(BGM_DIR, exist_ok=True) 

client = None
if not API_KEY:
    logger.warning("GEMINI_API_KEY is not set. Brain service will not function correctly.", extra={"event": "config_warning"})
else:
    client = genai.Client(api_key=API_KEY)

class VideoHandler(FileSystemEventHandler):
    def on_created(self, event):
        if event.is_directory:
            return
        
        filepath = event.src_path
        filename = os.path.basename(filepath)
        
        if not filename.lower().endswith(('.mp4', '.mov', '.avi', '.mkv')):
            return

        logger.info(f"New video detected", extra={"event": "file_detected", "file_name": filename})
        try:
            self.process_video(filepath, filename)
        except Exception as e:
            logger.error(f"Error processing video", extra={"event": "process_error", "file_name": filename, "error": str(e)})

    def process_video(self, filepath, filename):
        if not client:
            logger.error("Client not initialized", extra={"event": "client_error"})
            return

        # Load metadata if exists
        metadata = load_metadata(filepath)
        
        logger.info("Uploading to Gemini", extra={"event": "upload_start", "file_name": filename, "has_metadata": metadata is not None})
        
        try:
            # Upload file
            video_file = client.files.upload(file=filepath)
            
            # Wait for processing
            while video_file.state.name == "PROCESSING":
                time.sleep(2)
                video_file = client.files.get(name=video_file.name)

            if video_file.state.name == "FAILED":
                raise ValueError(f"Video processing failed: {video_file.state.name}")

            logger.info("Video processed by Gemini", extra={"event": "upload_complete", "file_name": filename})

            # Fetch trending style
            style_context = ""
            bgm_path = None
            try:
                style, bgm_path = fetch_latest_style()
                if style:
                    style_context = f"""
                    APPLY THIS TRENDING STYLE:
                    - Cuts/Min aim: {style.get('cuts_per_min')}
                    - Filter: {style.get('filter_usage')}
                    - Transition: {style.get('transition_type')}
                    - Caption Style: {style.get('caption_style')}
                    """
            except Exception as e:
                logger.error(f"Failed to fetch style: {e}")

            # Prepare script context if provided
            script_context = ""
            if metadata and metadata.get("script"):
                script_context = f"""
                USER PROVIDED SCRIPT/TRANSCRIPT:
                {metadata['script']}
                
                Use this to better understand timing and context.
                """
            
            # Check options
            options = metadata.get("options", {}) if metadata else {}
            auto_sound_effects = options.get("auto_sound_effects", False)
            generate_bgm = options.get("generate_bgm", False)
            
            # Generate content
            prompt = f"""
            Analyze this video.
            {script_context} 
            {style_context}
            
            {'AUDIO ANALYSIS: Identify moments for sound effects based on speech emphasis, laughter, pauses, and reactions.' if auto_sound_effects else ''}
            
            Output a JSON object with the following structure:
            {{
                "cuts": [
                    {{
                        "start_time": "HH:MM:SS",
                        "end_time": "HH:MM:SS",
                        "description": "Short description",
                        "filter": "none",  # Always use 'none' to disable filters
                        "transition_type": "fade/wipeleft/slideup/circleopen (default: {style.get('transition_type', 'fade')})",
                        "focus_point": 0.5,
                        "caption": "Short, punchy text overlay (e.g. 'WOW!', 'Nice!')",
                        "caption_style": {{
                            "font": "sans/serif/handwriting (default: {style.get('caption_style', 'sans')})",
                            "color": "white/yellow/cyan",
                            "position": "bottom/center/top",
                            "box": true/false,
                            "background_asset": "simple_box/news_ticker/none (choose appropriate style)"
                        }}
                    }}
                ],
                "editing_style": {{
                    "tempo": "fast/slow/dynamic",
                    "mood": "exciting/calm/etc"
                }},
                "se_events": [
                    {{
                        "timestamp": "HH:MM:SS",
                        "type": "impact/whoosh/laugh/correct/incorrect (e.g. use 'impact' for Emphasis)",
                        "tag": "funny/serious/etc"
                    }}
                ],
                "visual_effects": [
                    {{
                        "start": "HH:MM:SS",
                        "end": "HH:MM:SS",
                        "type": "zoom_in/pan_left/pan_right/zoom_out",
                        "speed": "slow/fast (default: fast for zoom_in, slow for pan)"
                    }}
                ],
                "thumbnail": {{
                    "timestamp": "HH:MM:SS (Best frame for clickbait)",
                    "text": "Short Uppercase Title (e.g. SHOCKING!)",
                    "color": "red/yellow/white"
                }}
            }}

            
            # New SDK usage for generation
            Focus on identifying excitement points and editing style.
            IMPORTANT: 
            1. Do not caption every single segment. Be selective. Prioritize reactions.
            2. ADD SOUND EFFECTS (SE) where appropriate.
            3. ADD VISUAL EFFECTS (Zoom/Pan).
            4. THUMBNAIL: Choose the most expressive/shocking frame and a short punchy title.
            5. VERTICAL CROP: For each cut, determine the `focus_point` (0.0-1.0) where the subject is located horizontally. 0.5 is center.
            Ensure strict JSON output.
            """
            response = client.models.generate_content(
                model=MODEL_NAME,
                contents=[video_file, prompt],
                config=types.GenerateContentConfig(response_mime_type="application/json")
            )
            
            text = response.text.strip()
            # Cleanup markdown if present (though response_mime_type should prevent it)
            if text.startswith("```json"):
                text = text[7:-3]
            elif text.startswith("```"):
                text = text[3:-3]

            data = json.loads(text)
            
            # Merge manual instructions if provided
            if metadata and metadata.get("manual_instructions"):
                data = merge_instructions(data, metadata["manual_instructions"])
                logger.info("Merged manual instructions", extra={"event": "instructions_merged"})
            
            # Generate BGM if requested
            # Select professional BGM based on video mood
            mood = data.get("editing_style", {}).get("mood", "").lower()
            
            # Professional BGM Library (Kevin MacLeod)
            bgm_library = {
                "energetic": [
                    "energetic_pro.mp3", "Monkeys_Spinning_Monkeys.mp3", "Fluffing_a_Duck.mp3", 
                    "Run_Amok.mp3", "Swing_Machine.mp3", "The_Builder.mp3", "Pixel_Peeker_Polka_faster.mp3"
                ],
                "upbeat": [
                    "upbeat_pro.mp3", "New_Friendly.mp3", "Carefree.mp3", "Sneaky_Snitch.mp3", 
                    "Scheming_Weasel_faster.mp3"
                ],
                "calm": [
                    "calm_pro.mp3", "Easy_Lemon.mp3", "Sheep_May_Safely_Graze.mp3", 
                    "Dreams_Become_Real.mp3", "Almost_Bliss.mp3", "Local_Forecast_Elevator.mp3"
                ],
                "dramatic": [
                    "dramatic_pro.mp3", "Volatile_Reaction.mp3", "The_Complex.mp3", 
                    "Hitman.mp3", "Day_of_Chaos.mp3", "Sneaky_Adventure.mp3"
                ],
                "happy": [
                    "happy_pro.mp3", "Carefree.mp3", "Fluffing_a_Duck.mp3", "Monkeys_Spinning_Monkeys.mp3"
                ]
            }
            
            # Map specific keywords to categories
            mood_category_map = {
                "exciting": "energetic", "fun": "energetic", "peaceful": "calm", 
                "relaxing": "calm", "serious": "dramatic", "mysterious": "dramatic", 
                "cheerful": "happy", "positive": "upbeat"
            }
            
            # Determine category
            category = mood_category_map.get(mood, mood)
            if category not in bgm_library:
                if "calm" in mood or "quiet" in mood: category = "calm"
                elif "sad" in mood or "dark" in mood: category = "dramatic"
                else: category = "energetic" # Default
            
            # Select random track from category
            import random
            available_tracks = bgm_library.get(category, bgm_library["energetic"])
            
            # Filter to ensure file exists
            valid_tracks = [t for t in available_tracks if os.path.exists(os.path.join(BGM_DIR, t))]
            if not valid_tracks: valid_tracks = ["default_bgm.mp3"]
            
            bgm_filename = random.choice(valid_tracks)
            bgm_path = os.path.join(BGM_DIR, bgm_filename)
            
            data["bgm_path"] = bgm_path
            logger.info(f"Selected BGM: {bgm_filename} for mood: {mood} (category: {category})", 
                       extra={"event": "bgm_selected", "mood": mood, "bgm": bgm_filename})
            
            data["original_filename"] = filename
            
            output_path = os.path.join(JSON_DIR, f"{filename}.json")
            with open(output_path, "w") as f:
                json.dump(data, f, indent=2)
            
            logger.info("Analysis saved", extra={"event": "analysis_saved", "path": output_path})
            
            # Cleanup remote file
            try:
                client.files.delete(name=video_file.name)
            except:
                pass
            
        except Exception as e:
            logger.error("Failed to process with Gemini", extra={"event": "gemini_error", "error": str(e)})
            raise e

def fetch_latest_style():
    db_path = "/app/data/trends.db"
    if not os.path.exists(db_path):
        return None, None
        
    import sqlite3
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    cursor = conn.cursor()
    
    try:
        # Get latest style
        cursor.execute("SELECT * FROM styles ORDER BY created_at DESC LIMIT 1")
        style = cursor.fetchone()
        
        bgm_path = None
        if style:
            # Try to find matching BGM
            bgm_mood = style['bgm_mood']
            if bgm_mood:
                cursor.execute("SELECT path FROM assets WHERE type='bgm' AND tags LIKE ? ORDER BY RANDOM() LIMIT 1", (f"%{bgm_mood}%",))
                bgm = cursor.fetchone()
                if bgm:
                    bgm_path = bgm['path']
        
        return dict(style) if style else None, bgm_path
    
    except Exception as e:
        logger.error(f"DB Error: {e}")
        return None, None
    finally:
        conn.close()

def load_metadata(video_path):
    """Load metadata JSON file if it exists"""
    metadata_path = video_path + "_metadata.json"
    if os.path.exists(metadata_path):
        try:
            with open(metadata_path, 'r') as f:
                return json.load(f)
        except Exception as e:
            logger.error(f"Failed to load metadata: {e}")
    return None

def extract_audio(video_path):
    """Extract audio from video for analysis"""
    audio_path = video_path.replace(Path(video_path).suffix, "_audio.wav")
    try:
        subprocess.run([
            "ffmpeg", "-y", "-i", video_path,
            "-vn", "-acodec", "pcm_s16le", "-ar", "16000", "-ac", "1",
            audio_path
        ], check=True, capture_output=True)
        return audio_path
    except Exception as e:
        logger.error(f"Audio extraction failed: {e}")
        return None

def merge_instructions(ai_data, manual_instructions):
    """Merge manual instructions with AI analysis, prioritizing user instructions"""
    if not manual_instructions:
        return ai_data
    
    # Priority: User instructions > AI suggestions
    manual_cuts = manual_instructions.get("cuts", [])
    manual_captions = manual_instructions.get("captions", [])
    manual_effects = manual_instructions.get("effects", [])
    
    # Apply manual cuts (remove or keep specific segments)
    if manual_cuts:
        for manual_cut in manual_cuts:
            if manual_cut["action"] == "remove":
                # Remove AI-generated cuts that fall within this timeframe
                ai_data["cuts"] = [
                    cut for cut in ai_data["cuts"]
                    if not (cut["start_time"] >= manual_cut["start"] and cut["end_time"] <= manual_cut["end"])
                ]
    
    # Add manual captions to cuts
    if manual_captions:
        for manual_cap in manual_captions:
            # Find the cut that contains this timestamp and add caption
            for cut in ai_data["cuts"]:
                if cut["start_time"] <= manual_cap["timestamp"] <= cut["end_time"]:
                    cut["caption"] = manual_cap["text"]
                    if "caption_style" not in cut:
                        cut["caption_style"] = {}
                    cut["caption_style"]["color"] = manual_cap["style"]
                    break
    
    # Add manual effects
    if manual_effects:
        if "visual_effects" not in ai_data:
            ai_data["visual_effects"] = []
        for manual_fx in manual_effects:
            ai_data["visual_effects"].append({
                "start": manual_fx["timestamp"],
                "end": manual_fx["timestamp"],  # Instant effect
                "type": manual_fx["type"],
                "speed": "fast"
            })
    
    return ai_data

def calculate_video_duration(video_path):
    """Get video duration in seconds"""
    try:
        result = subprocess.run([
            "ffprobe", "-v", "error", "-show_entries",
            "format=duration", "-of", "default=noprint_wrappers=1:nokey=1",
            video_path
        ], capture_output=True, text=True, check=True)
        return float(result.stdout.strip())
    except:
        return 60  # Default to 60 seconds if ffprobe fails

if __name__ == "__main__":
    os.makedirs(RAW_DIR, exist_ok=True)
    os.makedirs(JSON_DIR, exist_ok=True)

    event_handler = VideoHandler()
    observer = Observer()
    observer.schedule(event_handler, RAW_DIR, recursive=False)
    observer.start()
    
    logger.info(f"Brain service started", extra={"event": "startup", "watched_dir": RAW_DIR, "model": MODEL_NAME})
    
    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        observer.stop()
    observer.join()
