//! Network whitelisting utilities.
//!
//! Provides pattern matching for hostnames and network rules.

use std::net::IpAddr;

/// Network whitelist matcher.
#[derive(Clone, Debug, Default)]
pub struct NetworkWhitelist {
    rules: Vec<WhitelistRule>,
}

/// A single whitelist rule.
#[derive(Clone, Debug)]
pub struct WhitelistRule {
    pub host: HostMatcher,
    pub port: Option<u16>,
}

/// Host matcher supporting wildcards.
#[derive(Clone, Debug, PartialEq)]
pub enum HostMatcher {
    /// Match any host.
    Any,
    /// Exact hostname match.
    Exact(String),
    /// Wildcard match (*.example.com).
    Wildcard { suffix: String },
    /// IP address match (with optional CIDR).
    Ip { addr: IpAddr, prefix: u8 },
}

impl NetworkWhitelist {
    /// Create an empty whitelist.
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a rule to the whitelist.
    pub fn add_rule(&mut self, rule: &str) -> Result<(), String> {
        let parsed = Self::parse_rule(rule)?;
        self.rules.push(parsed);
        Ok(())
    }

    /// Check if a host:port combination is allowed.
    pub fn is_allowed(&self, host: &str, port: Option<u16>) -> bool {
        for rule in &self.rules {
            if rule.matches(host, port) {
                return true;
            }
        }
        false
    }

    /// Parse a rule string like "localhost:*" or "*.npmjs.org:443".
    fn parse_rule(rule: &str) -> Result<WhitelistRule, String> {
        let parts: Vec<&str> = rule.rsplitn(2, ':').collect();
        let (host_part, port_part) = if parts.len() == 2 {
            (parts[1], Some(parts[0]))
        } else {
            (rule, None)
        };

        let port = match port_part {
            Some("*") | None => None,
            Some(p) => Some(
                p.parse::<u16>()
                    .map_err(|e| format!("Invalid port: {}", e))?,
            ),
        };

        let host = Self::parse_host(host_part)?;

        Ok(WhitelistRule {
            host,
            port: Some(port).flatten(),
        })
    }

    /// Parse a host pattern.
    fn parse_host(pattern: &str) -> Result<HostMatcher, String> {
        if pattern == "*" {
            return Ok(HostMatcher::Any);
        }

        // Check for wildcard (*.example.com)
        if pattern.starts_with("*.") {
            let suffix = pattern[1..].to_string(); // Keep the dot
            return Ok(HostMatcher::Wildcard { suffix });
        }

        // Try parsing as IP address
        if let Ok(addr) = pattern.parse::<IpAddr>() {
            return Ok(HostMatcher::Ip { addr, prefix: 128 });
        }

        // Try parsing as CIDR
        if pattern.contains('/') {
            let parts: Vec<&str> = pattern.split('/').collect();
            if parts.len() == 2 {
                let addr = parts[0]
                    .parse::<IpAddr>()
                    .map_err(|e| format!("Invalid IP: {}", e))?;
                let prefix = parts[1]
                    .parse::<u8>()
                    .map_err(|e| format!("Invalid prefix: {}", e))?;
                return Ok(HostMatcher::Ip { addr, prefix });
            }
        }

        // Exact match
        Ok(HostMatcher::Exact(pattern.to_string()))
    }
}

impl WhitelistRule {
    /// Check if this rule matches the given host:port.
    pub fn matches(&self, host: &str, port: Option<u16>) -> bool {
        // Check port first
        if let Some(rule_port) = self.port {
            if port != Some(rule_port) {
                return false;
            }
        }

        // Check host
        self.host.matches(host)
    }
}

impl HostMatcher {
    /// Check if this matcher matches the given host.
    pub fn matches(&self, host: &str) -> bool {
        match self {
            HostMatcher::Any => true,
            HostMatcher::Exact(pattern) => host.eq_ignore_ascii_case(pattern),
            HostMatcher::Wildcard { suffix } => {
                host.eq_ignore_ascii_case(&suffix[1..]) || // exact match without *
                host.to_lowercase().ends_with(&suffix.to_lowercase())
            }
            HostMatcher::Ip { addr, prefix } => {
                // Try to parse host as IP and check if it matches
                if let Ok(host_ip) = host.parse::<IpAddr>() {
                    Self::ip_matches(host_ip, *addr, *prefix)
                } else {
                    false
                }
            }
        }
    }

