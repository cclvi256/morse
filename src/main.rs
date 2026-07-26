mod audio_output;
mod cli;
mod exam;
mod input;
mod realtime;

use clap::Parser;
use morse::{InputKind, translate};

use crate::{
    audio_output::{SAMPLE_RATE, encode_ogg},
    cli::Cli,
    exam::run_exam,
    input::{default_output_path, read_input},
    realtime::run_realtime,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("morse: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    if cli.realtime {
        return run_realtime(cli.unit);
    }
    if let Some(direction) = cli.exam {
        return run_exam(direction, &cli.input);
    }

    let input = read_input(&cli.input)?;
    let (kind, translation, audio_morse) = translate(&input)?;
    let output = cli.output.unwrap_or_else(default_output_path);
    println!(
        "{}",
        match kind {
            InputKind::Text => format!("Morse: {translation}"),
            InputKind::Morse => format!("Text: {translation}"),
        }
    );

    let samples = morse::synthesize(
        &audio_morse,
        cli.frequency,
        cli.unit,
        cli.waveform,
        SAMPLE_RATE,
    );
    encode_ogg(&samples, SAMPLE_RATE, &output)?;
    println!("Audio: {}", output.display());
    Ok(())
}
