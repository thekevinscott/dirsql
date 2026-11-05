//! The named collating symbols bash accepts in a bracket (`[[.hyphen.]]`):
//! the POSIX portable-character names from bash's `collsyms.h`, so a name
//! means the same character whatever the locale.

const NAMES: &[(&str, char)] = &[
    ("NUL", '\0'),
    ("SOH", '\x01'),
    ("STX", '\x02'),
    ("ETX", '\x03'),
    ("EOT", '\x04'),
    ("ENQ", '\x05'),
    ("ACK", '\x06'),
    ("alert", '\x07'),
    ("BS", '\x08'),
    ("backspace", '\x08'),
    ("HT", '\t'),
    ("tab", '\t'),
    ("LF", '\n'),
    ("newline", '\n'),
    ("VT", '\x0b'),
    ("vertical-tab", '\x0b'),
    ("FF", '\x0c'),
    ("form-feed", '\x0c'),
    ("CR", '\r'),
    ("carriage-return", '\r'),
    ("SO", '\x0e'),
    ("SI", '\x0f'),
    ("DLE", '\x10'),
    ("DC1", '\x11'),
    ("DC2", '\x12'),
    ("DC3", '\x13'),
    ("DC4", '\x14'),
    ("NAK", '\x15'),
    ("SYN", '\x16'),
    ("ETB", '\x17'),
    ("CAN", '\x18'),
    ("EM", '\x19'),
    ("SUB", '\x1a'),
    ("ESC", '\x1b'),
    ("IS4", '\x1c'),
    ("FS", '\x1c'),
    ("IS3", '\x1d'),
    ("GS", '\x1d'),
    ("IS2", '\x1e'),
    ("RS", '\x1e'),
    ("IS1", '\x1f'),
    ("US", '\x1f'),
    ("space", ' '),
    ("exclamation-mark", '!'),
    ("quotation-mark", '"'),
    ("number-sign", '#'),
    ("dollar-sign", '$'),
    ("percent-sign", '%'),
    ("ampersand", '&'),
    ("apostrophe", '\''),
    ("left-parenthesis", '('),
    ("right-parenthesis", ')'),
    ("asterisk", '*'),
    ("plus-sign", '+'),
    ("comma", ','),
    ("hyphen", '-'),
    ("hyphen-minus", '-'),
    ("minus", '-'),
    ("dash", '-'),
    ("period", '.'),
    ("full-stop", '.'),
    ("slash", '/'),
    ("solidus", '/'),
    ("zero", '0'),
    ("one", '1'),
    ("two", '2'),
    ("three", '3'),
    ("four", '4'),
    ("five", '5'),
    ("six", '6'),
    ("seven", '7'),
    ("eight", '8'),
    ("nine", '9'),
    ("colon", ':'),
    ("semicolon", ';'),
    ("less-than-sign", '<'),
    ("equals-sign", '='),
    ("greater-than-sign", '>'),
    ("question-mark", '?'),
    ("commercial-at", '@'),
    ("left-square-bracket", '['),
    ("backslash", '\\'),
    ("reverse-solidus", '\\'),
    ("right-square-bracket", ']'),
    ("circumflex", '^'),
    ("circumflex-accent", '^'),
    ("underscore", '_'),
    ("grave-accent", '`'),
    ("left-brace", '{'),
    ("left-curly-bracket", '{'),
    ("vertical-line", '|'),
    ("right-brace", '}'),
    ("right-curly-bracket", '}'),
    ("tilde", '~'),
    ("DEL", '\x7f'),
];

/// The character a collating symbol's name stands for: a single character
/// names itself, a known name its character, and anything else nothing.
pub(crate) fn collating_symbol(name: &[char]) -> Option<char> {
    if let Some((_, c)) = NAMES
        .iter()
        .find(|(known, _)| known.chars().eq(name.iter().copied()))
    {
        return Some(*c);
    }
    match name {
        [c] => Some(*c),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(name: &str) -> Option<char> {
        collating_symbol(&name.chars().collect::<Vec<_>>())
    }

    #[test]
    fn a_single_character_names_itself() {
        assert_eq!(symbol("a"), Some('a'));
        assert_eq!(symbol("\u{e9}"), Some('\u{e9}'));
        assert_eq!(symbol("."), Some('.'));
    }

    #[test]
    fn a_portable_name_is_its_character() {
        assert_eq!(symbol("hyphen"), Some('-'));
        assert_eq!(symbol("NUL"), Some('\0'));
        assert_eq!(symbol("DEL"), Some('\x7f'));
        assert_eq!(symbol("right-square-bracket"), Some(']'));
    }

    #[test]
    fn names_are_case_sensitive() {
        assert_eq!(symbol("SPACE"), None);
        assert_eq!(symbol("nul"), None);
    }

    #[test]
    fn an_unknown_name_is_nothing() {
        assert_eq!(symbol("ab"), None);
        assert_eq!(symbol(""), None);
        assert_eq!(symbol("hyphe"), None);
    }
}
