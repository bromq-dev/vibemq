FROM rust:1.83-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM gcr.io/distroless/cc-debian12

COPY --from=builder --chmod=755 /app/target/release/vibemq /usr/local/bin/vibemq

EXPOSE 1883 9001
ENTRYPOINT ["/usr/local/bin/vibemq"]
