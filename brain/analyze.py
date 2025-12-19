import os
import time
import json
import logging
import sys
from google import genai
from google.genai import types
from watchdog.observers.polling import PollingObserver as Observer
from watchdog.events import FileSystemEventHandler
from pythonjsonlogger import jsonlogger
from dotenv import load_dotenv

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
API_KEY = os.environ.get("GEMINI_API_KEY")
# User requested specific model name
MODEL_NAME = os.environ.get("GEMINI_MODEL", "gemini-2.5-flash") 

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

        logger.info("Uploading to Gemini", extra={"event": "upload_start", "file_name": filename})
        
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

            # Generate content
            prompt = """
            Analyze this video. 
            Output a JSON object with the following structure:
            {
                "cuts": [
                    {
                        "start_time": "HH:MM:SS",
                        "end_time": "HH:MM:SS",
                        "description": "Short description",
                        "filter": "suggested style/filter e.g. sepia, vivid, grayscale, none"
                    }
                ],
                "editing_style": {
                    "tempo": "fast/slow/dynamic",
                    "mood": "exciting/calm/etc"
                }
            }
            Focus on identifying excitement points and editing style. 
            Ensure strict JSON output.
            """
            
            # New SDK usage for generation
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
