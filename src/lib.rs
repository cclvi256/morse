use std::f32::consts::TAU;

const MORSE_TABLE: &[(char, &str)] = &[
    ('A', ".-"),
    ('B', "-..."),
    ('C', "-.-."),
    ('D', "-.."),
    ('E', "."),
    ('F', "..-."),
    ('G', "--."),
    ('H', "...."),
    ('I', ".."),
    ('J', ".---"),
    ('K', "-.-"),
    ('L', ".-.."),
    ('M', "--"),
    ('N', "-."),
    ('O', "---"),
    ('P', ".--."),
    ('Q', "--.-"),
    ('R', ".-."),
    ('S', "..."),
    ('T', "-"),
    ('U', "..-"),
    ('V', "...-"),
    ('W', ".--"),
    ('X', "-..-"),
    ('Y', "-.--"),
    ('Z', "--.."),
    ('0', "-----"),
    ('1', ".----"),
    ('2', "..---"),
    ('3', "...--"),
    ('4', "....-"),
    ('5', "....."),
    ('6', "-...."),
    ('7', "--..."),
    ('8', "---.."),
    ('9', "----."),
];

const ERROR_PROSIGN: &str = "........";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Text,
    Morse,
}

/// Incrementally translates keyed Morse without requiring whitespace between
/// letters. This is used by the terminal real-time mode and is independent of
/// terminal event handling so its behaviour can be tested directly.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MorseStreamDecoder {
    text: String,
    current_code: String,
}

impl MorseStreamDecoder {
    pub fn push_signal(&mut self, signal: char) -> Result<(), String> {
        match signal {
            '.' | '-' => {
                self.current_code.push(signal);
                Ok(())
            }
            other => Err(format!("invalid Morse signal '{other}'")),
        }
    }

    /// Finalize the Morse sequence currently being keyed. Unknown sequences
    /// are retained as `?`, allowing a real-time session to continue.
    pub fn finish_letter(&mut self) -> bool {
        if self.current_code.is_empty() {
            return false;
        }
        if self.current_code == ERROR_PROSIGN {
            self.current_code.clear();
            return self.text.pop().is_some();
        }
        let letter = MORSE_TABLE
            .iter()
            .find(|(_, morse)| *morse == self.current_code)
            .map(|(letter, _)| *letter)
            .unwrap_or('?');
        self.text.push(letter);
        self.current_code.clear();
        true
    }

    pub fn finish_word(&mut self) -> bool {
        let changed = self.finish_letter();
        if !self.text.is_empty() && !self.text.ends_with(' ') {
            self.text.push(' ');
            true
        } else {
            changed
        }
    }

    pub fn morse(&self) -> &str {
        &self.current_code
    }

    /// Text decoded so far, including a tentative decoding of the Morse
    /// sequence currently being entered.
    pub fn display_text(&self) -> String {
        let mut text = self.text.clone();
        if !self.current_code.is_empty() {
            if self.current_code == ERROR_PROSIGN {
                text.push('⌫');
            } else {
                text.push(
                    MORSE_TABLE
                        .iter()
                        .find(|(_, morse)| *morse == self.current_code)
                        .map(|(letter, _)| *letter)
                        .unwrap_or('?'),
                );
            }
        }
        text
    }

    pub fn finalized_text(&self) -> &str {
        &self.text
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Square,
    Triangle,
    Sawtooth,
    Piano,
}

impl Waveform {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "sine" | "sin" => Ok(Self::Sine),
            "square" => Ok(Self::Square),
            "triangle" | "tri" => Ok(Self::Triangle),
            "saw" | "sawtooth" => Ok(Self::Sawtooth),
            "piano" => Ok(Self::Piano),
            _ => Err(format!(
                "unsupported timbre '{value}'; use sine, square, triangle, sawtooth, or piano"
            )),
        }
    }
}

pub fn detect_input(input: &str) -> Result<InputKind, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("input is empty".into());
    }

    if trimmed
        .chars()
        .all(|c| matches!(c, '.' | '·' | '-' | '_' | '/' | ' ' | '\t' | '\r' | '\n'))
    {
        Ok(InputKind::Morse)
    } else if trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c.is_ascii_whitespace())
    {
        Ok(InputKind::Text)
    } else {
        Err(
            "input must contain either Morse symbols (., ·, -, _, /) or English letters and digits"
                .into(),
        )
    }
}

