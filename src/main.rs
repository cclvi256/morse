use std::fs::{self, File};
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, ValueEnum};
use crossterm::{
    cursor,
    event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{self, ClearType},
};
use morse::{
    InputKind, MorseStreamDecoder, Waveform, alphanumeric_content, decode_morse, encode_text,
    morse_signal_count, translate,
};

const SAMPLE_RATE: u32 = 44_100;
const DIT_DAH_BOUNDARY_UNITS: f64 = 1.732_050_807_568_877_2; // sqrt(3)
const LETTER_WORD_BOUNDARY_UNITS: f64 = 4.582_575_694_955_84; // sqrt(21)

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

    /// Decode Morse as you key it in the focused terminal
    #[arg(
        short = 'r',
        long = "rt",
        visible_alias = "realtime",
        conflicts_with_all = ["input", "exam", "frequency", "waveform", "output"]
    )]
    realtime: bool,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RealtimeKey {
    Char(char),
    Space,
    Enter,
    Tab,
}

impl RealtimeKey {
    fn parse(value: &str) -> Result<Self, String> {
        let value = value.trim();
        match value.to_ascii_lowercase().as_str() {
            "space" => Ok(Self::Space),
            "enter" | "return" => Ok(Self::Enter),
            "tab" => Ok(Self::Tab),
            _ if value.chars().count() == 1 => Ok(Self::Char(
                value
                    .chars()
                    .next()
                    .expect("a one-character key must have a character"),
            )),
            _ => Err("enter one character, or use space, enter, or tab".into()),
        }
    }

    fn matches(self, code: KeyCode) -> bool {
        match (self, code) {
            (Self::Space, KeyCode::Char(' '))
            | (Self::Enter, KeyCode::Enter)
            | (Self::Tab, KeyCode::Tab) => true,
            (Self::Char(expected), KeyCode::Char(actual)) => expected.eq_ignore_ascii_case(&actual),
            _ => false,
        }
    }
}

impl std::fmt::Display for RealtimeKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Char(character) => write!(formatter, "{character}"),
            Self::Space => formatter.write_str("space"),
            Self::Enter => formatter.write_str("enter"),
            Self::Tab => formatter.write_str("tab"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum RealtimeMode {
    Single { key: RealtimeKey },
    Double { dot: RealtimeKey, dash: RealtimeKey },
}

struct RawMode {
    keyboard_enhancement_enabled: bool,
}

impl RawMode {
    fn enable(require_key_release_events: bool) -> Result<Self, String> {
        terminal::enable_raw_mode()
            .map_err(|error| format!("could not enable raw terminal mode: {error}"))?;
        if !require_key_release_events {
            return Ok(Self {
                keyboard_enhancement_enabled: false,
            });
        }

        let supported = terminal::supports_keyboard_enhancement().unwrap_or(false);
        if !supported {
            let _ = terminal::disable_raw_mode();
            return Err(
                "single-key mode needs a terminal with kitty keyboard protocol support to receive key-release events; choose double-key mode instead"
                    .into(),
            );
        }
        if let Err(error) = execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            )
        ) {
            let _ = terminal::disable_raw_mode();
            return Err(format!("could not enable key-release events: {error}"));
        }
        Ok(Self {
            keyboard_enhancement_enabled: true,
        })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if self.keyboard_enhancement_enabled {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = terminal::disable_raw_mode();
    }
}

