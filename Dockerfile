FROM rust:1.91-alpine AS builder

# Install MUSL compiler + build tools
RUN apk add --no-cache musl-dev build-base

# Get platform from buildx
ARG TARGETPLATFORM

# Determine correct Rust target triple
# Note: armv7 not supported - rust:alpine images don't have arm/v7 variant
RUN case "$TARGETPLATFORM" in \
    "linux/amd64")   echo "x86_64-unknown-linux-musl"        > /rust_target ;; \
    "linux/arm64")   echo "aarch64-unknown-linux-musl"       > /rust_target ;; \
    *) echo "Unsupported platform: $TARGETPLATFORM" && exit 1 ;; \
    esac

# Add the rust target
RUN rustup target add $(cat /rust_target)

WORKDIR /app

# Copy manifests first for caching
COPY Cargo.toml Cargo.lock ./

# Dummy src for dependency caching
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies (cached via BuildKit)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release --target $(cat /rust_target)

# Remove dummy
RUN rm -rf src

# Copy actual source code
COPY src ./src

# Touch main.rs to invalidate the binary but not deps
RUN touch src/main.rs

# Build final binary (deps cached via BuildKit)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release --target $(cat /rust_target) && \
    cp target/$(cat /rust_target)/release/vibemq /vibemq

FROM scratch

COPY --from=builder /vibemq /vibemq
COPY ./vibemq.toml /etc/vibemq/config.toml

EXPOSE 1883 9001
ENTRYPOINT ["/vibemq"]
CMD ["--config", "/etc/vibemq/config.toml"]
