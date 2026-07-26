use std::f32::consts::TAU;

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

pub fn synthesize(
    morse: &str,
    frequency: f32,
    unit_ms: u32,
    waveform: Waveform,
    sample_rate: u32,
) -> Vec<f32> {
    let unit_samples = ((sample_rate as u64 * unit_ms as u64) / 1_000).max(1) as usize;
    let fade_samples = (sample_rate as usize / 200).min(unit_samples / 2);
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
                    let silence_units = if gap_end - gap_start == 1 && symbols[gap_start] == ' ' {
                        3
                    } else if gap_end - gap_start == 2
                        && symbols[gap_start..gap_end].iter().all(|c| *c == ' ')
                    {
                        7
                    } else {
                        14
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