fn run_realtime(unit_ms: u32) -> Result<(), String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("real-time mode requires an interactive terminal".into());
    }

    let mode = choose_realtime_mode()?;
    match mode {
        RealtimeMode::Single { key } => println!(
            "Ready. Press {key}; up to {:.1} ms is a dot and a longer hold is a dash (unit: {unit_ms} ms). Press Escape or Ctrl-C to finish.",
            millis_for_units(unit_ms, DIT_DAH_BOUNDARY_UNITS),
        ),
        RealtimeMode::Double { dot, dash } => println!(
            "Ready. {dot} keys a dot and {dash} keys a dash. Press Escape or Ctrl-C to finish."
        ),
    }
    let _raw_mode = RawMode::enable(matches!(mode, RealtimeMode::Single { .. }))?;
    let mut decoder = MorseStreamDecoder::default();
    let mut held_since: Option<Instant> = None;
    let mut last_signal_at: Option<Instant> = None;
    redraw_realtime(&decoder)?;

    loop {
        let now = Instant::now();
        let single_key_is_held =
            matches!(mode, RealtimeMode::Single { .. }) && held_since.is_some();
        if finalize_realtime_idle(
            &mut decoder,
            last_signal_at,
            now,
            unit_ms,
            single_key_is_held,
        ) {
            redraw_realtime(&decoder)?;
        }

        if !event::poll(std::time::Duration::from_millis(20))
            .map_err(|error| format!("could not read keyboard event: {error}"))?
        {
            continue;
        }
        let Event::Key(key_event) =
            event::read().map_err(|error| format!("could not read keyboard event: {error}"))?
        else {
            continue;
        };
        if is_exit_key(key_event) {
            break;
        }
        match mode {
            RealtimeMode::Single { key } => match key_event.kind {
                KeyEventKind::Press if key.matches(key_event.code) => {
                    // Some terminals report auto-repeat as additional Press events.
                    // The first press, not the latest repeat, defines the hold time.
                    record_initial_press(&mut held_since, Instant::now());
                }
                KeyEventKind::Release if key.matches(key_event.code) => {
                    if let Some(pressed_at) = held_since.take() {
                        let signal = if pressed_at.elapsed()
                            > duration_for_units(unit_ms, DIT_DAH_BOUNDARY_UNITS)
                        {
                            '-'
                        } else {
                            '.'
                        };
                        decoder.push_signal(signal)?;
                        last_signal_at = Some(Instant::now());
                        redraw_realtime(&decoder)?;
                    }
                }
                _ => {}
            },
            RealtimeMode::Double { dot, dash } if key_event.kind == KeyEventKind::Press => {
                let signal = if dot.matches(key_event.code) {
                    Some('.')
                } else if dash.matches(key_event.code) {
                    Some('-')
                } else {
                    None
                };
                if let Some(signal) = signal {
                    decoder.push_signal(signal)?;
                    last_signal_at = Some(Instant::now());
                    redraw_realtime(&decoder)?;
                }
            }
            RealtimeMode::Double { .. } => {}
        }
    }

    decoder.finish_letter();
    execute!(
        io::stdout(),
        cursor::MoveToColumn(0),
        terminal::Clear(ClearType::CurrentLine)
    )
    .map_err(|error| format!("could not update terminal: {error}"))?;
    println!("Text: {}", decoder.finalized_text().trim_end());
    Ok(())
}

fn choose_realtime_mode() -> Result<RealtimeMode, String> {
    println!("Choose keying mode: 1) single key (short = ., long = -)  2) double key (. and -)");
    let choice = prompt_line("Mode [1/2]: ")?;
    match choice.trim() {
        "1" | "single" => {
            let key = prompt_realtime_key("Key to use [space/enter/tab or one character]: ")?;
            Ok(RealtimeMode::Single { key })
        }
        "2" | "double" => {
            let dot = prompt_realtime_key("Key for dot (.): ")?;
            let dash = prompt_realtime_key("Key for dash (-): ")?;
            if dot == dash {
                return Err("dot and dash must use different keys".into());
            }
            Ok(RealtimeMode::Double { dot, dash })
        }
        _ => Err("choose 1 (single key) or 2 (double key)".into()),
    }
}

fn prompt_realtime_key(prompt: &str) -> Result<RealtimeKey, String> {
    let key = RealtimeKey::parse(&prompt_line(prompt)?)?;
    if matches!(key, RealtimeKey::Char('\u{1b}')) {
        return Err("Escape is reserved for exiting real-time mode".into());
    }
    Ok(key)
}

fn prompt_line(prompt: &str) -> Result<String, String> {
    print!("{prompt}");
    io::stdout()
        .flush()
        .map_err(|error| format!("could not write prompt: {error}"))?;
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(|error| format!("could not read selection: {error}"))?;
    Ok(value)
}

fn is_exit_key(event: KeyEvent) -> bool {
    event.code == KeyCode::Esc
        || (event.code == KeyCode::Char('c')
            && event
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL))
}

