//! Shared TUI utilities.

/// Strip ANSI escape sequences from a string.
///
/// Handles CSI sequences (`ESC[...X`), OSC sequences (`ESC]...ST`),
/// and simple two-byte sequences (`ESC X`).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    // CSI sequence: ESC [ ... <final byte 0x40–0x7E>
                    chars.next();
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch.is_ascii() && (0x40..=0x7E).contains(&(ch as u8)) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC sequence: ESC ] ... (ST = ESC \ or BEL)
                    chars.next();
                    while let Some(&ch) = chars.peek() {
                        if ch == '\x07' {
                            chars.next();
                            break;
                        }
                        if ch == '\x1b' {
                            chars.next();
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                        chars.next();
                    }
                }
                Some(_) => {
                    // Two-byte escape (e.g., ESC M, ESC 7, etc.)
                    chars.next();
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_unchanged() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn strips_reset_code() {
        assert_eq!(strip_ansi("hello\x1b[0m world"), "hello world");
    }

    #[test]
    fn strips_color_codes() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn strips_bold_and_color() {
        assert_eq!(strip_ansi("\x1b[1;34mbold blue\x1b[0m"), "bold blue");
    }

    #[test]
    fn strips_multiple_sequences() {
        assert_eq!(
            strip_ansi("\x1b[32mgreen\x1b[0m and \x1b[33myellow\x1b[0m"),
            "green and yellow"
        );
    }

    #[test]
    fn empty_string() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn only_escape_codes() {
        assert_eq!(strip_ansi("\x1b[0m\x1b[1m\x1b[31m"), "");
    }
}
