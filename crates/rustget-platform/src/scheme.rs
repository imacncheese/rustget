//! URL-scheme and file-association registration.
#![forbid(unsafe_code)]

use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemeRequest {
    pub url: String,
    pub category: Option<String>,
    pub tags: Vec<String>,
}

pub fn parse_scheme_request(raw: &str) -> Result<SchemeRequest, String> {
    let parsed = url::Url::parse(raw).map_err(|error| format!("invalid rustget URL: {error}"))?;
    if parsed.scheme() != "rustget" {
        return Err(format!("unsupported scheme: {}", parsed.scheme()));
    }
    let action = parsed
        .host_str()
        .or_else(|| {
            parsed
                .path_segments()
                .and_then(|mut segments| segments.next())
        })
        .unwrap_or_default();
    if action != "add" {
        return Err(format!("unsupported rustget action: {action}"));
    }
    let mut url = None;
    let mut category = None;
    let mut tags = Vec::new();
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "url" => url = Some(value.into_owned()),
            "category" if !value.is_empty() => category = Some(value.into_owned()),
            "tags" => {
                tags = value
                    .split(',')
                    .map(str::trim)
                    .filter(|tag| !tag.is_empty())
                    .map(str::to_string)
                    .collect()
            }
            _ => {}
        }
    }
    let url = url.ok_or_else(|| "rustget URL is missing the url parameter".to_string())?;
    url::Url::parse(&url).map_err(|error| format!("invalid download URL: {error}"))?;
    Ok(SchemeRequest {
        url,
        category,
        tags,
    })
}

#[cfg(any(windows, target_os = "linux"))]
fn run(program: &str, args: &[String]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run {program}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(windows)]
fn reg(args: &[String]) -> Result<(), String> {
    run("reg", args)
}

#[cfg(windows)]
pub fn install_scheme(executable: &Path, scheme: &str) -> Result<(), String> {
    let root = format!("HKCU\\Software\\Classes\\{scheme}");
    reg(&[
        root.clone(),
        "/ve".into(),
        "/d".into(),
        format!("URL:RustGet {scheme} Protocol"),
        "/f".into(),
    ])?;
    reg(&[
        root.clone(),
        "/v".into(),
        "URL Protocol".into(),
        "/d".into(),
        String::new(),
        "/f".into(),
    ])?;
    reg(&[
        format!("{root}\\DefaultIcon"),
        "/ve".into(),
        "/d".into(),
        format!("{},0", executable.display()),
        "/f".into(),
    ])?;
    let command = format!("\"{}\" handle-url \"%1\"", executable.display());
    reg(&[
        format!("{root}\\shell\\open\\command"),
        "/ve".into(),
        "/d".into(),
        command,
        "/f".into(),
    ])
}

#[cfg(windows)]
pub fn remove_scheme(scheme: &str) -> Result<(), String> {
    reg(&[format!("HKCU\\Software\\Classes\\{scheme}"), "/f".into()])
}

#[cfg(windows)]
pub fn scheme_installed(scheme: &str) -> Result<bool, String> {
    let status = Command::new("reg")
        .args(["query", &format!("HKCU\\Software\\Classes\\{scheme}")])
        .status()
        .map_err(|error| format!("cannot run reg: {error}"))?;
    Ok(status.success())
}

#[cfg(windows)]
pub fn install_association(executable: &Path, extension: &str) -> Result<(), String> {
    let extension = normalize_extension(extension)?;
    let prog_id = format!("RustGet{extension}");
    let root = format!("HKCU\\Software\\Classes\\{extension}");
    reg(&[
        root.clone(),
        "/ve".into(),
        "/d".into(),
        prog_id.clone(),
        "/f".into(),
    ])?;
    let prog = format!("HKCU\\Software\\Classes\\{prog_id}");
    reg(&[
        prog.clone(),
        "/ve".into(),
        "/d".into(),
        "RustGet download".into(),
        "/f".into(),
    ])?;
    reg(&[
        format!("{prog}\\DefaultIcon"),
        "/ve".into(),
        "/d".into(),
        format!("{},0", executable.display()),
        "/f".into(),
    ])?;
    let command = format!("\"{}\" add \"%1\"", executable.display());
    reg(&[
        format!("{prog}\\shell\\open\\command"),
        "/ve".into(),
        "/d".into(),
        command,
        "/f".into(),
    ])
}

#[cfg(windows)]
pub fn remove_association(extension: &str) -> Result<(), String> {
    let extension = normalize_extension(extension)?;
    reg(&[format!("HKCU\\Software\\Classes\\{extension}"), "/f".into()])?;
    let _ = reg(&[
        format!("HKCU\\Software\\Classes\\RustGet{extension}"),
        "/f".into(),
    ]);
    Ok(())
}

#[cfg(windows)]
pub fn association_installed(extension: &str) -> Result<bool, String> {
    let extension = normalize_extension(extension)?;
    let status = Command::new("reg")
        .args(["query", &format!("HKCU\\Software\\Classes\\{extension}")])
        .status()
        .map_err(|error| format!("cannot run reg: {error}"))?;
    Ok(status.success())
}

