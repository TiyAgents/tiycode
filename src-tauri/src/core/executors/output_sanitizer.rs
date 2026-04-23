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

    #[test]
    fn truncated_bare_esc_at_end() {
        // Input ending with a bare ESC
        let sanitized = sanitize_terminal_output("hello\u{1b}");
        assert_eq!(sanitized, "hello");
    }

    #[test]
    fn truncated_csi_at_end() {
        // ESC [ without a final byte
        let sanitized = sanitize_terminal_output("hello\u{1b}[31");
        assert_eq!(sanitized, "hello");
    }

    #[test]
    fn truncated_osc_at_end() {
        // ESC ] without BEL/ST terminator
        let sanitized = sanitize_terminal_output("hello\u{1b}]0;title");
        assert_eq!(sanitized, "hello");
    }

    #[test]
    fn truncated_dcs_at_end() {
        // ESC P without ST terminator
        let sanitized = sanitize_terminal_output("hello\u{1b}Pdata");
        assert_eq!(sanitized, "hello");
    }

    #[test]
    fn strips_apc_sequence() {
        // APC (ESC _) with ST terminator — used by iTerm2 image protocol
        let raw = "before\u{1b}_payload\u{1b}\\after";
        let sanitized = sanitize_terminal_output(raw);
        assert_eq!(sanitized, "beforeafter");
    }

    #[test]
    fn empty_input_returns_empty() {
        let sanitized = sanitize_terminal_output("");
        assert_eq!(sanitized, "");
    }

    #[test]
    fn pure_printable_passes_through() {
        let raw = "hello world\nline two\ttabbed";
        let sanitized = sanitize_terminal_output(raw);
        assert_eq!(sanitized, raw);
    }
}
