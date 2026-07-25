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
./target/release/morse-keyboard-debug
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
select the key or keys. `-u` selects the Morse time unit for all real-time
timing. In single-key mode, a hold at or below `sqrt(3) * unit` is a dot and a
longer hold is a dash; this is the geometric-mean boundary between a one-unit
dot and a three-unit dash. The original press timestamp is retained while a key
is held, so keyboard auto-repeat cannot shorten a long hold. In double-key
mode, the selected dot and dash keys enter their symbols directly. Pauses use
the same geometric-mean principle: `sqrt(3) * unit` separates intra-letter
from letter gaps, and `sqrt(21) * unit` separates letter from word gaps. The
live line shows the Morse sequence being entered and the decoded text. Press
Escape or Ctrl-C to end the session. Key events are read from the focused
terminal, not globally from the operating system. Single-key mode needs a
terminal that supports the kitty keyboard protocol (such as kitty, foot,
WezTerm, or recent Alacritty) because it depends on key-release events;
double-key mode works in normal terminals.

## Keyboard debugger

Build the package and run `morse-keyboard-debug` in the terminal where real-time
mode has trouble:

```sh
cargo build --release
./target/release/morse-keyboard-debug --unit 80
```

The debugger prints every parsed keyboard event with its event kind, key code,
modifiers, keyboard state, time since the previous event, and measured hold
duration. On release it applies the same `sqrt(3) * unit` threshold as
single-key real-time mode and prints `dit (.)` or `dah (-)`. It also warns about duplicate presses,
repeats without a press, releases without a press, and keys whose release was
never observed. Press Ctrl-C to exit.

Keyboard enhancement defaults to `auto`. Use `--enhancement force` to request
the kitty keyboard protocol when terminal support detection is wrong, or
`--enhancement off` to inspect the terminal's unenhanced event stream.
