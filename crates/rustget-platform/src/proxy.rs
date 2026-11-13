//! Windows Internet Settings proxy discovery without executing PAC scripts.

/// Proxies configured for the current Windows user.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemProxy {
    pub http: Option<String>,
    pub https: Option<String>,
    /// `AutoConfigURL` when present; PAC evaluation is not implemented.
    pub pac_url: Option<String>,
}

impl SystemProxy {
    #[must_use]
    pub fn preferred(&self) -> Option<&str> {
        self.https.as_deref().or(self.http.as_deref())
    }
}

/// Parse a `ProxyServer` value such as `proxy:8080` or
/// `http=p1:8080;https=p2:8443`.
#[must_use]
pub fn parse_proxy_server(value: &str) -> (Option<String>, Option<String>) {
    let value = value.trim();
    if value.is_empty() {
        return (None, None);
    }
    if !value.contains('=') {
        let proxy = normalize(value);
        return (Some(proxy.clone()), Some(proxy));
    }
    let mut http = None;
    let mut https = None;
    for entry in value.split(';') {
        let Some((scheme, address)) = entry.split_once('=') else {
            continue;
        };
        let address = normalize(address);
        match scheme.trim().to_ascii_lowercase().as_str() {
            "http" => http = Some(address),
            "https" => https = Some(address),
            "socks" | "socks5" => {
                let address = address.replacen("http://", "socks5://", 1);
                http.get_or_insert_with(|| address.clone());
                https.get_or_insert(address);
            }
            _ => {}
        }
    }
    (http, https)
}

fn normalize(value: &str) -> String {
    let value = value.trim();
    if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{value}")
    }
}

/// Read the current user's Internet Settings proxy.
#[must_use]
pub fn system_proxy() -> Option<SystemProxy> {
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
            .ok()?;
        let enabled: u32 = key.get_value("ProxyEnable").unwrap_or(0);
        let server: String = key.get_value("ProxyServer").unwrap_or_default();
        let pac_url: String = key.get_value("AutoConfigURL").unwrap_or_default();
        let (http, https) = if enabled != 0 {
            parse_proxy_server(&server)
        } else {
            (None, None)
        };
        let pac_url = (!pac_url.trim().is_empty()).then_some(pac_url);
        if http.is_none() && https.is_none() && pac_url.is_none() {
            return None;
        }
        Some(SystemProxy {
            http,
            https,
            pac_url,
        })
    }
    #[cfg(not(windows))]
    {
        let http = std::env::var("HTTP_PROXY")
            .ok()
            .or_else(|| std::env::var("http_proxy").ok())
            .map(|value| normalize(&value));
        let https = std::env::var("HTTPS_PROXY")
            .ok()
            .or_else(|| std::env::var("https_proxy").ok())
            .map(|value| normalize(&value))
            .or_else(|| http.clone());
        if http.is_none() && https.is_none() {
            return None;
        }
        Some(SystemProxy {
            http,
            https,
            pac_url: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_proxy_for_both_schemes() {
        let (http, https) = parse_proxy_server("proxy.local:8080");
        assert_eq!(http.as_deref(), Some("http://proxy.local:8080"));
        assert_eq!(https.as_deref(), Some("http://proxy.local:8080"));
    }

    #[test]
    fn parses_per_scheme_proxy() {
        let (http, https) = parse_proxy_server("http=p1:80;https=p2:443");
        assert_eq!(http.as_deref(), Some("http://p1:80"));
        assert_eq!(https.as_deref(), Some("http://p2:443"));
    }

    #[test]
    fn detects_pac_url() {
        let proxy = SystemProxy {
            pac_url: Some("https://example/proxy.pac".into()),
            ..SystemProxy::default()
        };
        assert!(proxy.preferred().is_none());
        assert!(proxy.pac_url.is_some());
    }
}
