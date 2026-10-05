//! Importing an existing deck by pasting it — Quizlet's export, an Anki CSV/TSV
//! export, or anything else that is one card per line.
//!
//! Entirely deterministic: no AI call, no cost, no latency. A deck you already
//! wrote does not need a model to read it, and routing it through the review
//! queue would be friction for content the student authored themselves — so
//! these become cards directly, with duplicates reported rather than created.

use serde::Serialize;

/// How the two halves of a card are separated. Quizlet's export defaults to a
/// tab between term and definition; people also pick comma, or " - ".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldSeparator {
    Tab,
    Comma,
    Semicolon,
    Dash,
}

impl FieldSeparator {
    fn as_pattern(self) -> &'static str {
        match self {
            FieldSeparator::Tab => "\t",
            FieldSeparator::Comma => ",",
            FieldSeparator::Semicolon => ";",
            FieldSeparator::Dash => " - ",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "tab" => Some(FieldSeparator::Tab),
            "comma" => Some(FieldSeparator::Comma),
            "semicolon" => Some(FieldSeparator::Semicolon),
            "dash" => Some(FieldSeparator::Dash),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedPair {
    pub front: String,
    pub back: String,
}

/// Picks the separator that yields the most two-sided lines.
///
/// Order matters on a tie: tab first because it is unambiguous and is what both
/// Quizlet and Anki export by default, and `" - "` last because a dash appears
/// inside ordinary prose far more often than a tab does.
pub fn detect_separator(text: &str) -> FieldSeparator {
    let candidates =
        [FieldSeparator::Tab, FieldSeparator::Comma, FieldSeparator::Semicolon, FieldSeparator::Dash];
    let mut best = FieldSeparator::Tab;
    let mut best_score = 0usize;
    for candidate in candidates {
        let score = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter(|line| split_once_outside_quotes(line, candidate).is_some())
            .count();
        if score > best_score {
            best_score = score;
            best = candidate;
        }
    }
    best
}

/// Splits on the first separator that is not inside double quotes, so a CSV
/// field like `"Boyle's law, simplified"` is not torn in half by its own comma.
fn split_once_outside_quotes(line: &str, sep: FieldSeparator) -> Option<(String, String)> {
    let pattern = sep.as_pattern();
    let bytes: Vec<char> = line.chars().collect();
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let mut in_quotes = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '"' {
            in_quotes = !in_quotes;
            i += 1;
            continue;
        }
        if !in_quotes && i + pattern_chars.len() <= bytes.len() && bytes[i..i + pattern_chars.len()] == pattern_chars[..]
        {
            let front: String = bytes[..i].iter().collect();
            let back: String = bytes[i + pattern_chars.len()..].iter().collect();
            return Some((front, back));
        }
        i += 1;
    }
    None
}

/// Removes the surrounding quotes a CSV export adds, and un-doubles the escaped
/// quotes inside.
fn unquote(field: &str) -> String {
    let trimmed = field.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1].replace("\"\"", "\"")
    } else {
        trimmed.to_string()
    }
}

/// Anki exports carry HTML by default (`<br>`, `<b>`, `&nbsp;`). Cards here are
/// rendered as plain text, so markup would show up literally on the card.
/// Deliberately a small, predictable strip rather than a real HTML parser: this
/// is export output, not arbitrary web content, and a parser would be a new
/// dependency to solve a problem of this size.
fn strip_html(input: &str) -> String {
    /// A tag that represents a line or block boundary becomes a space; an
    /// inline tag disappears. Dropping every tag outright fuses the words
    /// either side of a `<br>` — "and<br>a break" became "anda break".
    fn is_boundary(tag: &str) -> bool {
        let name = tag.trim_start_matches('/').split([' ', '/', '\t']).next().unwrap_or("").to_lowercase();
        matches!(
            name.as_str(),
            "br" | "p" | "div" | "li" | "tr" | "td" | "th" | "ul" | "ol" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        )
    }

    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for c in input.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                if is_boundary(&tag) {
                    out.push(' ');
                }
            }
            _ if in_tag => tag.push(c),
            _ => out.push(c),
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parses pasted text into front/back pairs.
///
/// Lines that do not contain the separator are skipped rather than failing the
/// import — an export often starts with a header or a stray blank line, and
/// losing the other 200 cards over it would be absurd.
pub fn parse_pairs(text: &str, separator: Option<FieldSeparator>) -> Vec<ParsedPair> {
    let separator = separator.unwrap_or_else(|| detect_separator(text));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| split_once_outside_quotes(line, separator))
        .map(|(front, back)| ParsedPair { front: strip_html(&unquote(&front)), back: strip_html(&unquote(&back)) })
        .filter(|pair| !pair.front.is_empty() && !pair.back.is_empty())
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPreview {
    pub separator: FieldSeparator,
    pub pairs: Vec<ParsedPair>,
    /// Lines that had no separator and were skipped.
    pub skipped_lines: usize,
}