#[cfg(target_os = "linux")]
pub fn install_scheme(executable: &Path, scheme: &str) -> Result<(), String> {
    let directory = dirs_desktop()?;
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join("rustget-handler.desktop");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=RustGet\nExec=\"{}\" handle-url %u\nMimeType=x-scheme-handler/{scheme};\nNoDisplay=true\n",
        executable.display()
    );
    std::fs::write(&path, entry).map_err(|error| error.to_string())?;
    run(
        "xdg-mime",
        &[
            "default".into(),
            "rustget-handler.desktop".into(),
            format!("x-scheme-handler/{scheme}"),
        ],
    )
}

#[cfg(target_os = "linux")]
pub fn remove_scheme(_scheme: &str) -> Result<(), String> {
    let path = dirs_desktop()?.join("rustget-handler.desktop");
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(target_os = "linux")]
pub fn scheme_installed(scheme: &str) -> Result<bool, String> {
    let output = Command::new("xdg-mime")
        .args(["query", "default", &format!("x-scheme-handler/{scheme}")])
        .output()
        .map_err(|error| format!("cannot run xdg-mime: {error}"))?;
    Ok(output.status.success()
        && String::from_utf8_lossy(&output.stdout).trim() == "rustget-handler.desktop")
}

#[cfg(target_os = "linux")]
pub fn install_association(executable: &Path, extension: &str) -> Result<(), String> {
    let extension = normalize_extension(extension)?;
    let mime = if extension == ".zip" {
        "application/zip"
    } else {
        "application/octet-stream"
    };
    let directory = dirs_desktop()?;
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join("rustget-handler.desktop");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=RustGet\nExec=\"{}\" add %f\nMimeType={mime};\nNoDisplay=true\n",
        executable.display()
    );
    std::fs::write(&path, entry).map_err(|error| error.to_string())?;
    run(
        "xdg-mime",
        &[
            "default".into(),
            "rustget-handler.desktop".into(),
            mime.into(),
        ],
    )
}

#[cfg(target_os = "linux")]
pub fn remove_association(_extension: &str) -> Result<(), String> {
    remove_scheme("rustget")
}

#[cfg(target_os = "linux")]
pub fn association_installed(extension: &str) -> Result<bool, String> {
    let extension = normalize_extension(extension)?;
    let mime = if extension == ".zip" {
        "application/zip"
    } else {
        "application/octet-stream"
    };
    let output = Command::new("xdg-mime")
        .args(["query", "default", mime])
        .output()
        .map_err(|error| format!("cannot run xdg-mime: {error}"))?;
    Ok(output.status.success()
        && String::from_utf8_lossy(&output.stdout).trim() == "rustget-handler.desktop")
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn install_scheme(_executable: &Path, _scheme: &str) -> Result<(), String> {
    Err("URL-scheme registration is not implemented on this platform".into())
}
#[cfg(not(any(windows, target_os = "linux")))]
pub fn remove_scheme(_scheme: &str) -> Result<(), String> {
    Err("URL-scheme registration is not implemented on this platform".into())
}
#[cfg(not(any(windows, target_os = "linux")))]
pub fn scheme_installed(_scheme: &str) -> Result<bool, String> {
    Err("URL-scheme registration is not implemented on this platform".into())
}
#[cfg(not(any(windows, target_os = "linux")))]
pub fn install_association(_executable: &Path, _extension: &str) -> Result<(), String> {
    Err("file association is not implemented on this platform".into())
}
#[cfg(not(any(windows, target_os = "linux")))]
pub fn remove_association(_extension: &str) -> Result<(), String> {
    Err("file association is not implemented on this platform".into())
}
#[cfg(not(any(windows, target_os = "linux")))]
pub fn association_installed(_extension: &str) -> Result<bool, String> {
    Err("file association is not implemented on this platform".into())
}

fn normalize_extension(extension: &str) -> Result<String, String> {
    let extension = extension.trim();
    if extension.is_empty() || extension.contains(['/', '\\', '.']) {
        return Err(format!("invalid extension: {extension}"));
    }
    Ok(format!(".{extension}"))
}

#[cfg(target_os = "linux")]
fn dirs_desktop() -> Result<std::path::PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
    Ok(std::path::PathBuf::from(home).join(".local/share/applications"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rustget_add_urls() {
        let request = parse_scheme_request(
            "rustget://add?url=https%3A%2F%2Fexample.test%2Ffile.bin&category=Video&tags=one,two",
        )
        .unwrap();
        assert_eq!(request.url, "https://example.test/file.bin");
        assert_eq!(request.category.as_deref(), Some("Video"));
        assert_eq!(request.tags, ["one", "two"]);
        assert!(parse_scheme_request("rustget://other?url=https://example.test").is_err());
        assert!(parse_scheme_request("rustget://add").is_err());
        assert!(parse_scheme_request("https://example.test").is_err());
    }

    #[test]
    fn normalizes_extensions() {
        assert_eq!(normalize_extension("zip").unwrap(), ".zip");
        assert_eq!(
            normalize_extension(".zip").unwrap_err(),
            "invalid extension: .zip"
        );
        assert!(normalize_extension("../evil").is_err());
    }
}
