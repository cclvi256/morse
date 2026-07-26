use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use clap::{Parser, ValueEnum};
use crossterm::{
    event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{self, disable_raw_mode, enable_raw_mode},
};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Inspect terminal keyboard events and key-hold timing for Morse input"
)]
struct Cli {
    /// Morse time unit in milliseconds
    #[arg(short, long, default_value_t = 80, value_parser = parse_positive_u64)]
    unit: u64,

    /// Kitty keyboard protocol handling
    #[arg(long, value_enum, default_value_t = Enhancement::Auto)]
    enhancement: Enhancement,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Enhancement {
    /// Enable enhanced events when the terminal reports support
    Auto,
    /// Request enhanced events even when support detection fails
    Force,
    /// Do not request enhanced keyboard events
    Off,
}

struct TerminalGuard {
    enhanced: bool,
}

impl TerminalGuard {
    fn enter(enhancement: Enhancement) -> Result<(Self, bool), String> {
        enable_raw_mode().map_err(|error| format!("could not enable raw mode: {error}"))?;

        let detected = terminal::supports_keyboard_enhancement().unwrap_or(false);
        let should_enable = match enhancement {
            Enhancement::Auto => detected,
            Enhancement::Force => true,
            Enhancement::Off => false,
        };
        if should_enable
            && let Err(error) = execute!(
                io::stdout(),
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                )
            )
        {
            let _ = disable_raw_mode();
            return Err(format!(
                "could not request enhanced keyboard events: {error}"
            ));
        }

        Ok((
            Self {
                enhanced: should_enable,
            },
            detected,
        ))
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.enhanced {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = disable_raw_mode();
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("morse-keyboard-debug: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("an interactive terminal is required".into());
    }

    let (guard, enhancement_detected) = TerminalGuard::enter(cli.enhancement)?;
    let enhancement_enabled = guard.enhanced;
    let dit_dah_threshold = duration_for_units(cli.unit, 3.0_f64.sqrt());
    println!(
        "Keyboard debugger started. Press and hold candidate Morse keys; press Ctrl-C to stop.\r"
    );
    println!(
        "enhancement: requested={:?}, detected={}, enabled={}; unit={} ms; dit/dah threshold={:.1} ms (sqrt(3) units)\r",
        cli.enhancement,
        enhancement_detected,
        enhancement_enabled,
        cli.unit,
        millis(dit_dah_threshold)
    );
    if !enhancement_enabled {
        println!(
            "note: without enhanced keyboard events, most terminals report presses only; hold durations cannot be measured.\r"
        );
    }
    println!(
        "{:>9}  {:>9}  {:<7}  {:<16}  {:<14}  details\r",
        "elapsed", "delta", "kind", "code", "modifiers"
    );
    io::stdout()
        .flush()
        .map_err(|error| format!("could not write output: {error}"))?;

    let started = Instant::now();
    let mut previous = started;
    let mut held: HashMap<KeyCode, Instant> = HashMap::new();

    loop {
        let event = event::read().map_err(|error| format!("could not read event: {error}"))?;
        let now = Instant::now();
        let elapsed = now.duration_since(started);
        let delta = now.duration_since(previous);
        previous = now;

        match event {
            Event::Key(key) => {
                if is_ctrl_c(key) {
                    println!(
                        "{:>8.1}ms  {:>8.1}ms  {:<7}  {:<16}  {:<14}  stop\r",
                        millis(elapsed),
                        millis(delta),
                        kind_name(key.kind),
                        format!("{:?}", key.code),
                        format!("{:?}", key.modifiers),
                    );
                    break;
                }
                let details =
                    describe_key_lifecycle(&mut held, key.code, key.kind, now, dit_dah_threshold);
                println!(
                    "{:>8.1}ms  {:>8.1}ms  {:<7}  {:<16}  {:<14}  state={:?}{}\r",
                    millis(elapsed),
                    millis(delta),
                    kind_name(key.kind),
                    format!("{:?}", key.code),
                    format!("{:?}", key.modifiers),
                    key.state,
                    details,
                );
            }
            other => println!(
                "{:>8.1}ms  {:>8.1}ms  {:<7}  {:?}\r",
                millis(elapsed),
                millis(delta),
                "other",
                other
            ),
        }
        io::stdout()
            .flush()
            .map_err(|error| format!("could not write output: {error}"))?;
    }

    if !held.is_empty() {
        println!(
            "{} key(s) still appeared held at exit; their release events were not observed.\r",
            held.len()
        );
    }
    drop(guard);
    Ok(())
}

fn describe_key_lifecycle(
    held: &mut HashMap<KeyCode, Instant>,
    code: KeyCode,
    kind: KeyEventKind,
    now: Instant,
    dit_dah_threshold: Duration,
) -> String {
    match kind {
        KeyEventKind::Press => match held.entry(code) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(now);
                " | tracking hold".into()
            }
            std::collections::hash_map::Entry::Occupied(_) => {
                " | WARNING: duplicate press before release".into()
            }
        },
        KeyEventKind::Repeat => held.get(&code).map_or_else(
            || " | WARNING: repeat without a tracked press".into(),
            |pressed| format!(" | held={:.1}ms", millis(now.duration_since(*pressed))),
        ),
        KeyEventKind::Release => held.remove(&code).map_or_else(
            || " | WARNING: release without a tracked press".into(),
            |pressed| {
                let duration = now.duration_since(pressed);
                let signal = if duration > dit_dah_threshold {
                    "dah (-)"
                } else {
                    "dit (.)"
                };
                format!(" | held={:.1}ms => {signal}", millis(duration))
            },
        ),
    }
}

fn is_ctrl_c(key: KeyEvent) -> bool {
    key.kind == KeyEventKind::Press
        && key.code == KeyCode::Char('c')
        && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn kind_name(kind: KeyEventKind) -> &'static str {
    match kind {
        KeyEventKind::Press => "press",
        KeyEventKind::Repeat => "repeat",
        KeyEventKind::Release => "release",
    }
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn duration_for_units(unit_ms: u64, units: f64) -> Duration {
    Duration::from_secs_f64(unit_ms as f64 * units / 1_000.0)
}

fn parse_positive_u64(value: &str) -> Result<u64, String> {
    let value = value
        .parse()
        .map_err(|_| "unit must be a positive integer".to_string())?;
    if value == 0 {
        Err("unit must be greater than zero".into())
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_preserves_initial_press_across_duplicate_press() {
        let key = KeyCode::Char('a');
        let start = Instant::now();
        let mut held = HashMap::new();

        assert!(
            describe_key_lifecycle(
                &mut held,
                key,
                KeyEventKind::Press,
                start,
                Duration::from_millis(50)
            )
            .contains("tracking")
        );
        assert!(
            describe_key_lifecycle(
                &mut held,
                key,
                KeyEventKind::Press,
                start + Duration::from_millis(30),
                Duration::from_millis(50)
            )
            .contains("duplicate")
        );
        assert!(
            describe_key_lifecycle(
                &mut held,
                key,
                KeyEventKind::Release,
                start + Duration::from_millis(80),
                Duration::from_millis(50)
            )
            .contains("dah")
        );
    }

    #[test]
    fn release_without_press_is_reported() {
        let mut held = HashMap::new();
        let result = describe_key_lifecycle(
            &mut held,
            KeyCode::Enter,
            KeyEventKind::Release,
            Instant::now(),
            Duration::from_millis(50),
        );
        assert!(result.contains("without a tracked press"));
    }
}
