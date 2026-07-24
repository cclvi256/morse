# morse

A command-line English/digit to Morse translator (and Morse to English/digit
translator) that also renders the message as OGG Vorbis audio.

## Requirements

- Rust 1.85 or newer
- `ffmpeg` with the `libvorbis` encoder

## Build and use

```sh
cargo build --release
./target/release/morse "Hello 2026"
./target/release/morse ".... . .-.. .-.. ---  ..--- ----- ..--- -...."
./target/release/morse -- "-..."  # use -- when Morse begins with a dash
./target/release/morse -f 900 -u 120 -w square -o signal.ogg "SOS"
echo "MORSE 5" | ./target/release/morse --frequency=750 --timbre=piano
./target/release/morse --exam encode "SOS 2"
./target/release/morse -e d "... --- ...  ..---"
./target/release/morse --realtime
```

The input charset selects the direction automatically. Input containing only
Morse symbols and whitespace is decoded as Morse; both `.` and `·` are accepted
as dots, and both `-` and `_` as dashes. ASCII English letters and digits are
encoded as Morse. Encoded Morse uses one space between letters and two spaces
between words. When decoding, one space separates letters, two spaces separate
words, and three or more spaces create a paragraph break. Any whitespace run
containing a tab or newline also creates a paragraph break (`\n\n`). `/` remains
accepted as a word separator for compatibility. With no text argument, input is
read from stdin and an interactive terminal displays a prompt.

## Examination mode

Use `-e` or `--exam` with `e`/`encode` to practise encoding English and digits,
or `d`/`decode` to practise decoding Morse. The supplied text is the question;
the program then waits for an answer. It checks only the letters and digits, so
letter case and formatting do not affect a correct result. A correct answer
prints `Correct, consumed 0.000 s, rate: 0.00 sigs/min, 0.00 chars/min` (with
the measured values substituted). An incorrect answer prints `Incorrect,
expected:` followed by the expected answer on the next line.

Defaults are a 700 Hz sine wave and an 80 ms unit. Audio is written to a
timestamped path such as `audio/morse-1753334400000.ogg`; the `audio` directory
is created automatically. Dots last one unit and dashes three; gaps within a
letter, between letters, and between words last one, three, and seven units
respectively.

## Real-time keying mode

Run `-r`, `--rt`, or `--realtime` in an interactive terminal to decode Morse
as you key it. First select either single-key mode or double-key mode, then
select the key or keys. In single-key mode, a press up to the configured hold
time is a dot and a longer press is a dash. After selecting the key, enter that
hold time in milliseconds; leave it empty for the 50 ms default. `-u` controls
only the normal Morse gaps. In double-key mode, the
selected dot and dash keys enter their symbols directly. A pause of three
units ends a letter, and a pause of seven units starts a new word. The live
line shows the Morse sequence being entered and the decoded text. Press Escape
or Ctrl-C to end the session. Key events are read from the focused terminal,
not globally from the operating system. Single-key mode needs a terminal that
supports the kitty keyboard protocol (such as kitty, foot, WezTerm, or recent
Alacritty) because it depends on key-release events; double-key mode works in
normal terminals.
