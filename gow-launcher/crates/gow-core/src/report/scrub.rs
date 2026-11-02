//! Taking out of a text what identifies the player: the folders the launcher and the game live in, the user name and
//! the computer name. Applied to the copy that goes into a report, never to the files on disk.
//!
//! Folders are matched with `\` and `/` interchangeable (a run of them counts as one), with or without the `\\?\` prefix
//! and, on Windows, ignoring case. Names are matched as whole words. Everything is replaced in one pass, longest folder
//! first, so a placeholder is never scanned again.

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// One or more `\` or `/`.
    Separator,
    Char(char),
}

#[derive(Debug, Clone)]
struct Pattern {
    tokens: Vec<Token>,
    /// What the match is replaced with (`<HOME>`).
    placeholder: &'static str,
    /// A folder (`true`) or a user or computer name (`false`).
    folder: bool,
    /// Number of characters, for ordering.
    length: usize,
}

#[derive(Debug, Clone)]
pub struct Scrubber {
    patterns: Vec<Pattern>,
    ignore_case: bool,
}

fn is_separator(c: char) -> bool {
    c == '\\' || c == '/'
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric()
}

impl Scrubber {
    /// `ignore_case`: compare letters without regard to case (Windows file names).
    pub fn new(ignore_case: bool) -> Self {
        Self { patterns: Vec::new(), ignore_case }
    }

    fn fold(&self, c: char) -> char {
        if self.ignore_case {
            c.to_lowercase().next().unwrap_or(c)
        } else {
            c
        }
    }

    fn push(&mut self, text: &str, placeholder: &'static str, folder: bool) {
        let mut tokens = Vec::new();
        for c in text.chars() {
            if is_separator(c) {
                if tokens.last() != Some(&Token::Separator) {
                    tokens.push(Token::Separator);
                }
            } else {
                tokens.push(Token::Char(self.fold(c)));
            }
        }
        let length = text.chars().count();
        self.patterns.push(Pattern { tokens, placeholder, folder, length });
        self.patterns.sort_by(|a, b| b.folder.cmp(&a.folder).then(b.length.cmp(&a.length)));
    }

    /// Adds a folder. Too short to be meaningful (a drive letter, `/`) is ignored: it would wipe unrelated text.
    pub fn folder(mut self, folder: &str, placeholder: &'static str) -> Self {
        let trimmed = strip_verbatim(folder.trim()).trim_end_matches(is_separator);
        let drive_only = trimmed.len() == 2 && trimmed.ends_with(':');
        if trimmed.chars().count() >= 4 && !drive_only {
            self.push(trimmed, placeholder, true);
        }
        self
    }

    /// Adds a user or computer name, matched as a whole word. Names shorter than three characters are ignored: they
    /// would replace letters all over the text.
    pub fn name(mut self, name: &str, placeholder: &'static str) -> Self {
        let name = name.trim();
        if name.chars().count() >= 3 && name.chars().all(|c| is_word(c) || c == ' ' || c == '-' || c == '_' || c == '.')
        {
            self.push(name, placeholder, false);
        }
        self
    }

    fn matches_at(&self, pattern: &Pattern, text: &[char], start: usize) -> Option<usize> {
        let before = start.checked_sub(1).map(|i| text[i]);
        if pattern.folder {
            // A folder does not start in the middle of a word (`/app/tmp` is not `/tmp`).
            if before.is_some_and(|c| is_word(c) || c == '_') {
                return None;
            }
        } else if before.is_some_and(is_word) {
            return None;
        }
        let mut at = start;
        for token in &pattern.tokens {
            match token {
                Token::Separator => {
                    if !text.get(at).copied().is_some_and(is_separator) {
                        return None;
                    }
                    while text.get(at).copied().is_some_and(is_separator) {
                        at += 1;
                    }
                }
                Token::Char(expected) => {
                    if text.get(at).map(|c| self.fold(*c)) != Some(*expected) {
                        return None;
                    }
                    at += 1;
                }
            }
        }
        let after = text.get(at).copied();
        let continues =
            if pattern.folder { after.is_some_and(|c| is_word(c) || c == '_') } else { after.is_some_and(is_word) };
        (!continues).then_some(at)
    }

    pub fn scrub(&self, text: &str) -> String {
        if self.patterns.is_empty() {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        // First character of each pattern (`None`: it starts with a separator), to skip most positions with one comparison.
        let first: Vec<Option<char>> = self
            .patterns
            .iter()
            .map(|pattern| match pattern.tokens.first() {
                Some(Token::Char(c)) => Some(*c),
                _ => None,
            })
            .collect();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < chars.len() {
            let folded = self.fold(chars[i]);
            let hit = self.patterns.iter().enumerate().find_map(|(index, pattern)| {
                let starts_here = match first[index] {
                    Some(c) => c == folded,
                    None => is_separator(chars[i]),
                };
                if starts_here {
                    self.matches_at(pattern, &chars, i).map(|end| (end, pattern))
                } else {
                    None
                }
            });
            match hit {
                Some((end, pattern)) => {
                    if pattern.folder {
                        trim_verbatim_prefix(&mut out);
                    }
                    out.push_str(pattern.placeholder);
                    i = end;
                }
                None => {
                    out.push(chars[i]);
                    i += 1;
                }
            }
        }
        out
    }
}

/// `\\?\C:\x` -> `C:\x` (and the `//?/` spelling).
fn strip_verbatim(path: &str) -> &str {
    for prefix in ["\\\\?\\", "//?/"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            return rest;
        }
    }
    path
}