    /// Check if an IP matches another IP with prefix.
    fn ip_matches(host: IpAddr, pattern: IpAddr, prefix: u8) -> bool {
        match (host, pattern) {
            (IpAddr::V4(h), IpAddr::V4(p)) => {
                if prefix > 32 {
                    return false;
                }
                let mask = if prefix == 0 {
                    0u32
                } else {
                    !0u32 << (32 - prefix)
                };
                let h_bits = u32::from(h) & mask;
                let p_bits = u32::from(p) & mask;
                h_bits == p_bits
            }
            (IpAddr::V6(h), IpAddr::V6(p)) => {
                if prefix > 128 {
                    return false;
                }
                let mask = if prefix == 0 {
                    0u128
                } else {
                    !0u128 << (128 - prefix)
                };
                let h_bits = u128::from(h) & mask;
                let p_bits = u128::from(p) & mask;
                h_bits == p_bits
            }
            _ => false, // IPv4 and IPv6 don't match
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::Ipv4Addr;
    use std::net::Ipv6Addr;
    #[test]
    fn test_any_matcher() {
        let matcher = HostMatcher::Any;
        assert!(matcher.matches("any.host.com"));
        assert!(matcher.matches("localhost"));
    }

    #[test]
    fn test_exact_matcher() {
        let matcher = HostMatcher::Exact("github.com".to_string());
        assert!(matcher.matches("github.com"));
        assert!(matcher.matches("GITHUB.COM")); // case insensitive
        assert!(!matcher.matches("api.github.com"));
    }

    #[test]
    fn test_wildcard_matcher() {
        let matcher = HostMatcher::Wildcard {
            suffix: ".npmjs.org".to_string(),
        };
        assert!(matcher.matches("registry.npmjs.org"));
        assert!(matcher.matches("api.npmjs.org"));
        assert!(matcher.matches("npmjs.org")); // matches the base domain too
        assert!(!matcher.matches("npmjs.com"));
    }

    #[test]
    fn test_ip_matcher() {
        let matcher = HostMatcher::Ip {
            addr: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 0)),
            prefix: 24,
        };
        assert!(matcher.matches("192.168.1.0"));
        assert!(matcher.matches("192.168.1.100"));
        assert!(matcher.matches("192.168.1.255"));
        assert!(!matcher.matches("192.168.2.1"));
    }

    #[test]
    fn test_network_whitelist() {
        let mut whitelist = NetworkWhitelist::new();
        whitelist.add_rule("localhost:*").unwrap();
        whitelist.add_rule("*.npmjs.org:443").unwrap();
        whitelist.add_rule("github.com").unwrap();

        // localhost any port
        assert!(whitelist.is_allowed("localhost", Some(3000)));
        assert!(whitelist.is_allowed("localhost", Some(8080)));

        // npmjs subdomain port 443 only
        assert!(whitelist.is_allowed("registry.npmjs.org", Some(443)));
        assert!(!whitelist.is_allowed("registry.npmjs.org", Some(80)));

        // github any port (no port specified in rule)
        assert!(whitelist.is_allowed("github.com", Some(443)));
        assert!(whitelist.is_allowed("github.com", Some(80)));

        // not whitelisted
        assert!(!whitelist.is_allowed("google.com", Some(443)));
    }

    #[test]
    fn test_parse_rule() {
        let rule = NetworkWhitelist::parse_rule("*.example.com:443").unwrap();
        assert!(matches!(rule.host, HostMatcher::Wildcard { .. }));
        assert_eq!(rule.port, Some(443));

        let rule = NetworkWhitelist::parse_rule("localhost:*").unwrap();
        assert!(matches!(rule.host, HostMatcher::Exact(_)));
        assert_eq!(rule.port, None);

        let rule = NetworkWhitelist::parse_rule("192.168.1.0/24").unwrap();
        assert!(matches!(rule.host, HostMatcher::Ip { .. }));
        assert_eq!(rule.port, None);
    }

    #[test]
    fn test_ipv6_matching() {
        let matcher = HostMatcher::Ip {
            addr: IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 0)),
            prefix: 8,
        };
        assert!(matcher.matches("fd00::1"));
        assert!(matcher.matches("fdff::ffff"));
        assert!(!matcher.matches("fe00::1"));
    }
}
