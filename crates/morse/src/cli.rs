use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use morse::Waveform;

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Translate English letters/digits and Morse code, then create OGG audio"
)]
pub(crate) struct Cli {
    /// English/digit text or Morse code. Reads stdin when omitted.
    #[arg(value_name = "TEXT", trailing_var_arg = true)]
    pub(crate) input: Vec<String>,

    /// Tone frequency in Hz
    #[arg(short = 'f', long = "frequency", default_value_t = 700.0, value_parser = positive_frequency)]
    pub(crate) frequency: f32,

    /// Morse time unit in milliseconds
    #[arg(short = 'u', long = "unit", default_value_t = 80, value_parser = positive_unit)]
    pub(crate) unit: u32,

    /// Sound timbre: sine, square, triangle, sawtooth, or piano
    #[arg(short = 'w', long = "wave", visible_alias = "timbre", default_value = "sine", value_parser = parse_waveform)]
    pub(crate) waveform: Waveform,

    /// OGG output path
    #[arg(short = 'o', long = "output")]
    pub(crate) output: Option<PathBuf>,

    /// Examine encoding or decoding; accepts e/encode or d/decode
    #[arg(short = 'e', long = "exam", value_name = "MODE", ignore_case = true)]
    pub(crate) exam: Option<ExamDirection>,

    /// Decode Morse as you key it in the focused terminal
    #[arg(short = 'r', long = "rt", visible_alias = "realtime", conflicts_with_all = ["input", "exam", "frequency", "waveform", "output"])]
    pub(crate) realtime: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum ExamDirection {
    #[value(alias = "e")]
    Encode,
    #[value(alias = "d")]
    Decode,
}

fn positive_frequency(value: &str) -> Result<f32, String> {
    let parsed: f32 = value
        .parse()
        .map_err(|_| "frequency must be a number".to_string())?;
    if parsed.is_finite() && (20.0..=20_000.0).contains(&parsed) {
        Ok(parsed)
    } else {
        Err("frequency must be between 20 and 20000 Hz".into())
    }
}

fn positive_unit(value: &str) -> Result<u32, String> {
    let parsed: u32 = value
        .parse()
        .map_err(|_| "unit must be a positive integer".to_string())?;
    if (1..=10_000).contains(&parsed) {
        Ok(parsed)
    } else {
        Err("unit must be between 1 and 10000 ms".into())
    }
}

fn parse_waveform(value: &str) -> Result<Waveform, String> {
    Waveform::parse(value)
}