/// Removes a `\\?\` (any separators, even doubled ones) that ends `out`: what is left of a verbatim path whose folder was
/// just replaced.
fn trim_verbatim_prefix(out: &mut String) {
    let bytes = out.as_bytes();
    let separators_before = |end: usize| bytes[..end].iter().rev().take_while(|b| **b == b'\\' || **b == b'/').count();
    let mut end = bytes.len();
    let after = separators_before(end);
    if after == 0 {
        return;
    }
    end -= after;
    if end == 0 || bytes[end - 1] != b'?' {
        return;
    }
    end -= 1;
    let before = separators_before(end);
    if before >= 2 {
        out.truncate(end - before);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows() -> Scrubber {
        Scrubber::new(true)
            .folder("C:\\Users\\Mark", "<HOME>")
            .folder("C:\\Users\\Mark\\GodOfWar-PC", "<INSTALL>")
            .folder("C:\\Users\\Mark\\AppData\\Roaming\\GOW_PC\\Launcher", "<DATA>")
            .name("Mark", "<USER>")
            .name("DESKTOP-9K2", "<PC>")
    }

    #[test]
    fn replaces_home_user_and_pc_in_windows_paths() {
        let text = "open C:\\Users\\Mark\\Documents\\a.txt on DESKTOP-9K2 as Mark";
        assert_eq!(windows().scrub(text), "open <HOME>\\Documents\\a.txt on <PC> as <USER>");
    }

    #[test]
    fn mixed_slashes_case_and_the_verbatim_prefix_are_the_same_folder() {
        let scrubber = windows();
        assert_eq!(scrubber.scrub("c:/users/mark/x"), "<HOME>/x");
        assert_eq!(scrubber.scrub("C:\\Users/Mark\\x"), "<HOME>\\x");
        assert_eq!(scrubber.scrub("\\\\?\\C:\\Users\\Mark\\x"), "<HOME>\\x");
        assert_eq!(scrubber.scrub("//?/C:/Users/Mark/x"), "<HOME>/x");
        // Doubled backslashes, as a path looks inside an escaped string.
        assert_eq!(scrubber.scrub("C:\\\\Users\\\\Mark\\\\x"), "<HOME>\\\\x");
        assert_eq!(scrubber.scrub("\\\\\\\\?\\\\C:\\\\Users\\\\Mark\\\\x"), "<HOME>\\\\x");
    }

    #[test]
    fn the_longest_folder_wins_when_one_is_inside_another() {
        let text = "C:\\Users\\Mark\\GodOfWar-PC\\game and C:\\Users\\Mark\\AppData\\Roaming\\GOW_PC\\Launcher\\logs";
        assert_eq!(windows().scrub(text), "<INSTALL>\\game and <DATA>\\logs");
    }

    #[test]
    fn a_name_inside_a_longer_word_is_left_alone() {
        let scrubber = windows();
        assert_eq!(
            scrubber.scrub("Marketing, Markus and Mark2 stay; mark goes"),
            "Marketing, Markus and Mark2 stay; <USER> goes"
        );
        assert_eq!(scrubber.scrub("C:\\Users\\Markus\\x"), "C:\\Users\\Markus\\x");
        assert_eq!(scrubber.scrub("user=Mark."), "user=<USER>.");
    }

    #[test]
    fn linux_paths_are_case_sensitive_and_a_folder_never_starts_inside_a_word() {
        let scrubber =
            Scrubber::new(false).folder("/home/mark", "<HOME>").folder("/tmp", "<TEMP>").name("mark", "<USER>");
        assert_eq!(scrubber.scrub("/home/mark/.local/x /tmp/a /var/tmp/b"), "<HOME>/.local/x <TEMP>/a /var/tmp/b");
        assert_eq!(scrubber.scrub("/home/Mark/x"), "/home/Mark/x");
    }

    #[test]
    fn folders_that_would_wipe_the_text_are_ignored() {
        let scrubber =
            Scrubber::new(true).folder("C:\\", "<X>").folder("/", "<X>").folder("C:", "<X>").name("ab", "<N>");
        assert_eq!(scrubber.scrub("C:\\Windows /usr ab"), "C:\\Windows /usr ab");
    }

    #[test]
    fn placeholders_are_not_scanned_again() {
        let scrubber = Scrubber::new(true).folder("C:\\Users\\home", "<HOME>").name("home", "<USER>");
        assert_eq!(scrubber.scrub("C:\\Users\\home\\a and home"), "<HOME>\\a and <USER>");
    }

    #[test]
    fn accents_and_other_scripts_survive() {
        let scrubber = Scrubber::new(true).folder("C:\\Users\\José", "<HOME>").name("José", "<USER>");
        assert_eq!(scrubber.scrub("C:\\Users\\josé\\ação.txt, José e ação"), "<HOME>\\ação.txt, <USER> e ação");
    }

    #[test]
    fn nothing_to_replace_returns_the_text() {
        assert_eq!(Scrubber::new(true).scrub("a\nb"), "a\nb");
    }
}
