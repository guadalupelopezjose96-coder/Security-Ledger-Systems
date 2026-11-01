FROM rust:1.80-slim-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

FROM gcr.io/distroless/cc-debian12:nonroot

WORKDIR /app

COPY --from=builder --chown=nonroot:nonroot /app/target/release/server /app/server

USER nonroot:nonroot

EXPOSE 8080

ENTRYPOINT ["/app/server"]
