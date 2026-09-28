use std::fs;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SecurityError {
    #[error("Invalid URL format: {0}")]
    InvalidUrl(String),

    #[error("Unsupported URL scheme '{0}', only http and https are permitted")]
    UnsupportedScheme(String),

    #[error("Failed to resolve hostname '{0}': {1}")]
    DnsResolutionFailed(String, std::io::Error),

    #[error("SSRF Protection: Host '{0}' resolved to forbidden/private IP address: {1}")]
    ForbiddenIpAddress(String, IpAddr),
}

#[derive(Debug, Clone)]
pub struct SsrfPolicy {
    /// Allow private RFC 1918 IPs (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, IPv6 fc00::/7)
    pub allow_private_ips: bool,
    /// Allow loopback (127.0.0.0/8, ::1, localhost)
    pub allow_loopback: bool,
    /// Explicit allowed hostnames or domains (e.g. "minio.internal", "localhost")
    pub allowed_hosts: Vec<String>,
}

impl Default for SsrfPolicy {
    fn default() -> Self {
        let allow_private = std::env::var("DUON_ALLOW_PRIVATE_IPS")
            .or_else(|_| std::env::var("SSRF_ALLOW_PRIVATE_IPS"))
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let allow_loopback = std::env::var("DUON_ALLOW_LOOPBACK")
            .or_else(|_| std::env::var("SSRF_ALLOW_LOOPBACK"))
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let allowed_hosts = std::env::var("DUON_SSRF_ALLOWED_HOSTS")
            .or_else(|_| std::env::var("SSRF_ALLOWED_HOSTS"))
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        Self {
            allow_private_ips: allow_private,
            allow_loopback,
            allowed_hosts,
        }
    }
}

impl SsrfPolicy {
    /// Strict policy blocking loopback, private IPs, cloud metadata, etc.
    pub fn strict() -> Self {
        Self {
            allow_private_ips: false,
            allow_loopback: false,
            allowed_hosts: Vec::new(),
        }
    }

    /// Permissive policy allowing intranet / private IPs (suitable for on-premise / VPC deployments)
    pub fn allow_intranet() -> Self {
        Self {
            allow_private_ips: true,
            allow_loopback: false,
            allowed_hosts: Vec::new(),
        }
    }
}

/// Validates that a URL complies with default SSRF protection rules.
pub fn validate_url_ssrf(url_str: &str) -> Result<(), SecurityError> {
    validate_url_ssrf_with_policy(url_str, &SsrfPolicy::default())
}

/// Validates that a URL does not point to forbidden internal/metadata addresses according to policy.
pub fn validate_url_ssrf_with_policy(url_str: &str, policy: &SsrfPolicy) -> Result<(), SecurityError> {
    let parsed = reqwest::Url::parse(url_str)
        .map_err(|e| SecurityError::InvalidUrl(e.to_string()))?;

    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(SecurityError::UnsupportedScheme(scheme.to_string()));
    }

    let host_str = parsed
        .host_str()
        .ok_or_else(|| SecurityError::InvalidUrl("Missing host".to_string()))?;

    // Check if host is in explicit whitelist
    let lower_host = host_str.to_lowercase();
    if policy.allowed_hosts.iter().any(|h| h == &lower_host || lower_host.ends_with(&format!(".{}", h))) {
        return Ok(());
    }

    // Check if host is direct IP or resolve domain
    if let Ok(ip) = host_str.parse::<IpAddr>() {
        check_ip_security(host_str, ip, policy)?;
    } else {
        // Resolve domain to IP addresses
        let port = parsed.port_or_known_default().unwrap_or(80);
        let socket_addrs = format!("{}:{}", host_str, port)
            .to_socket_addrs()
            .map_err(|e| SecurityError::DnsResolutionFailed(host_str.to_string(), e))?;

        for addr in socket_addrs {
            check_ip_security(host_str, addr.ip(), policy)?;
        }
    }

    Ok(())
}

fn check_ip_security(host: &str, ip: IpAddr, policy: &SsrfPolicy) -> Result<(), SecurityError> {
    match ip {
        IpAddr::V4(ipv4) => {
            let octets = ipv4.octets();
            // Loopback 127.0.0.0/8
            if octets[0] == 127 && !policy.allow_loopback {
                return Err(SecurityError::ForbiddenIpAddress(host.to_string(), ip));
            }
            // Private 10.0.0.0/8
            if octets[0] == 10 && !policy.allow_private_ips {
                return Err(SecurityError::ForbiddenIpAddress(host.to_string(), ip));
            }
            // Private 172.16.0.0/12
            if octets[0] == 172 && (16..=31).contains(&octets[1]) && !policy.allow_private_ips {
                return Err(SecurityError::ForbiddenIpAddress(host.to_string(), ip));
            }
            // Private 192.168.0.0/16
            if octets[0] == 192 && octets[1] == 168 && !policy.allow_private_ips {
                return Err(SecurityError::ForbiddenIpAddress(host.to_string(), ip));
            }
            // Link-local / Cloud metadata 169.254.0.0/16 - strictly forbidden
            if octets[0] == 169 && octets[1] == 254 {
                return Err(SecurityError::ForbiddenIpAddress(host.to_string(), ip));
            }
            // Current / broadcast / multicast
            if octets[0] == 0 || octets[0] >= 224 {
                return Err(SecurityError::ForbiddenIpAddress(host.to_string(), ip));
            }
        }
        IpAddr::V6(ipv6) => {
            // Loopback ::1
            if ipv6.is_loopback() && !policy.allow_loopback {
                return Err(SecurityError::ForbiddenIpAddress(host.to_string(), ip));
            }
            // Unique local fc00::/7
            let segments = ipv6.segments();
            if (segments[0] & 0xfe00) == 0xfc00 && !policy.allow_private_ips {
                return Err(SecurityError::ForbiddenIpAddress(host.to_string(), ip));
            }
            // Link-local fe80::/10 - strictly forbidden
            if (segments[0] & 0xffc0) == 0xfe80 {
                return Err(SecurityError::ForbiddenIpAddress(host.to_string(), ip));
            }
        }
    }
    Ok(())
}

/// RAII Guard ensuring uploaded ephemeral files are deleted from disk immediately upon Drop.
pub struct EphemeralFile {
    path: PathBuf,
}

impl EphemeralFile {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Creates a temporary file from bytes that will be automatically deleted on drop.
    pub fn from_bytes(bytes: &[u8], filename_hint: Option<&str>) -> std::io::Result<Self> {
        let suffix = filename_hint
            .and_then(|f| Path::new(f).extension().and_then(|s| s.to_str()))
            .map(|ext| format!(".{}", ext))
            .unwrap_or_else(|| ".tmp".to_string());

        let temp_file = tempfile::Builder::new()
            .prefix("duon-ephemeral-")
            .suffix(&suffix)
            .tempfile()?;

        // Keep the temporary path alive so EphemeralFile controls its drop lifecycle
        let (_, path) = temp_file.keep().map_err(|e| e.error)?;
        fs::write(&path, bytes)?;

        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for EphemeralFile {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

