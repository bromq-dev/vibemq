//! CPU Profiling Support
//!
//! Provides pprof-compatible CPU profiling endpoint.
//! Enable with `--features pprof` at compile time.
//!
//! Usage:
//!   # Build with profiling
//!   cargo build --release --features pprof
//!
//!   # Collect 30s CPU profile
//!   curl http://localhost:6060/debug/pprof/profile?seconds=30 > profile.pb
//!
//!   # Generate flamegraph
//!   pprof -http=:8080 profile.pb

use std::net::SocketAddr;
use std::time::Duration;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use pprof::protos::Message;
use tokio::net::TcpListener;
use tracing::{error, info};

/// Default profiling server bind address
pub const DEFAULT_BIND: &str = "127.0.0.1:6060";

/// Start the profiling HTTP server
pub async fn start_server(bind: SocketAddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(bind).await?;
    info!("Profiling server listening on http://{}/debug/pprof/profile", bind);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);

        tokio::spawn(async move {
            if let Err(e) = http1::Builder::new()
                .serve_connection(io, service_fn(handle_request))
                .await
            {
                error!("Profiling server error: {}", e);
            }
        });
    }
}

async fn handle_request(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let response = match (req.method(), req.uri().path()) {
        (&Method::GET, "/debug/pprof/profile") => {
            // Parse duration from query string
            let seconds = req
                .uri()
                .query()
                .and_then(|q| {
                    q.split('&')
                        .find(|p| p.starts_with("seconds="))
                        .and_then(|p| p.strip_prefix("seconds="))
                        .and_then(|s| s.parse().ok())
                })
                .unwrap_or(30u64);

            match collect_profile(seconds).await {
                Ok(data) => Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/octet-stream")
                    .header(
                        "Content-Disposition",
                        "attachment; filename=\"profile.pb\"",
                    )
                    .body(Full::new(Bytes::from(data)))
                    .unwrap(),
                Err(e) => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Full::new(Bytes::from(format!("Profile error: {}", e))))
                    .unwrap(),
            }
        }
        (&Method::GET, "/debug/pprof/flamegraph") => {
            // Parse duration from query string
            let seconds = req
                .uri()
                .query()
                .and_then(|q| {
                    q.split('&')
                        .find(|p| p.starts_with("seconds="))
                        .and_then(|p| p.strip_prefix("seconds="))
                        .and_then(|s| s.parse().ok())
                })
                .unwrap_or(30u64);

            match collect_flamegraph(seconds).await {
                Ok(svg) => Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "image/svg+xml")
                    .body(Full::new(Bytes::from(svg)))
                    .unwrap(),
                Err(e) => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Full::new(Bytes::from(format!("Flamegraph error: {}", e))))
                    .unwrap(),
            }
        }
        (&Method::GET, "/") | (&Method::GET, "/debug/pprof") => {
            let html = r#"<!DOCTYPE html>
<html>
<head><title>VibeMQ Profiling</title></head>
<body>
<h1>VibeMQ Profiling</h1>
<ul>
  <li><a href="/debug/pprof/profile?seconds=30">CPU Profile (30s, protobuf)</a></li>
  <li><a href="/debug/pprof/flamegraph?seconds=30">Flamegraph (30s, SVG)</a></li>
</ul>
<p>Usage:</p>
<pre>
# Download profile
curl http://localhost:6060/debug/pprof/profile?seconds=30 > profile.pb

# View with pprof
go tool pprof -http=:8080 profile.pb

# Or get SVG flamegraph directly
curl http://localhost:6060/debug/pprof/flamegraph?seconds=10 > flamegraph.svg
</pre>
</body>
</html>"#;
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/html")
                .body(Full::new(Bytes::from(html)))
                .unwrap()
        }
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::new(Bytes::from("Not Found")))
            .unwrap(),
    };

    Ok(response)
}

async fn collect_profile(seconds: u64) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    info!("Starting {}s CPU profile", seconds);

    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(1000)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()?;

    tokio::time::sleep(Duration::from_secs(seconds)).await;

    let report = guard.report().build()?;
    let mut buf = Vec::new();
    let profile = report.pprof()?;
    profile.encode(&mut buf)?;

    info!("CPU profile collected ({} bytes)", buf.len());
    Ok(buf)
}

async fn collect_flamegraph(
    seconds: u64,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    info!("Starting {}s flamegraph collection", seconds);

    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(1000)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()?;

    tokio::time::sleep(Duration::from_secs(seconds)).await;

    let report = guard.report().build()?;
    let mut buf = Vec::new();
    report.flamegraph(&mut buf)?;

    info!("Flamegraph collected ({} bytes)", buf.len());
    Ok(buf)
}
