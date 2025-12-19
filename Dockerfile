FROM lukemathwalker/cargo-chef:latest-rust-1-alpine AS chef
RUN apk add --no-cache musl-dev build-base
WORKDIR /app

# Get platform from buildx
ARG TARGETPLATFORM

# Determine correct Rust target triple
RUN case "$TARGETPLATFORM" in \
    "linux/amd64")   echo "x86_64-unknown-linux-musl"        > /rust_target ;; \
    "linux/arm64")   echo "aarch64-unknown-linux-musl"       > /rust_target ;; \
    *) echo "Unsupported platform: $TARGETPLATFORM" && exit 1 ;; \
    esac
RUN rustup target add $(cat /rust_target)

# Planner stage - analyze dependencies
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Builder stage - cook dependencies (cached) then build
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this layer is cached until Cargo.toml/Cargo.lock change
RUN cargo chef cook --release --target $(cat /rust_target) --recipe-path recipe.json
# Copy source and build
COPY . .
RUN cargo build --release --target $(cat /rust_target)
RUN cp target/$(cat /rust_target)/release/vibemq /vibemq

# Final minimal image
FROM scratch
COPY --from=builder /vibemq /vibemq
COPY ./vibemq.toml /etc/vibemq/config.toml

EXPOSE 1883 9001
ENTRYPOINT ["/vibemq"]
CMD ["--config", "/etc/vibemq/config.toml"]
