use std::{
    fs::{self, File},
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

pub(crate) const SAMPLE_RATE: u32 = 44_100;

pub(crate) fn encode_ogg(samples: &[f32], sample_rate: u32, output: &Path) -> Result<(), String> {
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|e| format!("could not create directory '{}': {e}", parent.display()))?;
    }
    let output_file = File::create(output)
        .map_err(|e| format!("could not create '{}': {e}", output.display()))?;
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "f32le",
            "-ar",
            &sample_rate.to_string(),
            "-ac",
            "1",
            "-i",
            "pipe:0",
            "-c:a",
            "libvorbis",
            "-q:a",
            "5",
            "-f",
            "ogg",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::from(output_file))
        .spawn()
        .map_err(|e| format!("could not start ffmpeg; install ffmpeg to create OGG audio: {e}"))?;
    let stdin = child.stdin.as_mut().ok_or("could not open ffmpeg input")?;
    for sample in samples {
        stdin
            .write_all(&sample.to_le_bytes())
            .map_err(|e| format!("could not send audio to ffmpeg: {e}"))?;
    }
    let status = child
        .wait()
        .map_err(|e| format!("could not wait for ffmpeg: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg failed with status {status}"))
    }
}