fn finalize_realtime_idle(
    decoder: &mut MorseStreamDecoder,
    last_signal_at: Option<Instant>,
    now: Instant,
    unit_ms: u32,
    key_is_held: bool,
) -> bool {
    // The Morse gap begins after a key is released. In single-key mode a dash
    // may itself last longer than a letter gap, so it must never trigger a
    // separator while that key is still held.
    if key_is_held {
        return false;
    }
    let Some(last_signal_at) = last_signal_at else {
        return false;
    };
    let elapsed = now.duration_since(last_signal_at);
    if elapsed >= duration_for_units(unit_ms, LETTER_WORD_BOUNDARY_UNITS) {
        decoder.finish_word()
    } else if elapsed >= duration_for_units(unit_ms, DIT_DAH_BOUNDARY_UNITS) {
        decoder.finish_letter()
    } else {
        false
    }
}

fn duration_for_units(unit_ms: u32, units: f64) -> std::time::Duration {
    std::time::Duration::from_secs_f64(f64::from(unit_ms) * units / 1_000.0)
}

fn record_initial_press(held_since: &mut Option<Instant>, pressed_at: Instant) {
    held_since.get_or_insert(pressed_at);
}

fn millis_for_units(unit_ms: u32, units: f64) -> f64 {
    f64::from(unit_ms) * units
}

fn redraw_realtime(decoder: &MorseStreamDecoder) -> Result<(), String> {
    execute!(
        io::stdout(),
        cursor::MoveToColumn(0),
        terminal::Clear(ClearType::CurrentLine),
    )
    .map_err(|error| format!("could not update terminal: {error}"))?;
    print!(
        "Morse: {:<8} Text: {}",
        decoder.morse(),
        decoder.display_text()
    );
    io::stdout()
        .flush()
        .map_err(|error| format!("could not update terminal: {error}"))
}

#[cfg(test)]
mod realtime_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn held_dash_does_not_end_the_previous_letter() {
        let mut decoder = MorseStreamDecoder::default();
        decoder.push_signal('-').unwrap();
        decoder.push_signal('.').unwrap();
        let three_units_ago = Instant::now() - Duration::from_millis(300);

        assert!(!finalize_realtime_idle(
            &mut decoder,
            Some(three_units_ago),
            Instant::now(),
            100,
            true,
        ));
        decoder.push_signal('-').unwrap();
        decoder.push_signal('.').unwrap();
        decoder.finish_letter();
        assert_eq!(decoder.finalized_text(), "C");
    }

    #[test]
    fn geometric_mean_boundaries_separate_morse_timings() {
        let unit = 100;
        assert_eq!(
            duration_for_units(unit, DIT_DAH_BOUNDARY_UNITS).as_millis(),
            173
        );
        assert_eq!(
            duration_for_units(unit, LETTER_WORD_BOUNDARY_UNITS).as_millis(),
            458
        );
    }

    #[test]
    fn geometric_mean_gap_ends_letters_and_words() {
        let now = Instant::now();
        let mut decoder = MorseStreamDecoder::default();
        decoder.push_signal('.').unwrap();

        assert!(!finalize_realtime_idle(
            &mut decoder,
            Some(now - Duration::from_millis(170)),
            now,
            100,
            false,
        ));
        assert!(finalize_realtime_idle(
            &mut decoder,
            Some(now - Duration::from_millis(180)),
            now,
            100,
            false,
        ));
        assert_eq!(decoder.finalized_text(), "E");
        assert!(finalize_realtime_idle(
            &mut decoder,
            Some(now - Duration::from_millis(460)),
            now,
            100,
            false,
        ));
        assert_eq!(decoder.finalized_text(), "E ");
    }

    #[test]
    fn repeated_press_does_not_shorten_a_hold() {
        let pressed_at = Instant::now();
        let mut held_since = None;
        record_initial_press(&mut held_since, pressed_at);
        record_initial_press(&mut held_since, pressed_at + Duration::from_millis(250));

        assert_eq!(held_since, Some(pressed_at));
        let release_at = pressed_at + Duration::from_millis(250);
        assert!(
            release_at.duration_since(held_since.unwrap())
                > duration_for_units(100, DIT_DAH_BOUNDARY_UNITS)
        );
    }
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
