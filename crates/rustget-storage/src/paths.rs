use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;

#[derive(Debug, Clone)]
pub struct DownloadPaths {
    pub destination: PathBuf,
    pub part: PathBuf,
    pub journal: PathBuf,
}

impl DownloadPaths {
    #[must_use]
    pub fn new(destination: impl AsRef<Path>) -> Self {
        let destination = destination.as_ref().to_path_buf();
        let mut part = destination.as_os_str().to_os_string();
        part.push(".part");
        let mut journal = destination.as_os_str().to_os_string();
        journal.push(".rustget");
        Self {
            destination,
            part: PathBuf::from(part),
            journal: PathBuf::from(journal),
        }
    }
}

/// Resolve a Content-Disposition filename, including RFC 5987 `filename*`.
#[must_use]
pub fn content_disposition_filename(value: &str) -> Option<String> {
    let mut plain = None;
    let mut extended = None;
    for part in split_parameters(value) {
        let Some((name, raw)) = part.split_once('=') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let raw = raw.trim();
        if name == "filename" {
            plain = Some(unquote(raw).to_string());
        } else if name == "filename*" {
            extended = parse_extended(unquote(raw));
        }
    }
    extended.or(plain).filter(|s| !s.is_empty())
}

fn split_parameters(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        match ch {
            '\\' if quoted && !escaped => escaped = true,
            '"' if !escaped => quoted = !quoted,
            ';' if !quoted => {
                parts.push(value[start..index].trim());
                start = index + 1;
            }
            _ => escaped = false,
        }
    }
    parts.push(value[start..].trim());
    parts
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

fn parse_extended(value: &str) -> Option<String> {
    let (charset, rest) = value.split_once('\'')?;
    let (_language, encoded) = rest.split_once('\'')?;
    if !charset.eq_ignore_ascii_case("utf-8") && !charset.eq_ignore_ascii_case("iso-8859-1") {
        return None;
    }
    percent_decode_str(encoded)
        .decode_utf8()
        .ok()
        .map(std::borrow::Cow::into_owned)
}

#[must_use]
pub fn filename_from_url(url: &url::Url) -> Option<String> {
    let last = url.path_segments()?.rev().find(|s| !s.is_empty())?;
    let decoded = percent_decode_str(last).decode_utf8().ok()?;
    Some(decoded.into_owned())
}

/// Make an untrusted remote filename safe on Windows, macOS and Linux while
/// preserving the extension. Path separators are never retained.
#[must_use]
pub fn sanitize_filename(input: &str) -> String {
    let base = input
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(input)
        .trim()
        .trim_matches('.');
    let mut cleaned: String = base
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            {
                '_'
            } else {
                ch
            }
        })
        .collect();
    cleaned = cleaned.trim_end_matches([' ', '.']).to_string();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return "download".into();
    }
    let reserved = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = cleaned.split('.').next().unwrap_or(&cleaned);
    if reserved
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        cleaned = format!("_{cleaned}");
    }
    truncate_preserving_extension(&cleaned, 180)
}

fn truncate_preserving_extension(name: &str, max_chars: usize) -> String {
    if name.chars().count() <= max_chars {
        return name.to_string();
    }
    let path = Path::new(name);
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    let suffix = if extension.is_empty() {
        String::new()
    } else {
        format!(".{extension}")
    };
    let room = max_chars.saturating_sub(suffix.chars().count());
    let short: String = stem.chars().take(room).collect();
    format!("{short}{suffix}")
}

/// Return a path that does not yet exist, adding ` (n)` before the extension.
#[must_use]
pub fn unique_destination(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if !path.exists() && !sidecar_exists(path, ".part") {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    let extension = path.extension().and_then(|e| e.to_str());
    for index in 1..10_000 {
        let name = match extension {
            Some(ext) => format!("{stem} ({index}).{ext}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() && !sidecar_exists(&candidate, ".part") {
            return candidate;
        }
    }
    path.to_path_buf()
}

fn sidecar_exists(path: &Path, suffix: &str) -> bool {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(suffix);
    Path::new(&sidecar).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_rfc5987_filenames() {
        assert_eq!(
            content_disposition_filename("attachment; filename=\"report.pdf\""),
            Some("report.pdf".into())
        );
        assert_eq!(
            content_disposition_filename("attachment; filename*=UTF-8''na%C3%AFve%20file.txt"),
            Some("naïve file.txt".into())
        );
        assert_eq!(
            content_disposition_filename(
                "inline; filename=fallback.bin; filename*=UTF-8''better.bin"
            ),
            Some("better.bin".into())
        );
    }

    #[test]
    fn strips_path_traversal() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("..\\..\\Windows\\win.ini"), "win.ini");
        assert_eq!(sanitize_filename(".."), "download");
        assert_eq!(sanitize_filename(""), "download");
    }

    #[test]
    fn handles_windows_reserved_names_and_illegal_chars() {
        assert_eq!(sanitize_filename("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_filename("a:b?c*.txt"), "a_b_c_.txt");
        assert_eq!(sanitize_filename("trailing. . ."), "trailing");
    }

    #[test]
    fn truncates_long_names_but_keeps_extension() {
        let long = format!("{}.zip", "a".repeat(500));
        let safe = sanitize_filename(&long);
        assert!(safe.chars().count() <= 180);
        assert!(safe.ends_with(".zip"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_case_collisions_get_a_unique_destination() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("FILE.BIN");
        std::fs::write(&existing, b"x").unwrap();
        let unique = unique_destination(dir.path().join("file.bin"));
        assert_ne!(unique, existing);
        assert!(!unique.exists());
    }

    #[test]
    fn urls_decode_to_filenames() {
        let url = url::Url::parse("https://example.test/a/my%20file.zip?x=1").unwrap();
        assert_eq!(filename_from_url(&url), Some("my file.zip".into()));
    }

    #[test]
    fn unique_destination_uses_sidecar_presence() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("file.bin");
        std::fs::write(&target, b"x").unwrap();
        let unique = unique_destination(&target);
        assert_eq!(unique.file_name().unwrap(), "file (1).bin");
    }
}
