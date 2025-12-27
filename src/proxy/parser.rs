//! PROXY Protocol Parser
//!
//! Auto-detects and parses PROXY v1 (text) and v2 (binary) headers.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use bytes::BytesMut;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::timeout;

/// PROXY v1 signature: "PROXY "
const PROXY_V1_SIGNATURE: &[u8] = b"PROXY ";

/// PROXY v2 signature (12 bytes)
const PROXY_V2_SIGNATURE: &[u8] = b"\r\n\r\n\x00\r\nQUIT\n";

/// Maximum PROXY header size
const MAX_HEADER_SIZE: usize = 536;

/// Information extracted from a PROXY protocol header
#[derive(Debug, Clone)]
pub struct ProxyInfo {
    /// Original client address (source from PROXY header)
    pub client_addr: SocketAddr,

    /// Server address the client connected to (destination from PROXY header)
    pub server_addr: Option<SocketAddr>,

    /// TLS termination info from PROXY v2 TLVs (if present and trusted)
    pub tls_info: Option<ProxyTlsInfo>,

    /// Protocol version used (v1 or v2)
    pub version: ProxyVersion,
}

/// TLS termination information from PROXY v2 TLVs
#[derive(Debug, Clone)]
pub struct ProxyTlsInfo {
    /// Server Name Indication (SNI) from PP2_TYPE_AUTHORITY
    pub sni: Option<String>,

    /// Client certificate Common Name (CN) from PP2_SUBTYPE_SSL_CN
    pub client_cert_cn: Option<String>,

    /// Whether client provided a verified certificate
    pub client_cert_verified: bool,
}

/// PROXY protocol version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyVersion {
    V1,
    V2,
}

/// Errors that can occur during PROXY header parsing
#[derive(Debug)]
pub enum ProxyError {
    /// Timeout waiting for PROXY header
    Timeout,
    /// Invalid or malformed PROXY header
    InvalidHeader(String),
    /// IO error reading from socket
    Io(std::io::Error),
    /// Connection closed before header received
    ConnectionClosed,
    /// PROXY protocol not detected (no signature)
    NotProxyProtocol,
}

impl std::fmt::Display for ProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProxyError::Timeout => write!(f, "PROXY header timeout"),
            ProxyError::InvalidHeader(msg) => write!(f, "invalid PROXY header: {}", msg),
            ProxyError::Io(e) => write!(f, "IO error: {}", e),
            ProxyError::ConnectionClosed => write!(f, "connection closed"),
            ProxyError::NotProxyProtocol => write!(f, "no PROXY protocol signature"),
        }
    }
}

impl std::error::Error for ProxyError {}

impl From<std::io::Error> for ProxyError {
    fn from(e: std::io::Error) -> Self {
        ProxyError::Io(e)
    }
}

/// Parse PROXY protocol header from a stream
///
/// This function:
/// 1. Reads initial bytes to detect v1 vs v2
/// 2. Parses the appropriate format
/// 3. Extracts client address and optional TLS info
///
/// Returns the parsed ProxyInfo and any remaining bytes that should be
/// prepended to the stream for subsequent reads.
pub async fn parse_proxy_header<S: AsyncRead + Unpin>(
    stream: &mut S,
    timeout_duration: Duration,
    parse_tls_info: bool,
) -> Result<(ProxyInfo, BytesMut), ProxyError> {
    let mut buf = BytesMut::with_capacity(MAX_HEADER_SIZE);

    // Read initial bytes with timeout
    let result = timeout(
        timeout_duration,
        read_until_header_complete(stream, &mut buf),
    )
    .await;

    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err(ProxyError::Timeout),
    }

    // Detect version and parse
    if buf.len() >= 12 && buf[..12] == *PROXY_V2_SIGNATURE {
        parse_v2_header(&buf, parse_tls_info)
    } else if buf.len() >= 6 && buf[..6] == *PROXY_V1_SIGNATURE {
        parse_v1_header(&buf)
    } else {
        Err(ProxyError::NotProxyProtocol)
    }
}

