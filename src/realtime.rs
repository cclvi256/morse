use std::{
    fmt,
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

use crossterm::{
    cursor,
    event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{self, ClearType},
};
use morse::MorseStreamDecoder;

const DIT_DAH_BOUNDARY_UNITS: f64 = 1.732_050_807_568_877_2; // sqrt(3)
const LETTER_WORD_BOUNDARY_UNITS: f64 = 4.582_575_694_955_84; // sqrt(21)

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

impl fmt::Display for RealtimeKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
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
        if !terminal::supports_keyboard_enhancement().unwrap_or(false) {
            let _ = terminal::disable_raw_mode();
            return Err("single-key mode needs a terminal with kitty keyboard protocol support to receive key-release events; choose double-key mode instead".into());
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

pub(crate) fn run_realtime(unit_ms: u32) -> Result<(), String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("real-time mode requires an interactive terminal".into());
    }
    let mode = choose_realtime_mode()?;
    match mode {
        RealtimeMode::Single { key } => println!(
            "Ready. Press {key}; up to {:.1} ms is a dot and a longer hold is a dash (unit: {unit_ms} ms). Press Escape or Ctrl-C to finish.",
            millis_for_units(unit_ms, DIT_DAH_BOUNDARY_UNITS)
        ),
        RealtimeMode::Double { dot, dash } => println!(
            "Ready. {dot} keys a dot and {dash} keys a dash. Press Escape or Ctrl-C to finish."
        ),
    }
    let _raw_mode = RawMode::enable(matches!(mode, RealtimeMode::Single { .. }))?;
    let mut decoder = MorseStreamDecoder::default();
    let mut held_since = None;
    let mut last_signal_at = None;
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
        if !event::poll(Duration::from_millis(20))
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
                    record_initial_press(&mut held_since, Instant::now())
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
    match prompt_line("Mode [1/2]: ")?.trim() {
        "1" | "single" => Ok(RealtimeMode::Single {
            key: prompt_realtime_key("Key to use [space/enter/tab or one character]: ")?,
        }),
        "2" | "double" => {
            let dot = prompt_realtime_key("Key for dot (.): ")?;
            let dash = prompt_realtime_key("Key for dash (-): ")?;
            if dot == dash {
                Err("dot and dash must use different keys".into())
            } else {
                Ok(RealtimeMode::Double { dot, dash })
            }
        }
        _ => Err("choose 1 (single key) or 2 (double key)".into()),
    }
}
fn prompt_realtime_key(prompt: &str) -> Result<RealtimeKey, String> {
    let key = RealtimeKey::parse(&prompt_line(prompt)?)?;
    if matches!(key, RealtimeKey::Char('\u{1b}')) {
        Err("Escape is reserved for exiting real-time mode".into())
    } else {
        Ok(key)
    }
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
fn duration_for_units(unit_ms: u32, units: f64) -> Duration {
    Duration::from_secs_f64(f64::from(unit_ms) * units / 1_000.0)
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
        terminal::Clear(ClearType::CurrentLine)
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
mod tests {
    use super::*;
    #[test]
    fn held_dash_does_not_end_the_previous_letter() {
        let mut decoder = MorseStreamDecoder::default();
        decoder.push_signal('-').unwrap();
        decoder.push_signal('.').unwrap();
        let ago = Instant::now() - Duration::from_millis(300);
        assert!(!finalize_realtime_idle(
            &mut decoder,
            Some(ago),
            Instant::now(),
            100,
            true
        ));
        decoder.push_signal('-').unwrap();
        decoder.push_signal('.').unwrap();
        decoder.finish_letter();
        assert_eq!(decoder.finalized_text(), "C");
    }
    #[test]
    fn geometric_mean_boundaries_separate_morse_timings() {
        assert_eq!(
            duration_for_units(100, DIT_DAH_BOUNDARY_UNITS).as_millis(),
            173
        );
        assert_eq!(
            duration_for_units(100, LETTER_WORD_BOUNDARY_UNITS).as_millis(),
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
            false
        ));
        assert!(finalize_realtime_idle(
            &mut decoder,
            Some(now - Duration::from_millis(180)),
            now,
            100,
            false
        ));
        assert_eq!(decoder.finalized_text(), "E");
        assert!(finalize_realtime_idle(
            &mut decoder,
            Some(now - Duration::from_millis(460)),
            now,
            100,
            false
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
        assert!(
            (pressed_at + Duration::from_millis(250)).duration_since(held_since.unwrap())
                > duration_for_units(100, DIT_DAH_BOUNDARY_UNITS)
        );
    }
}
