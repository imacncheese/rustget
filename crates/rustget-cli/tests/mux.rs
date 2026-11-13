use std::process::Command;

#[test]
fn mux_command_invokes_ffmpeg_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let video = directory.path().join("video.mp4");
    let audio = directory.path().join("audio.m4a");
    let output = directory.path().join("out.mp4");
    std::fs::write(&video, b"video-bytes").unwrap();
    std::fs::write(&audio, b"audio-bytes").unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_rustget"))
        .arg("mux")
        .arg(&video)
        .arg("--audio")
        .arg(&audio)
        .arg("--output")
        .arg(&output)
        .arg("--ffmpeg")
        .arg(env!("CARGO_BIN_EXE_rustget-ffmpeg-stub"))
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(std::fs::read(output).unwrap(), b"video-bytes");
}