/// Read bytes until we have a complete PROXY header
async fn read_until_header_complete<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
) -> Result<(), ProxyError> {
    // Initial read - need at least 16 bytes to detect version and read v2 length
    buf.resize(16, 0);
    let mut total_read = 0;

    while total_read < 16 {
        let n = stream.read(&mut buf[total_read..16]).await?;
        if n == 0 {
            return Err(ProxyError::ConnectionClosed);
        }
        total_read += n;
    }

    // Detect version from signature
    if buf[..12] == *PROXY_V2_SIGNATURE {
        // V2: Read the full header based on length field
        // Length is in bytes 14-15 (big-endian u16)
        let header_len = u16::from_be_bytes([buf[14], buf[15]]) as usize;
        let total_len = 16 + header_len;

        if total_len > MAX_HEADER_SIZE {
            return Err(ProxyError::InvalidHeader(format!(
                "v2 header too large: {} bytes",
                total_len
            )));
        }

        buf.resize(total_len, 0);
        while total_read < total_len {
            let n = stream.read(&mut buf[total_read..total_len]).await?;
            if n == 0 {
                return Err(ProxyError::ConnectionClosed);
            }
            total_read += n;
        }
    } else if buf[..6] == *PROXY_V1_SIGNATURE {
        // V1: Read until CRLF (max 107 bytes)
        buf.truncate(total_read);

        loop {
            if buf.windows(2).any(|w| w == b"\r\n") {
                break;
            }
            if buf.len() >= 107 {
                return Err(ProxyError::InvalidHeader("v1 header too long".to_string()));
            }
            let mut tmp = [0u8; 1];
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                return Err(ProxyError::ConnectionClosed);
            }
            buf.extend_from_slice(&tmp[..n]);
        }
    } else {
        return Err(ProxyError::NotProxyProtocol);
    }

    Ok(())
}

/// Parse a PROXY v1 (text) header
fn parse_v1_header(buf: &[u8]) -> Result<(ProxyInfo, BytesMut), ProxyError> {
    // Find end of header (CRLF)
    let header_end = buf
        .windows(2)
        .position(|w| w == b"\r\n")
        .ok_or_else(|| ProxyError::InvalidHeader("no CRLF found".to_string()))?
        + 2;

    // Parse using ppp crate
    match ppp::v1::Header::try_from(&buf[..header_end]) {
        Ok(header) => {
            let (client_addr, server_addr) = match header.addresses {
                ppp::v1::Addresses::Tcp4(addrs) => {
                    let client =
                        SocketAddr::new(IpAddr::V4(addrs.source_address), addrs.source_port);
                    let server = SocketAddr::new(
                        IpAddr::V4(addrs.destination_address),
                        addrs.destination_port,
                    );
                    (client, Some(server))
                }
                ppp::v1::Addresses::Tcp6(addrs) => {
                    let client =
                        SocketAddr::new(IpAddr::V6(addrs.source_address), addrs.source_port);
                    let server = SocketAddr::new(
                        IpAddr::V6(addrs.destination_address),
                        addrs.destination_port,
                    );
                    (client, Some(server))
                }
                ppp::v1::Addresses::Unknown => {
                    // UNKNOWN protocol - use placeholder
                    let client = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
                    (client, None)
                }
            };

            let remaining = BytesMut::from(&buf[header_end..]);

            Ok((
                ProxyInfo {
                    client_addr,
                    server_addr,
                    tls_info: None, // V1 doesn't support TLVs
                    version: ProxyVersion::V1,
                },
                remaining,
            ))
        }
        Err(e) => Err(ProxyError::InvalidHeader(format!(
            "v1 parse error: {:?}",
            e
        ))),
    }
}

/// Parse a PROXY v2 (binary) header
fn parse_v2_header(buf: &[u8], parse_tls_info: bool) -> Result<(ProxyInfo, BytesMut), ProxyError> {
    match ppp::v2::Header::try_from(buf) {
        Ok(header) => {
            let (client_addr, server_addr) = match &header.addresses {
                ppp::v2::Addresses::IPv4(addrs) => {
                    let client =
                        SocketAddr::new(IpAddr::V4(addrs.source_address), addrs.source_port);
                    let server = SocketAddr::new(
                        IpAddr::V4(addrs.destination_address),
                        addrs.destination_port,
                    );
                    (client, Some(server))
                }
                ppp::v2::Addresses::IPv6(addrs) => {
                    let client =
                        SocketAddr::new(IpAddr::V6(addrs.source_address), addrs.source_port);
                    let server = SocketAddr::new(
                        IpAddr::V6(addrs.destination_address),
                        addrs.destination_port,
                    );
                    (client, Some(server))
                }
                ppp::v2::Addresses::Unix(_) => {
                    // Unix sockets - use placeholder IP
                    let client = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
                    (client, None)
                }
                ppp::v2::Addresses::Unspecified => {
                    // LOCAL command or UNSPEC - use placeholder
                    let client = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
                    (client, None)
                }
            };

            // Parse TLS info from TLVs if requested
            let tls_info = if parse_tls_info {
                extract_tls_info(&header)
            } else {
                None
            };

            // Calculate remaining bytes (header includes 16-byte prefix + length)
            let header_len = u16::from_be_bytes([buf[14], buf[15]]) as usize;
            let total_header_len = 16 + header_len;
            let remaining = BytesMut::from(&buf[total_header_len..]);

            Ok((
                ProxyInfo {
                    client_addr,
                    server_addr,
                    tls_info,
                    version: ProxyVersion::V2,
                },
                remaining,
            ))
        }
        Err(e) => Err(ProxyError::InvalidHeader(format!(
            "v2 parse error: {:?}",
            e
        ))),
    }
}

