//! Test-only FFmpeg stand-in: copies the first `-i` input to the output path.
#![forbid(unsafe_code)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = None;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "-i" {
            if input.is_none() {
                input = args.get(index + 1).cloned();
            }
            index += 2;
            continue;
        }
        index += 1;
    }
    let (Some(input), Some(output)) = (input, args.last()) else {
        eprintln!("ffmpeg-stub: missing input or output");
        std::process::exit(2);
    };
    if let Some(parent) = std::path::Path::new(output).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::copy(&input, output) {
        eprintln!("ffmpeg-stub: {error}");
        std::process::exit(1);
    }
}
