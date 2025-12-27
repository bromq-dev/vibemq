//! PROXY Protocol Module
//!
//! Handles HAProxy PROXY protocol v1/v2 header parsing for all listeners.
//! Supports auto-detection of protocol version and extraction of TLS
//! termination information from PROXY v2 TLVs.

mod parser;

pub use parser::{parse_proxy_header, ProxyError, ProxyInfo, ProxyTlsInfo, ProxyVersion};
