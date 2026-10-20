# Rust CLI/worker
FROM rust:1.88-slim-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# Cache dependencies separately from the source.
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release --locked && rm -rf src
COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

# whisper.cpp for on-device subtitles (no API). Set WHISPER_MODEL=none to skip
# the model download, or pick tiny/base/small/medium (bigger = better, slower).
FROM debian:bookworm-slim AS whisper
ARG WHISPER_VERSION=v1.7.6
ARG WHISPER_MODEL=small
RUN apt-get update && apt-get install -y --no-install-recommends \
      git cmake build-essential curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 --branch ${WHISPER_VERSION} https://github.com/ggml-org/whisper.cpp /whisper
WORKDIR /whisper
RUN cmake -B build -DWHISPER_BUILD_TESTS=OFF -DBUILD_SHARED_LIBS=OFF -DGGML_NATIVE=OFF \
    && cmake --build build -j --config Release --target whisper-cli
RUN mkdir -p /models \
    && if [ "${WHISPER_MODEL}" != "none" ]; then sh ./models/download-ggml-model.sh "${WHISPER_MODEL}" /models; fi

# Open-licensed (SIL OFL) fonts used by the editing formats.
FROM debian:bookworm-slim AS fonts
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY scripts/fetch-fonts.sh /fetch-fonts.sh
RUN sh /fetch-fonts.sh /fonts

FROM debian:bookworm-slim
ARG WHISPER_MODEL=small
RUN apt-get update && apt-get install -y --no-install-recommends \
      ffmpeg fonts-noto-cjk fontconfig ca-certificates libgomp1 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/muscle /usr/local/bin/muscle
COPY --from=whisper /whisper/build/bin/whisper-cli /usr/local/bin/whisper-cli
COPY --from=whisper /models /opt/whisper-models
COPY --from=fonts /fonts /usr/share/fonts/truetype/nue
RUN fc-cache -f
ENV NUE_DATA_DIR=/app/data \
    NUE_WHISPER_MODEL=/opt/whisper-models/ggml-${WHISPER_MODEL}.bin
WORKDIR /app
RUN mkdir -p /app/data/raw /app/data/json /app/data/output /app/data/temp /app/data/jobs
CMD ["muscle", "worker"]
