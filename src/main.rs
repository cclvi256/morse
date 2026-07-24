use std::fs::{self, File};
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, ValueEnum};
use morse::{
    InputKind, Waveform, alphanumeric_content, decode_morse, encode_text, morse_signal_count,
    translate,
};

const SAMPLE_RATE: u32 = 44_100;

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Translate English letters/digits and Morse code, then create OGG audio"
)]
struct Cli {
    /// English/digit text or Morse code. Reads stdin when omitted.
    #[arg(value_name = "TEXT", trailing_var_arg = true)]
    input: Vec<String>,

    /// Tone frequency in Hz
    #[arg(short = 'f', long = "frequency", default_value_t = 700.0, value_parser = positive_frequency)]
    frequency: f32,

    /// Morse time unit in milliseconds
    #[arg(short = 'u', long = "unit", default_value_t = 80, value_parser = positive_unit)]
    unit: u32,

    /// Sound timbre: sine, square, triangle, sawtooth, or piano
    #[arg(short = 'w', long = "wave", visible_alias = "timbre", default_value = "sine", value_parser = parse_waveform)]
    waveform: Waveform,

    /// OGG output path
    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,

    /// Examine encoding or decoding; accepts e/encode or d/decode
    #[arg(short = 'e', long = "exam", value_name = "MODE", ignore_case = true)]
    exam: Option<ExamDirection>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ExamDirection {
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

fn main() {
    if let Err(error) = run() {
        eprintln!("morse: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
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

fn run_exam(direction: ExamDirection, arguments: &[String]) -> Result<(), String> {
    let question = read_input(arguments)?;
    let (expected, expected_answer, question_morse, prompt) = match direction {
        ExamDirection::Encode => {
            let question_morse = encode_text(&question)?;
            let expected = alphanumeric_content(&question);
            (
                expected,
                question_morse.clone(),
                question_morse,
                format!("Encode: {question}"),
            )
        }
        ExamDirection::Decode => {
            let expected_text = decode_morse(&question)?;
            let question_morse = question.clone();
            (
                alphanumeric_content(&expected_text),
                expected_text,
                question_morse,
                format!("Decode: {question}"),
            )
        }
    };
    if expected.is_empty() {
        return Err("exam question must contain at least one English letter or digit".into());
    }

    println!("{prompt}");
    print!("Answer: ");
    io::stdout()
        .flush()
        .map_err(|e| format!("could not write prompt: {e}"))?;
    let started = Instant::now();
    let answer = read_answer()?;
    let elapsed = started.elapsed();
    let actual = match direction {
        ExamDirection::Encode => decode_morse(&answer)
            .map(|text| alphanumeric_content(&text))
            .unwrap_or_default(),
        ExamDirection::Decode => alphanumeric_content(&answer),
    };

    if actual != expected {
        println!("Incorrect, expected:");
        println!("{expected_answer}");
        return Ok(());
    }

    let seconds = elapsed.as_secs_f64();
    let rate_seconds = seconds.max(f64::EPSILON);
    let signals_per_minute = morse_signal_count(&question_morse) as f64 * 60.0 / rate_seconds;
    let characters_per_minute = expected.len() as f64 * 60.0 / rate_seconds;
    println!(
        "Correct, consumed {seconds:.3} s, rate: {signals_per_minute:.2} sigs/min, {characters_per_minute:.2} chars/min"
    );
    Ok(())
}

fn default_output_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    PathBuf::from(format!("audio/morse-{timestamp}.ogg"))
}

fn read_input(arguments: &[String]) -> Result<String, String> {
    if !arguments.is_empty() {
        return Ok(arguments.join(" "));
    }
    if io::stdin().is_terminal() {
        print!("Enter text or morse code: ");
        io::stdout()
            .flush()
            .map_err(|e| format!("could not write prompt: {e}"))?;
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| format!("could not read stdin: {e}"))?;
        return Ok(input);
    }
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("could not read stdin: {e}"))?;
    Ok(input)
}

fn read_answer() -> Result<String, String> {
    let mut answer = String::new();
    if io::stdin().is_terminal() {
        io::stdin()
            .read_line(&mut answer)
            .map_err(|e| format!("could not read answer: {e}"))?;
    } else {
        io::stdin()
            .read_to_string(&mut answer)
            .map_err(|e| format!("could not read answer: {e}"))?;
    }
    Ok(answer)
}

fn encode_ogg(samples: &[f32], sample_rate: u32, output: &PathBuf) -> Result<(), String> {
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

    {
        let stdin = child.stdin.as_mut().ok_or("could not open ffmpeg input")?;
        for sample in samples {
            stdin
                .write_all(&sample.to_le_bytes())
                .map_err(|e| format!("could not send audio to ffmpeg: {e}"))?;
        }
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
