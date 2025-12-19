FROM rust:1.75-slim-bookworm AS builder

WORKDIR /app

# Create a dummy project to cache dependencies
RUN cargo init
COPY Cargo.toml ./
RUN cargo build --release
RUN rm src/*.rs

# Copy actual source code
COPY . .
# Touch main.rs to ensure rebuild
RUN touch src/main.rs
RUN cargo build --release

FROM debian:bookworm-slim

# Install ffmpeg
RUN apt-get update && apt-get install -y ffmpeg fonts-dejavu-core && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/muscle .

# Setup directories
RUN mkdir -p /app/data/raw /app/data/json /app/data/output

CMD ["./muscle"]
