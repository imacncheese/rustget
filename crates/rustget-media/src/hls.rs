use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum HlsError {
    #[error("invalid playlist URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("empty playlist")]
    Empty,
    #[error("invalid attribute: {0}")]
    Attribute(String),
    #[error("invalid resolution: {0}")]
    Resolution(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HlsVariant {
    pub url: String,
    pub bandwidth: u64,
    pub average_bandwidth: Option<u64>,
    pub resolution: Option<(u32, u32)>,
    pub codecs: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub url: String,
    pub duration_ms: u64,
    pub byte_range: Option<(u64, u64)>,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaPlaylist {
    pub target_duration_secs: u64,
    pub segments: Vec<Segment>,
    pub init_map: Option<String>,
    pub encrypted: bool,
    pub ended: bool,
    pub key_uri: Option<String>,
    /// Hex IV from `#EXT-X-KEY:IV=0x...`, if present.
    pub key_iv: Option<String>,
    pub key_format: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HlsPlaylist {
    Master(Vec<HlsVariant>),
    Media(MediaPlaylist),
}

/// Parse a master or media playlist. Relative URIs resolve against `base`.
pub fn parse_hls(text: &str, base: &Url) -> Result<HlsPlaylist, HlsError> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return Err(HlsError::Empty);
    }
    if lines[0] != "#EXTM3U" {
        return Err(HlsError::Attribute("missing #EXTM3U".into()));
    }
    if lines
        .iter()
        .any(|line| line.starts_with("#EXT-X-STREAM-INF"))
    {
        return parse_master(&lines, base);
    }
    parse_media(&lines, base)
}

fn parse_master(lines: &[&str], base: &Url) -> Result<HlsPlaylist, HlsError> {
    let mut variants = Vec::new();
    let mut pending: Option<HashMap<String, String>> = None;
    for line in lines {
        if let Some(attributes) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending = Some(parse_attributes(attributes));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some(attributes) = pending.take() {
            let bandwidth = attributes
                .get("BANDWIDTH")
                .ok_or_else(|| HlsError::Attribute("variant missing BANDWIDTH".into()))?
                .parse()
                .map_err(|_| HlsError::Attribute("invalid BANDWIDTH".into()))?;
            let resolution = attributes
                .get("RESOLUTION")
                .map(|value| parse_resolution(value))
                .transpose()?;
            variants.push(HlsVariant {
                url: base.join(line)?.to_string(),
                bandwidth,
                average_bandwidth: attributes
                    .get("AVERAGE-BANDWIDTH")
                    .and_then(|value| value.parse().ok()),
                resolution,
                codecs: attributes.get("CODECS").cloned(),
            });
        }
    }
    if variants.is_empty() {
        return Err(HlsError::Empty);
    }
    variants.sort_by_key(|variant| variant.bandwidth);
    Ok(HlsPlaylist::Master(variants))
}

fn parse_media(lines: &[&str], base: &Url) -> Result<HlsPlaylist, HlsError> {
    let mut target_duration_secs = 0_u64;
    let mut duration_ms = 0_u64;
    let mut init_map = None;
    let mut encrypted = false;
    let mut key_uri = None;
    let mut key_iv = None;
    let mut key_format = None;
    let mut ended = false;
    let mut sequence = 0_u64;
    let mut segments = Vec::new();
    for line in lines {
        if let Some(value) = line.strip_prefix("#EXT-X-TARGETDURATION:") {
            target_duration_secs = value.trim().parse().unwrap_or(0);
        } else if let Some(value) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            sequence = value.trim().parse().unwrap_or(0);
        } else if let Some(value) = line.strip_prefix("#EXT-X-MAP:") {
            let attributes = parse_attributes(value);
            if let Some(uri) = attributes.get("URI") {
                init_map = Some(base.join(uri)?.to_string());
            }
        } else if let Some(value) = line.strip_prefix("#EXT-X-KEY:") {
            let attributes = parse_attributes(value);
            if attributes
                .get("METHOD")
                .is_none_or(|method| method != "NONE")
            {
                encrypted = true;
                key_uri = attributes
                    .get("URI")
                    .map(|uri| base.join(uri))
                    .transpose()?
                    .map(|url| url.to_string());
                key_iv = attributes.get("IV").cloned();
                key_format = attributes.get("KEYFORMAT").cloned();
            }
        } else if let Some(value) = line.strip_prefix("#EXTINF:") {
            let duration = value.split(',').next().unwrap_or("0");
            duration_ms = (duration.trim().parse::<f64>().unwrap_or(0.0) * 1000.0) as u64;
        } else if line.starts_with("#EXT-X-ENDLIST") {
            ended = true;
        } else if line.starts_with('#') {
            // Comment/unknown tag.
        } else if duration_ms > 0 {
            segments.push(Segment {
                url: base.join(line)?.to_string(),
                duration_ms,
                byte_range: None,
                sequence,
            });
            sequence += 1;
            duration_ms = 0;
        }
    }
    if segments.is_empty() {
        return Err(HlsError::Empty);
    }
    Ok(HlsPlaylist::Media(MediaPlaylist {
        target_duration_secs,
        segments,
        init_map,
        encrypted,
        ended,
        key_uri,
        key_iv,
        key_format,
    }))
}

fn parse_resolution(value: &str) -> Result<(u32, u32), HlsError> {
    let (width, height) = value
        .split_once('x')
        .ok_or_else(|| HlsError::Resolution(value.into()))?;
    let width = width
        .parse()
        .map_err(|_| HlsError::Resolution(value.into()))?;
    let height = height
        .parse()
        .map_err(|_| HlsError::Resolution(value.into()))?;
    Ok((width, height))
}

fn parse_attributes(value: &str) -> HashMap<String, String> {
    let mut attributes = HashMap::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut parts = Vec::new();
    for ch in value.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                current.push(ch);
            }
            ',' if !quoted => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    for part in parts {
        if let Some((key, value)) = part.split_once('=') {
            attributes.insert(
                key.trim().to_ascii_uppercase(),
                value.trim().trim_matches('"').to_string(),
            );
        }
    }
    attributes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_master_variants_and_sorts_by_bandwidth() {
        let text = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,CODECS="avc1.640028,mp4a.40.2"
hd.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360
low.m3u8
"#;
        let base = Url::parse("https://cdn.example.test/media/master.m3u8").unwrap();
        let HlsPlaylist::Master(variants) = parse_hls(text, &base).unwrap() else {
            panic!("expected master");
        };
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].url, "https://cdn.example.test/media/low.m3u8");
        assert_eq!(variants[1].resolution, Some((1920, 1080)));
    }

    #[test]
    fn parses_media_playlist_with_init_and_encryption() {
        let text = r#"#EXTM3U
#EXT-X-VERSION:7
#EXT-X-TARGETDURATION:6
#EXT-X-MAP:URI="init.mp4"
#EXT-X-KEY:METHOD=AES-128,URI="key.bin",IV=0x000102030405060708090a0b0c0d0e0f,KEYFORMAT="identity"
#EXTINF:5.5,
seg0.m4s
#EXTINF:5.5,
seg1.m4s
#EXT-X-ENDLIST
"#;
        let base = Url::parse("https://cdn.example.test/v/media.m3u8").unwrap();
        let HlsPlaylist::Media(playlist) = parse_hls(text, &base).unwrap() else {
            panic!("expected media");
        };
        assert_eq!(playlist.segments.len(), 2);
        assert_eq!(playlist.target_duration_secs, 6);
        assert!(playlist.encrypted);
        assert_eq!(
            playlist.init_map.as_deref(),
            Some("https://cdn.example.test/v/init.mp4")
        );
        assert_eq!(playlist.segments[1].duration_ms, 5500);
        assert_eq!(
            playlist.key_iv.as_deref(),
            Some("0x000102030405060708090a0b0c0d0e0f")
        );
        assert_eq!(playlist.key_format.as_deref(), Some("identity"));
        assert!(playlist.ended);
    }
}
