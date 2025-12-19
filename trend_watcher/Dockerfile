FROM python:3.11-slim

WORKDIR /app

# Install ffmpeg for yt-dlp audio extraction
RUN apt-get update && apt-get install -y ffmpeg && rm -rf /var/lib/apt/lists/*

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY . .

# Create data directories
RUN mkdir -p /app/data/raw /app/data/json /app/data/output /app/data/bgm /app/data/se

CMD ["python", "main.py"]
