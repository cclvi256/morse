use std::{
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) fn default_output_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    PathBuf::from(format!("audio/morse-{timestamp}.ogg"))
}

pub(crate) fn read_input(arguments: &[String]) -> Result<String, String> {
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

pub(crate) fn read_answer() -> Result<String, String> {
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
