import os
import sys
import sqlite3
import logging
import argparse
from pythonjsonlogger import jsonlogger

# Configure logging
logger = logging.getLogger()
logHandler = logging.StreamHandler(sys.stdout)
formatter = jsonlogger.JsonFormatter('%(asctime)s %(levelname)s %(message)s %(filename)s')
logHandler.setFormatter(formatter)
logger.addHandler(logHandler)
logger.setLevel(logging.INFO)

DB_PATH = "/app/data/trends.db"

import yt_dlp

# ... logging setup ...

def init_db():
    conn = sqlite3.connect(DB_PATH)
    cursor = conn.cursor()
    
    # Styles table
    cursor.execute('''
    CREATE TABLE IF NOT EXISTS styles (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        genre TEXT NOT NULL,
        cuts_per_min REAL,
        avg_shot_duration REAL,
        filter_usage TEXT,
        transition_type TEXT,
        caption_style TEXT,
        bgm_mood TEXT,
        se_tags TEXT,
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    )
    ''')
    
    # Assets table (BGM/SE)
    cursor.execute('''
    CREATE TABLE IF NOT EXISTS assets (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        type TEXT NOT NULL, -- 'bgm' or 'se'
        name TEXT,
        path TEXT,
        tags TEXT,
        source_url TEXT,
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    )
    ''')

    # Source Videos table
    cursor.execute('''
    CREATE TABLE IF NOT EXISTS source_videos (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        video_id TEXT UNIQUE NOT NULL,
        title TEXT,
        genre TEXT,
        view_count INTEGER,
        processed_at TIMESTAMP DEFAULT NULL,
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    )
    ''')
    
    conn.commit()
    conn.close()
    logger.info("Database initialized", extra={"event": "db_init", "path": DB_PATH})

import json
from google import genai
from google.genai import types
from dotenv import load_dotenv

load_dotenv()
API_KEY = os.environ.get("GEMINI_API_KEY")

def analyze_video(filepath, genre):
    if not API_KEY:
        logger.error("GEMINI_API_KEY not set")
        return None
        
    client = genai.Client(api_key=API_KEY)
    
    try:
        logger.info("Uploading for analysis...", extra={"event": "upload_start", "path": filepath})
        video_file = client.files.upload(file=filepath)
        
        # Wait for processing
        import time
        while video_file.state.name == "PROCESSING":
            time.sleep(2)
            video_file = client.files.get(name=video_file.name)
            
        if video_file.state.name == "FAILED":
            logger.error("Video processing failed")
            return None

        prompt = f"""
        Analyze this {genre} video style.
        Output JSON:
        {{
            "cuts_per_min": 0.0,
            "avg_shot_duration": 0.0,
            "filter_usage": "dominant filter or 'none'",
            "transition_type": "cut/fade/mix",
            "caption_style": "minimal/heavy/colorful/none",
            "bgm_mood": "energetic/calm/lofi/etc",
            "se_tags": ["whoosh", "pop", "laugh", "none"]
        }}
        """
        
        response = client.models.generate_content(
            model="gemini-2.5-flash",
            contents=[video_file, prompt],
            config=types.GenerateContentConfig(response_mime_type="application/json")
        )
        
        # Cleanup
        try:
            client.files.delete(name=video_file.name)
        except:
            pass

        return json.loads(response.text)
        
    except Exception as e:
        logger.error(f"Analysis failed: {e}")
        return None

def download_asset(query, asset_type, cursor, conn):
    logger.info(f"Searching asset: {query}", extra={"type": asset_type})
    
    asset_dir = f"/app/data/{asset_type}"
    os.makedirs(asset_dir, exist_ok=True)
    
    ydl_opts = {
        'format': 'bestaudio/best',
        'quiet': True,
        'ignoreerrors': True,
        'outtmpl': f'{asset_dir}/%(id)s.%(ext)s',
        'noplaylist': True,
    }
    
    try:
        with yt_dlp.YoutubeDL(ydl_opts) as ydl:
            # Search 1 candidate
            result = ydl.extract_info(f"ytsearch1:{query}", download=False)
            
            if 'entries' in result and result['entries']:
                entry = result['entries'][0]
                video_id = entry.get('id')
                title = entry.get('title')
                webpage_url = entry.get('webpage_url')
                
                # Check duplication in assets
                cursor.execute('SELECT id FROM assets WHERE source_url = ?', (webpage_url,))
                if cursor.fetchone():
                    logger.info("Asset already exists", extra={"title": title})
                    return

                logger.info(f"Downloading asset: {title}")
                ydl.download([webpage_url])
                
                # Save to DB
                # Extension unknown without post-processing, use simple glob or similar if needed for exact path
                # For now, store the expected path pattern (yt-dlp uses video_id)
                file_path = f"{asset_dir}/{video_id}" 
                
                cursor.execute('''
                    INSERT INTO assets (type, name, path, tags, source_url)
                    VALUES (?, ?, ?, ?, ?)
                ''', (asset_type, title, file_path, query, webpage_url))
                conn.commit()
                logger.info("Asset saved", extra={"title": title})
                
    except Exception as e:
        logger.error(f"Asset download failed: {e}")