pub fn preview(text: &str, separator: Option<FieldSeparator>) -> ImportPreview {
    let separator = separator.unwrap_or_else(|| detect_separator(text));
    let total = text.lines().filter(|l| !l.trim().is_empty()).count();
    let pairs = parse_pairs(text, Some(separator));
    ImportPreview { separator, skipped_lines: total.saturating_sub(pairs.len()), pairs }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_quizlet_tab_export() {
        let text = "mole\t6.022e23 particles\nmolar mass\tgrams per mole\n";
        let pairs = parse_pairs(text, None);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].front, "mole");
        assert_eq!(pairs[0].back, "6.022e23 particles");
        assert_eq!(pairs[1].front, "molar mass");
    }

    #[test]
    fn parses_a_comma_export_and_keeps_commas_inside_quoted_fields() {
        let text = "\"Boyle's law, simplified\",\"P and V are inversely proportional\"\nterm,definition\n";
        let pairs = parse_pairs(text, Some(FieldSeparator::Comma));
        assert_eq!(pairs.len(), 2);
        // The comma inside the quotes must not split the card.
        assert_eq!(pairs[0].front, "Boyle's law, simplified");
        assert_eq!(pairs[0].back, "P and V are inversely proportional");
        assert_eq!(pairs[1].front, "term");
    }

    #[test]
    fn parses_a_dash_separated_paste() {
        let text = "mitosis - cell division producing two identical cells\nmeiosis - division producing gametes\n";
        let pairs = parse_pairs(text, None);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].front, "mitosis");
        assert_eq!(pairs[0].back, "cell division producing two identical cells");
    }

    /// A dash inside an answer must not split it again — only the first
    /// separator counts.
    #[test]
    fn only_the_first_separator_splits_a_line() {
        let pairs = parse_pairs("term - part one - part two", Some(FieldSeparator::Dash));
        assert_eq!(pairs[0].front, "term");
        assert_eq!(pairs[0].back, "part one - part two");
    }

    #[test]
    fn detects_tab_in_preference_to_a_dash_that_appears_in_prose() {
        // Every line has a dash inside the definition, but the real separator
        // is the tab.
        let text = "osmosis\twater moves high - to - low concentration\ndiffusion\tparticles spread - evenly\n";
        assert_eq!(detect_separator(text), FieldSeparator::Tab);
        let pairs = parse_pairs(text, None);
        assert_eq!(pairs[0].front, "osmosis");
        assert_eq!(pairs[0].back, "water moves high - to - low concentration");
    }

    #[test]
    fn strips_the_html_an_anki_export_carries() {
        let text = "term\t<b>bold</b> and<br>a break&nbsp;here\n";
        let pairs = parse_pairs(text, None);
        assert_eq!(pairs[0].back, "bold and a break here");
        // An inline tag must NOT introduce a space, or every bolded word would
        // gain one; only a boundary tag does.
        assert_eq!(parse_pairs("a	un<b>frie</b>ndly
", None)[0].back, "unfriendly");
        assert_eq!(parse_pairs("a	one<p>two</p>three
", None)[0].back, "one two three");
        assert_eq!(parse_pairs("a	x<br/>y
", None)[0].back, "x y");
    }

    #[test]
    fn decodes_the_common_html_entities() {
        let pairs = parse_pairs("a\t&lt;tag&gt; &amp; &quot;quoted&quot; &#39;apostrophe&#39;\n", None);
        assert_eq!(pairs[0].back, "<tag> & \"quoted\" 'apostrophe'");
    }

    #[test]
    fn skips_lines_without_a_separator_rather_than_failing_the_import() {
        let text = "Front\tBack\na header line with no tab\nmole\t6.022e23\n\n   \n";
        let pairs = parse_pairs(text, Some(FieldSeparator::Tab));
        assert_eq!(pairs.len(), 2);
        let p = preview(text, Some(FieldSeparator::Tab));
        assert_eq!(p.skipped_lines, 1);
        assert_eq!(p.separator, FieldSeparator::Tab);
    }

    #[test]
    fn drops_pairs_with_an_empty_half() {
        let pairs = parse_pairs("good\tanswer\nempty back\t\n\tempty front\n", Some(FieldSeparator::Tab));
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].front, "good");
    }

    #[test]
    fn handles_crlf_line_endings() {
        let pairs = parse_pairs("a\tb\r\nc\td\r\n", Some(FieldSeparator::Tab));
        assert_eq!(pairs.len(), 2);
        // The \r must not survive into the card.
        assert_eq!(pairs[0].back, "b");
        assert_eq!(pairs[1].front, "c");
    }

    #[test]
    fn whitespace_around_fields_is_trimmed() {
        let pairs = parse_pairs("  spaced  \t  answer  \n", Some(FieldSeparator::Tab));
        assert_eq!(pairs[0].front, "spaced");
        assert_eq!(pairs[0].back, "answer");
    }

    #[test]
    fn an_empty_or_separator_less_paste_yields_nothing_rather_than_panicking() {
        assert!(parse_pairs("", None).is_empty());
        assert!(parse_pairs("   \n\n  ", None).is_empty());
        assert!(parse_pairs("just one long line with no separator at all", Some(FieldSeparator::Tab)).is_empty());
    }

    #[test]
    fn separator_names_round_trip() {
        for (name, expected) in [
            ("tab", FieldSeparator::Tab),
            ("Comma", FieldSeparator::Comma),
            ("SEMICOLON", FieldSeparator::Semicolon),
            (" dash ", FieldSeparator::Dash),
        ] {
            assert_eq!(FieldSeparator::parse(name), Some(expected));
        }
        assert_eq!(FieldSeparator::parse("pipe"), None);
    }

    #[test]
    fn nothing_panics_on_adversarial_input() {
        for text in ["\"", "\"\"\"", "\t\t\t", "a\t\"unclosed", "\u{0}\t\u{0}", &"x\ty\n".repeat(5000), "<<<>>>", "&amp;&amp;"] {
            let _ = preview(text, None);
            for sep in [FieldSeparator::Tab, FieldSeparator::Comma, FieldSeparator::Semicolon, FieldSeparator::Dash] {
                let _ = parse_pairs(text, Some(sep));
            }
        }
    }
}
