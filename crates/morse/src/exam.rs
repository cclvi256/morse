use std::{
    io::{self, Write},
    time::Instant,
};

use morse::{alphanumeric_content, decode_morse, encode_text, morse_signal_count};

use crate::{
    cli::ExamDirection,
    input::{read_answer, read_input},
};

pub(crate) fn run_exam(direction: ExamDirection, arguments: &[String]) -> Result<(), String> {
    let question = read_input(arguments)?;
    let (expected, expected_answer, question_morse, prompt) = match direction {
        ExamDirection::Encode => {
            let question_morse = encode_text(&question)?;
            (
                alphanumeric_content(&question),
                question_morse.clone(),
                question_morse,
                format!("Encode: {question}"),
            )
        }
        ExamDirection::Decode => {
            let expected_text = decode_morse(&question)?;
            (
                alphanumeric_content(&expected_text),
                expected_text,
                question.clone(),
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