/// Extract TLS information from PROXY v2 TLVs
fn extract_tls_info(header: &ppp::v2::Header) -> Option<ProxyTlsInfo> {
    let mut sni = None;
    let mut client_cert_cn = None;
    let mut client_cert_verified = false;

    // Iterate through TLVs
    for tlv_result in header.tlvs() {
        let tlv = match tlv_result {
            Ok(t) => t,
            Err(_) => continue,
        };

        match tlv.kind {
            // PP2_TYPE_AUTHORITY (0x02) - contains SNI
            0x02 => {
                if let Ok(s) = std::str::from_utf8(&tlv.value) {
                    sni = Some(s.to_string());
                }
            }
            // PP2_TYPE_SSL (0x20) - contains SSL sub-TLVs
            0x20 => {
                // Parse SSL sub-TLVs
                if let Some(info) = parse_ssl_tlv(&tlv.value) {
                    client_cert_cn = info.0;
                    client_cert_verified = info.1;
                }
            }
            // PP2_SUBTYPE_SSL_CN (0x22) - client cert CN as standalone TLV
            0x22 => {
                if let Ok(s) = std::str::from_utf8(&tlv.value) {
                    client_cert_cn = Some(s.to_string());
                }
            }
            _ => {}
        }
    }

    if sni.is_some() || client_cert_cn.is_some() || client_cert_verified {
        Some(ProxyTlsInfo {
            sni,
            client_cert_cn,
            client_cert_verified,
        })
    } else {
        None
    }
}

/// Parse PP2_TYPE_SSL TLV value to extract client cert CN
/// Returns (Option<CN>, client_verified)
fn parse_ssl_tlv(value: &[u8]) -> Option<(Option<String>, bool)> {
    // PP2_TYPE_SSL structure:
    // - 1 byte: client bitfield (bit 0 = PP2_CLIENT_SSL, bit 2 = PP2_CLIENT_CERT_CONN)
    // - 4 bytes: verify (0 = success, non-zero = error)
    // - remaining: sub-TLVs

    if value.len() < 5 {
        return None;
    }

    let client_flags = value[0];
    let _verify_result = u32::from_be_bytes([value[1], value[2], value[3], value[4]]);
    let client_verified = (client_flags & 0x04) != 0; // PP2_CLIENT_CERT_CONN

    let mut cn = None;

    // Parse sub-TLVs starting at offset 5
    let mut offset = 5;
    while offset + 3 <= value.len() {
        let sub_type = value[offset];
        let sub_len = u16::from_be_bytes([value[offset + 1], value[offset + 2]]) as usize;
        offset += 3;

        if offset + sub_len > value.len() {
            break;
        }

        // PP2_SUBTYPE_SSL_CN = 0x02
        if sub_type == 0x02 {
            if let Ok(s) = std::str::from_utf8(&value[offset..offset + sub_len]) {
                cn = Some(s.to_string());
            }
        }

        offset += sub_len;
    }

    Some((cn, client_verified))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_v1_signature() {
        assert_eq!(PROXY_V1_SIGNATURE, b"PROXY ");
    }

    #[test]
    fn test_proxy_v2_signature() {
        assert_eq!(PROXY_V2_SIGNATURE.len(), 12);
    }

    #[tokio::test]
    async fn test_parse_v1_tcp4() {
        let header = b"PROXY TCP4 192.168.1.1 10.0.0.1 12345 80\r\n";
        let mut cursor = std::io::Cursor::new(header.to_vec());

        let (info, remaining) = parse_proxy_header(&mut cursor, Duration::from_secs(5), false)
            .await
            .unwrap();

        assert_eq!(info.version, ProxyVersion::V1);
        assert_eq!(
            info.client_addr,
            "192.168.1.1:12345".parse::<SocketAddr>().unwrap()
        );
        assert_eq!(
            info.server_addr,
            Some("10.0.0.1:80".parse::<SocketAddr>().unwrap())
        );
        assert!(info.tls_info.is_none());
        assert!(remaining.is_empty());
    }

    #[tokio::test]
    async fn test_parse_v1_tcp6() {
        let header = b"PROXY TCP6 ::1 ::2 12345 80\r\n";
        let mut cursor = std::io::Cursor::new(header.to_vec());

        let (info, _) = parse_proxy_header(&mut cursor, Duration::from_secs(5), false)
            .await
            .unwrap();

        assert_eq!(info.version, ProxyVersion::V1);
        assert_eq!(
            info.client_addr,
            "[::1]:12345".parse::<SocketAddr>().unwrap()
        );
    }

    #[tokio::test]
    async fn test_parse_v1_unknown() {
        // UNKNOWN needs enough initial bytes (16) to be read
        let header = b"PROXY UNKNOWN  \r\n";
        let mut cursor = std::io::Cursor::new(header.to_vec());

        let (info, _) = parse_proxy_header(&mut cursor, Duration::from_secs(5), false)
            .await
            .unwrap();

        assert_eq!(info.version, ProxyVersion::V1);
        assert_eq!(info.client_addr.ip(), IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    }
}
