//! Shared sanitization helpers for command and terminal text output.
//!
//! These helpers remove ANSI / OSC / DCS style escape sequences and apply a
//! few terminal control semantics so agent-facing surfaces receive readable
//! plain text instead of raw control bytes.

/// Strip ANSI-style escape/control sequences from terminal text for agent/UI use.
///
/// The sanitizer removes CSI/OSC/DCS/APC-style sequences, applies backspaces,
/// drops carriage returns, and preserves printable text plus `\n` / `\t`.
pub fn sanitize_terminal_output(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut output = String::with_capacity(raw.len());
    let mut index = 0;

    while index < chars.len() {
        match chars[index] {
            '\u{1b}' => {
                index += 1;
                if index >= chars.len() {
                    break;
                }

                match chars[index] {
                    '[' => {
                        index += 1;
                        while index < chars.len() {
                            let ch = chars[index];
                            index += 1;
                            if ('@'..='~').contains(&ch) {
                                break;
                            }
                        }
                    }
                    ']' => {
                        index += 1;
                        while index < chars.len() {
                            match chars[index] {
                                '\u{7}' => {
                                    index += 1;
                                    break;
                                }
                                '\u{1b}' if chars.get(index + 1).copied() == Some('\\') => {
                                    index += 2;
                                    break;
                                }
                                _ => {
                                    index += 1;
                                }
                            }
                        }
                    }
                    'P' | 'X' | '^' | '_' => {
                        index += 1;
                        while index < chars.len() {
                            if chars[index] == '\u{1b}'
                                && chars.get(index + 1).copied() == Some('\\')
                            {
                                index += 2;
                                break;
                            }
                            index += 1;
                        }
                    }
                    '(' | ')' | '*' | '+' | '-' | '.' | '/' => {
                        index += 2;
                    }
                    _ => {
                        index += 1;
                    }
                }
            }
            '\u{8}' => {
                output.pop();
                index += 1;
            }
            '\r' => {
                index += 1;
            }
            ch if ch.is_control() && ch != '\n' && ch != '\t' => {
                index += 1;
            }
            ch => {
                output.push(ch);
                index += 1;
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::sanitize_terminal_output;

    #[test]
    fn strips_ansi_and_osc_sequences() {
        let raw = "\u{1b}[0m\u{1b}[49mhello\u{1b}[39m\n\u{1b}]1;whoami\u{7}whoami\njorben\r\n";

        let sanitized = sanitize_terminal_output(raw);

        assert_eq!(sanitized, "hello\nwhoami\njorben\n");
    }

    #[test]
    fn applies_backspaces() {
        let raw = "abc\u{8}\u{8}Z\n";

        let sanitized = sanitize_terminal_output(raw);

        assert_eq!(sanitized, "aZ\n");
    }

    #[test]
    fn strips_dcs_and_charset_sequences() {
        let raw = "start\u{1b}P1$r0m\u{1b}\\mid\u{1b}(Bend";

        let sanitized = sanitize_terminal_output(raw);

        assert_eq!(sanitized, "startmidend");
    }
}