def crawl(genre, limit):
    logger.info(f"Starting crawl for genre: {genre}", extra={"event": "crawl_start", "genre": genre, "limit": limit})
    
    # Create sample dir
    sample_dir = "/app/data/raw/trend_samples"
    os.makedirs(sample_dir, exist_ok=True)

    ydl_opts = {
        'format': 'best[ext=mp4]/best',
        'quiet': True,
        'ignoreerrors': True,
        'outtmpl': f'{sample_dir}/%(id)s.%(ext)s',
        'download_sections': [{"start_time": 0, "end_time": 60}], # Download first 60s
        'force_keyframes_at_cuts': True,
    }

    search_query = f"ytsearch{limit}:{genre} vlog"

    conn = sqlite3.connect(DB_PATH)
    cursor = conn.cursor()

    try:
        with yt_dlp.YoutubeDL(ydl_opts) as ydl:
            # First extract info to check dupes
            logger.info("Searching...", extra={"event": "search_start"})
            result = ydl.extract_info(search_query, download=False)
            
            if 'entries' in result:
                for entry in result['entries']:
                    if not entry: continue
                    
                    video_id = entry.get('id')
                    title = entry.get('title')
                    
                    # Check duplication
                    cursor.execute('SELECT id FROM source_videos WHERE video_id = ?', (video_id,))
                    if cursor.fetchone():
                        logger.info("Skipping duplicate", extra={"video_id": video_id})
                        continue

                    logger.info(f"Processing: {title}")
                    
                    # Record video
                    cursor.execute('''
                        INSERT INTO source_videos (video_id, title, genre, view_count)
                        VALUES (?, ?, ?, ?)
                    ''', (video_id, title, genre, entry.get('view_count', 0)))
                    conn.commit()

                    # Download sample
                    logger.info(f"Downloading sample for {video_id}...")
                    url = entry.get('webpage_url')
                    if url:
                         ydl.download([url])
                    else:
                         logger.warning(f"No URL found for {video_id}")
                         continue
                    
                    # File path
                    filename = f"{video_id}.mp4"
                    filepath = os.path.join(sample_dir, filename)
                    
                    if os.path.exists(filepath):
                        # Analyze
                        logger.info("Analyzing style...")
                        style = analyze_video(filepath, genre)
                        
                        if style:
                            cursor.execute('''
                                INSERT INTO styles (genre, cuts_per_min, avg_shot_duration, filter_usage, transition_type, caption_style, bgm_mood, se_tags)
                                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                            ''', (
                                genre,
                                style.get('cuts_per_min', 0),
                                style.get('avg_shot_duration', 0),
                                style.get('filter_usage', 'none'),
                                style.get('transition_type', 'cut'),
                                style.get('caption_style', 'none'),
                                style.get('bgm_mood', 'unknown'),
                                json.dumps(style.get('se_tags', []))
                            ))
                            conn.commit()
                            logger.info("Style saved", extra={"style": style})

                            # Collect Assets
                            # BGM
                            bgm_mood = style.get('bgm_mood')
                            if bgm_mood:
                                download_asset(f"No copyright music {bgm_mood}", "bgm", cursor, conn)
                            
                            # SE
                            se_tags = style.get('se_tags', [])
                            for tag in se_tags:
                                if tag and tag != "none":
                                    download_asset(f"Sound effect {tag}", "se", cursor, conn)
                        
                        # Cleanup sample to save space
                        os.remove(filepath)

    except Exception as e:
        logger.error(f"Crawl failed: {e}", extra={"event": "crawl_error", "error": str(e)})
    finally:
        conn.close()

def main():
    parser = argparse.ArgumentParser(description="Trend Watcher Service")
    subparsers = parser.add_subparsers(dest="command")
    
    # Init DB command
    subparsers.add_parser("init_db", help="Initialize the database")
    
    # Crawl command
    crawl_parser = subparsers.add_parser("crawl", help="Crawl trending videos")
    crawl_parser.add_argument("--genre", type=str, default="vlog", help="Genre to search for")
    crawl_parser.add_argument("--limit", type=int, default=5, help="Number of videos to fetch")
    
    args = parser.parse_args()
    
    # Ensure DB is initialized
    init_db()
    
    if args.command == "crawl":
        crawl(args.genre, args.limit)
    elif args.command == "init_db":
        pass # Already called init_db()
    else:
        # Default behavior: run as a service/cron? Or just exit for now.
        logger.info("No command specified, running idle loop...", extra={"event": "idle"})
        import time
        while True:
            time.sleep(3600)

if __name__ == "__main__":
    main()