pub fn encode_text(input: &str) -> Result<String, String> {
    input
        .split_whitespace()
        .map(|word| {
            word.chars()
                .map(|c| {
                    let upper = c.to_ascii_uppercase();
                    MORSE_TABLE
                        .iter()
                        .find(|(letter, _)| *letter == upper)
                        .map(|(_, code)| *code)
                        .ok_or_else(|| format!("unsupported character '{c}'"))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(|letters| letters.join(" "))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|words| words.join("  "))
}

pub fn decode_morse(input: &str) -> Result<String, String> {
    let mut output = String::new();
    let mut code = String::new();
    let mut pending_gap = None;
    let characters: Vec<char> = input.chars().collect();
    let mut index = 0;

    while index < characters.len() {
        match characters[index] {
            '.' | '·' => {
                if code.is_empty() {
                    append_gap(&mut output, pending_gap.take());
                }
                code.push('.');
                index += 1;
            }
            '-' | '_' => {
                if code.is_empty() {
                    append_gap(&mut output, pending_gap.take());
                }
                code.push('-');
                index += 1;
            }
            '/' => {
                append_decoded_code(&mut output, &mut code)?;
                pending_gap = Some(max_gap(pending_gap, MorseGap::Word));
                index += 1;
            }
            whitespace if whitespace.is_ascii_whitespace() => {
                append_decoded_code(&mut output, &mut code)?;
                let start = index;
                while index < characters.len() && characters[index].is_ascii_whitespace() {
                    index += 1;
                }
                let gap = whitespace_gap(&characters[start..index]);
                pending_gap = Some(max_gap(pending_gap, gap));
            }
            other => return Err(format!("invalid Morse character '{other}'")),
        }
    }
    append_decoded_code(&mut output, &mut code)?;
    Ok(output)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MorseGap {
    Letter,
    Word,
    Paragraph,
}

fn whitespace_gap(characters: &[char]) -> MorseGap {
    if characters.len() == 1 && characters[0] == ' ' {
        MorseGap::Letter
    } else if characters.iter().all(|character| *character == ' ') && characters.len() == 2 {
        MorseGap::Word
    } else {
        MorseGap::Paragraph
    }
}

fn max_gap(current: Option<MorseGap>, next: MorseGap) -> MorseGap {
    current.map_or(next, |gap| gap.max(next))
}

fn append_decoded_code(output: &mut String, code: &mut String) -> Result<(), String> {
    if code.is_empty() {
        return Ok(());
    }
    let letter = MORSE_TABLE
        .iter()
        .find(|(_, morse)| *morse == code)
        .map(|(letter, _)| *letter)
        .ok_or_else(|| format!("unknown Morse sequence '{code}'"))?;
    output.push(letter);
    code.clear();
    Ok(())
}

fn append_gap(output: &mut String, gap: Option<MorseGap>) {
    match gap {
        Some(MorseGap::Word) if !output.is_empty() && !output.ends_with([' ', '\n']) => {
            output.push(' ')
        }
        Some(MorseGap::Paragraph) if !output.is_empty() && !output.ends_with("\n\n") => {
            while output.ends_with(' ') {
                output.pop();
            }
            output.push_str("\n\n");
        }
        _ => {}
    }
}

pub fn alphanumeric_content(input: &str) -> String {
    input
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_uppercase())
        .collect()
}

pub fn morse_signal_count(input: &str) -> usize {
    input
        .chars()
        .filter(|character| matches!(character, '.' | '·' | '-' | '_'))
        .count()
}

pub fn translate(input: &str) -> Result<(InputKind, String, String), String> {
    match detect_input(input)? {
        InputKind::Text => {
            let morse = encode_text(input)?;
            Ok((InputKind::Text, morse.clone(), morse))
        }
        InputKind::Morse => {
            let text = decode_morse(input)?;
            let canonical_morse = encode_text(&text)?;
            Ok((InputKind::Morse, text, canonical_morse))
        }
    }
}

pub fn synthesize(
    morse: &str,
    frequency: f32,
    unit_ms: u32,
    waveform: Waveform,
    sample_rate: u32,
) -> Vec<f32> {
    let unit_samples = ((sample_rate as u64 * unit_ms as u64) / 1_000).max(1) as usize;
    let fade_samples = (sample_rate as usize / 200).min(unit_samples / 2); // 5 ms
    let mut samples = Vec::new();
    let symbols: Vec<char> = morse.chars().collect();
    let mut index = 0;

    while index < symbols.len() {
        let symbol = symbols[index];
        let tone_units = match symbol {
            '.' => 1,
            '-' => 3,
            _ => 0,
        };
        if tone_units > 0 {
            append_tone(
                &mut samples,
                tone_units * unit_samples,
                frequency,
                waveform,
                sample_rate,
                fade_samples,
            );
        }

        if matches!(symbol, '.' | '-') {
            match symbols.get(index + 1) {
                Some('.' | '-') => samples.resize(samples.len() + unit_samples, 0.0),
                Some(next) if next.is_ascii_whitespace() => {
                    let gap_start = index + 1;
                    let mut gap_end = gap_start;
                    while matches!(symbols.get(gap_end), Some(character) if character.is_ascii_whitespace())
                    {
                        gap_end += 1;
                    }
                    let silence_units = match whitespace_gap(&symbols[gap_start..gap_end]) {
                        MorseGap::Letter => 3,
                        MorseGap::Word => 7,
                        MorseGap::Paragraph => 14,
                    };
                    samples.resize(samples.len() + silence_units * unit_samples, 0.0);
                    index = gap_end;
                    continue;
                }
                Some('/') => {
                    samples.resize(samples.len() + 7 * unit_samples, 0.0);
                    index += 2;
                    continue;
                }
                _ => {}
            }
        }
        index += 1;
    }
    samples
}

fn append_tone(
    output: &mut Vec<f32>,
    count: usize,
    frequency: f32,
    waveform: Waveform,
    sample_rate: u32,
    fade_samples: usize,
) {
    for i in 0..count {
        let phase = frequency * i as f32 / sample_rate as f32;
        let base = match waveform {
            Waveform::Sine => (TAU * phase).sin(),
            Waveform::Square => {
                if phase.fract() < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Waveform::Triangle => 1.0 - 4.0 * (phase.fract() - 0.5).abs(),
            Waveform::Sawtooth => 2.0 * phase.fract() - 1.0,
            Waveform::Piano => {
                let elapsed = i as f32 / sample_rate as f32;
                let harmonics = (TAU * phase).sin()
                    + 0.50 * (TAU * phase * 2.0).sin()
                    + 0.25 * (TAU * phase * 3.0).sin()
                    + 0.12 * (TAU * phase * 4.0).sin();
                harmonics / 1.87 * (-2.4 * elapsed).exp()
            }
        };
        let attack = if fade_samples == 0 {
            1.0
        } else {
            (i as f32 / fade_samples as f32).min(1.0)
        };
        let release = if fade_samples == 0 {
            1.0
        } else {
            ((count - i - 1) as f32 / fade_samples as f32).min(1.0)
        };
        output.push(base * attack.min(release) * 0.72);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_text_to_morse() {
        assert_eq!(encode_text("SOS 2").unwrap(), "... --- ...  ..---");
    }

    #[test]
    fn translates_morse_to_text() {
        assert_eq!(decode_morse("... --- ...  ..---").unwrap(), "SOS 2");
    }

    #[test]
    fn accepts_alternate_morse_symbols() {
        assert_eq!(decode_morse("··· ___ ··· / ··___").unwrap(), "SOS 2");
        assert_eq!(detect_input("·_·").unwrap(), InputKind::Morse);
    }

    #[test]
    fn decodes_requested_whitespace_boundaries() {
        assert_eq!(decode_morse(".- -...").unwrap(), "AB");
        assert_eq!(decode_morse(".-  -...").unwrap(), "A B");
        assert_eq!(decode_morse(".-   -...").unwrap(), "A\n\nB");
        assert_eq!(decode_morse(".-\t-...").unwrap(), "A\n\nB");
        assert_eq!(decode_morse(".- \n -...").unwrap(), "A\n\nB");
    }

    #[test]
    fn normalizes_exam_content_and_counts_signals() {
        assert_eq!(alphanumeric_content("s-o_s 2"), "SOS2");
        assert_eq!(morse_signal_count(".- ·_ /"), 4);
    }

    #[test]
    fn distinguishes_charsets() {
        assert_eq!(detect_input("Hello 2").unwrap(), InputKind::Text);
        assert_eq!(detect_input(".... ..").unwrap(), InputKind::Morse);
        assert!(detect_input("Hello!").is_err());
    }

    #[test]
    fn morse_timing_is_correct() {
        // .- has 1 tone + 1 gap + 3 tone units.
        assert_eq!(synthesize(".-", 700.0, 10, Waveform::Sine, 1_000).len(), 50);
    }

    #[test]
    fn streams_morse_as_it_is_keyed() {
        let mut decoder = MorseStreamDecoder::default();
        decoder.push_signal('.').unwrap();
        decoder.push_signal('.').unwrap();
        decoder.push_signal('.').unwrap();
        assert_eq!(decoder.morse(), "...");
        assert_eq!(decoder.display_text(), "S");
        assert!(decoder.finish_letter());
        decoder.push_signal('-').unwrap();
        decoder.push_signal('-').unwrap();
        decoder.push_signal('-').unwrap();
        decoder.finish_word();
        assert_eq!(decoder.finalized_text(), "SO ");
    }

    #[test]
    fn streams_invalid_morse_without_stopping() {
        let mut decoder = MorseStreamDecoder::default();
        for signal in ".-.-.-".chars() {
            decoder.push_signal(signal).unwrap();
        }
        decoder.finish_letter();
        decoder.push_signal('.').unwrap();
        decoder.finish_letter();
        assert_eq!(decoder.finalized_text(), "?E");
    }

    #[test]
    fn eight_dits_are_the_realtime_error_backspace_prosign() {
        let mut decoder = MorseStreamDecoder::default();
        decoder.push_signal('.').unwrap();
        decoder.finish_letter();
        for _ in 0..8 {
            decoder.push_signal('.').unwrap();
        }
        assert_eq!(decoder.display_text(), "E⌫");
        assert!(decoder.finish_letter());
        assert_eq!(decoder.finalized_text(), "");
    }
}
