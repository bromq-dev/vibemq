//! PROXY Protocol Configuration
//!
//! Configuration types for HAProxy PROXY protocol v1/v2 support.

use serde::Deserialize;
use std::time::Duration;

/// PROXY protocol configuration for a listener
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProxyProtocolConfig {
    /// Enable PROXY protocol parsing on this listener
    pub enabled: bool,

    /// Trust TLS termination info from PROXY v2 TLVs.
    /// When true, parse PP2_TYPE_SSL TLVs for SNI and client cert CN.
    pub tls_termination: bool,

    /// Timeout for reading PROXY header in seconds.
    /// Default: 5 seconds
    pub timeout: u64,
}

impl Default for ProxyProtocolConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tls_termination: false,
            timeout: 5,
        }
    }
}

impl ProxyProtocolConfig {
    /// Get timeout as Duration
    pub fn timeout_duration(&self) -> Duration {
        Duration::from_secs(self.timeout)
    }
}
