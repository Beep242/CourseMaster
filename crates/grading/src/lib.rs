//! Deterministic answer grading: decide whether a typed answer is right
//! *without* spending money.
//!
//! Pure and dependency-free (serde only), like `crates/scheduler` and
//! `crates/srs`. No DB, no HTTP, no AI.
//!
//! # Why this exists
//!
//! Every AI call in this app shells out to `claude -p`: up to 180 seconds, a
//! real dollar cost, and a hard per-call budget cap. The existing practice-test
//! grader sent **every** short answer through that, including answers that were
//! character-for-character identical to the model answer. For a study session
//! answering dozens of cards, that is the difference between instant and
//! unusable.
//!
//! So grading is a ladder, cheapest rung first:
//!
//! 1. normalise and compare,
//! 2. compare as numbers, with a unit check,
//! 3. allow a typo budget scaled to the answer's length,
//! 4. and only then return [`Verdict::Undecided`] — the single case where an
//!    AI adjudication or a self-grade prompt is warranted.
//!
//! `Undecided` is deliberately *not* a synonym for "probably wrong". A
//! one-word answer that does not match is simply [`Verdict::Incorrect`]: short
//! factual answers are checkable, and asking a model to confirm that "mitosis"
//! is not "meiosis" wastes money to arrive at the obvious. `Undecided` is for
//! genuine phrasing ambiguity — a multi-word free-text answer, or a number that
//! matches while its unit does not.

use serde::{Deserialize, Serialize};

/// Longest answer pair this will attempt an edit-distance comparison on. The
/// matrix is O(n·m), and beyond essay length the result is meaningless anyway —
/// those fall through to the token-overlap rule instead.
const MAX_EDIT_DISTANCE_LEN: usize = 512;

/// Fraction of an accepted answer's words that must also appear in the
/// submission before a non-match is treated as ambiguous rather than wrong.
const OVERLAP_UNDECIDED_RATIO: f64 = 0.6;

/// An accepted answer of at least this many words is free text, where correct
/// answers legitimately differ in wording, so a miss is ambiguous not wrong.
const FREE_TEXT_WORD_COUNT: usize = 3;

/// Relative tolerance for comparing two numbers, so `0.333333` and `1/3` agree
/// and float formatting differences never decide a grade.
const NUMERIC_RELATIVE_TOLERANCE: f64 = 1e-6;
const NUMERIC_ABSOLUTE_TOLERANCE: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Correct,
    Incorrect,
    /// Cannot be decided without judgment. The *only* verdict that justifies
    /// an AI call or a "grade this yourself" prompt.
    Undecided,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Correct => "correct",
            Verdict::Incorrect => "incorrect",
            Verdict::Undecided => "undecided",
        }
    }

    pub fn is_correct(&self) -> bool {
        *self == Verdict::Correct
    }

    pub fn needs_judgment(&self) -> bool {
        *self == Verdict::Undecided
    }
}

/// Folds away everything that should never decide a grade: case, surrounding
/// and internal punctuation, repeated whitespace, and the typographic
/// characters word processors and PDFs substitute silently (curly quotes, en
/// and em dashes, non-breaking spaces).
///
/// `.`, `-`, `%` and `+` survive only when adjacent to a digit, so `3.14`,
/// `50%` and `-5` keep their meaning while `well-known` and `e.g.` lose
/// separators that carry none. `/` survives next to any alphanumeric, which is
/// what keeps both `1/2` and a compound unit like `m/s2` intact.
///
/// Commas, apostrophes and quote marks are *removed* rather than replaced with
/// a space, because they sit inside a word: that is what makes `1,000` equal
/// `1000` and `it's` equal `its`. Every other separator becomes a space, so it
/// splits words rather than fusing them.
///
/// Deliberately does **not** fold diacritics: doing it properly needs Unicode
/// normalisation tables, and `café`/`cafe` is better handled by the typo budget
/// than by a hand-rolled character map that would be wrong for other languages.
pub fn normalize(input: &str) -> String {
    let mapped: String = input
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{201B}' | '\u{02BC}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201F}' => '"',
            '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
            '\u{00A0}' | '\u{2007}' | '\u{202F}' | '\u{2009}' => ' ',
            other => other,
        })
        .flat_map(|c| c.to_lowercase())
        .collect();

    let chars: Vec<char> = mapped.chars().collect();
    let mut kept = String::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        if c.is_alphanumeric() {
            kept.push(c);
        } else if c.is_whitespace() {
            kept.push(' ');
        } else if matches!(c, '.' | '-' | '%' | '+') {
            let prev_digit = i > 0 && chars[i - 1].is_ascii_digit();
            let next_digit = chars.get(i + 1).is_some_and(|n| n.is_ascii_digit());
            if prev_digit || next_digit {
                kept.push(c);
            } else {
                kept.push(' ');
            }
        } else if c == '/' {
            // Next to any alphanumeric, not just a digit, so a compound unit
            // (`m/s2`, `mol/l`) stays one token the way `1/2` does.
            let prev_alnum = i > 0 && chars[i - 1].is_alphanumeric();
            let next_alnum = chars.get(i + 1).is_some_and(|n| n.is_alphanumeric());
            if prev_alnum && next_alnum {
                kept.push(c);
            } else {
                kept.push(' ');
            }
        } else if matches!(c, ',' | '\'' | '"') {
            // Removed, not spaced: these sit inside a word, so replacing them
            // would split `1,000` and `it's` into two tokens each.
        } else {
            kept.push(' ');
        }
    }

    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How many single-character edits to forgive, scaled to the answer's length.
/// Short answers get none — at three characters or fewer, one edit is usually a
/// different word (`ion`/`ions` is fine, but `cat`/`bat` must not be).
pub fn typo_budget(accepted_len: usize) -> usize {
    match accepted_len {
        0..=3 => 0,
        4..=8 => 1,
        9..=15 => 2,
        n => n / 8,
    }
}

/// Levenshtein distance, two rows rather than a full matrix.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// A number with whatever unit trailed it: `5 Ω`, `9.8 m/s2`, `50%`, `1/3`.
#[derive(Debug, Clone, PartialEq)]
pub struct Numeric {
    pub value: f64,
    /// Normalised and possibly empty.
    pub unit: String,
}

/// Reads a leading number (optionally a simple `a/b` fraction) plus a trailing
/// unit out of an already-normalised string. Returns `None` when there is no
/// number to compare, which is most answers.
pub fn parse_numeric(normalized: &str) -> Option<Numeric> {
    let s = normalized.trim();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    if matches!(chars.first(), Some('+') | Some('-')) {
        i += 1;
    }
    let digits_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == digits_start {
        return None;
    }
    // Scientific notation, but only when it really is one (`1e3`, not `5 each`).
    if i < chars.len() && (chars[i] == 'e') {
        let mut j = i + 1;
        if matches!(chars.get(j), Some('+') | Some('-')) {
            j += 1;
        }
        let exp_start = j;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
        }
    }

    let head: String = chars[..i].iter().collect();
    let mut value: f64 = head.parse().ok()?;
    let mut rest: String = chars[i..].iter().collect();

    // A fraction, where the denominator runs straight into the slash.
    if let Some(after) = rest.strip_prefix('/') {
        let denom_digits: String = after.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        if let Ok(denom) = denom_digits.parse::<f64>() {
            if denom != 0.0 {
                value /= denom;
                rest = after[denom_digits.len()..].to_string();
            }
        }
    }

    // Longhand scientific notation: "6.022 x 10^23", "6.022 × 10 23", "3*10^8".
    // Students write powers of ten this way constantly in chemistry and
    // physics, and without this `6.022 x 10^23` compares as the number 6.022
    // and is marked wrong against `6.022e23` — a correct answer rejected.
    // (`normalize` has already turned `^` into a space by this point.)
    if let Some(exponent) = parse_power_of_ten_suffix(&rest) {
        value *= 10f64.powi(exponent);
        rest = String::new();
    }

    if !value.is_finite() {
        return None;
    }
    Some(Numeric { value, unit: rest.split_whitespace().collect::<Vec<_>>().join(" ") })
}

/// Reads a trailing "× 10^n" and returns `n`. Returns `None` unless the whole
/// remainder is that construction, so a genuine unit like "x 10 apples" is not
/// mistaken for an exponent.
fn parse_power_of_ten_suffix(rest: &str) -> Option<i32> {
    let mut tokens: Vec<String> = rest.split_whitespace().map(str::to_string).collect();

    // The multiplication sign is OPTIONAL, because `normalize` has usually
    // already removed it: `×` and `*` are not alphanumeric so they become
    // spaces, leaving "6.022 × 10^23" as "6.022 10 23". Only a literal `x`
    // survives, since it is a letter. So both "x 10 23" and "10 23" have to be
    // recognised, and requiring the sign would have missed the `×` form —
    // which is the one a chemistry student is most likely to type.
    if !tokens.is_empty() {
        if matches!(tokens[0].as_str(), "x" | "×" | "*") {
            tokens.remove(0);
        } else if let Some(stripped) = tokens[0].strip_prefix(['x', '×', '*']).map(str::to_string) {
            if stripped.is_empty() {
                tokens.remove(0);
            } else {
                tokens[0] = stripped;
            }
        }
    }

    // Exactly "10" followed by the exponent and nothing else, so a real unit
    // like "x 10 apples" is not mistaken for a power.
    if tokens.len() != 2 || tokens[0] != "10" {
        return None;
    }
    tokens[1].parse::<i32>().ok()
}

#[derive(Debug, PartialEq)]
enum NumericOutcome {
    Equal,
    /// Values agree but only one side carried a unit — a judgment call.
    UnitUnclear,
    Different,
}

fn compare_numeric(submitted: &Numeric, accepted: &Numeric) -> NumericOutcome {
    let tolerance = (NUMERIC_RELATIVE_TOLERANCE * submitted.value.abs().max(accepted.value.abs()))
        .max(NUMERIC_ABSOLUTE_TOLERANCE);
    if (submitted.value - accepted.value).abs() > tolerance {
        return NumericOutcome::Different;
    }
    match (submitted.unit.is_empty(), accepted.unit.is_empty()) {
        (true, true) => NumericOutcome::Equal,
        (false, false) if submitted.unit == accepted.unit => NumericOutcome::Equal,
        // Right number, and either a missing unit or a different one. Both are
        // for a human or a model to call, not for this function.
        _ => NumericOutcome::UnitUnclear,
    }
}

fn word_overlap_ratio(submitted: &str, accepted: &str) -> f64 {
    let accepted_words: Vec<&str> = accepted.split(' ').filter(|w| !w.is_empty()).collect();
    if accepted_words.is_empty() {
        return 0.0;
    }
    let submitted_words: Vec<&str> = submitted.split(' ').filter(|w| !w.is_empty()).collect();
    let hits = accepted_words.iter().filter(|w| submitted_words.contains(w)).count();
    hits as f64 / accepted_words.len() as f64
}

/// Grades a typed answer against one or more acceptable answers.
///
/// Any single accepted answer matching is enough. See the module docs for why
/// a short non-match is `Incorrect` while a free-text one is `Undecided`.
pub fn grade_short_answer(submitted: &str, accepted: &[&str]) -> Verdict {
    let sub = normalize(submitted);
    if sub.is_empty() {
        return Verdict::Incorrect;
    }
    let sub_numeric = parse_numeric(&sub);
    let mut fallback = Verdict::Incorrect;

    for candidate in accepted {
        let acc = normalize(candidate);
        if acc.is_empty() {
            continue;
        }
        if sub == acc {
            return Verdict::Correct;
        }

        if let (Some(ns), Some(na)) = (sub_numeric.as_ref(), parse_numeric(&acc)) {
            match compare_numeric(ns, &na) {
                NumericOutcome::Equal => return Verdict::Correct,
                NumericOutcome::UnitUnclear => fallback = Verdict::Undecided,
                // A wrong number is decisively wrong; do not let the typo
                // budget rescue `42` answered against `24`.
                NumericOutcome::Different => {}
            }
            continue;
        }

        let acc_len = acc.chars().count();
        if acc_len <= MAX_EDIT_DISTANCE_LEN && sub.chars().count() <= MAX_EDIT_DISTANCE_LEN {
            let budget = typo_budget(acc_len);
            if budget > 0 && levenshtein(&sub, &acc) <= budget {
                return Verdict::Correct;
            }
        }

        let is_free_text = acc.split(' ').filter(|w| !w.is_empty()).count() >= FREE_TEXT_WORD_COUNT;
        if is_free_text || word_overlap_ratio(&sub, &acc) >= OVERLAP_UNDECIDED_RATIO {
            fallback = Verdict::Undecided;
        }
    }

    fallback
}

/// Grades a multiple-choice selection against the stored option text.
///
/// Always decisive: the student picked from a fixed list, so there is no
/// phrasing to interpret and never anything for a model to judge.
pub fn grade_choice(submitted: &str, correct: &str) -> Verdict {
    let sub = normalize(submitted);
    if sub.is_empty() {
        return Verdict::Incorrect;
    }
    if sub == normalize(correct) {
        Verdict::Correct
    } else {
        Verdict::Incorrect
    }
}

/// Reads the many ways a true/false answer gets written. `None` for anything
/// that is not recognisably one or the other.
pub fn parse_boolean(input: &str) -> Option<bool> {
    match normalize(input).as_str() {
        "true" | "t" | "yes" | "y" | "correct" | "1" => Some(true),
        "false" | "f" | "no" | "n" | "incorrect" | "0" => Some(false),
        _ => None,
    }
}

/// Grades a true/false answer. Also decisive — and tolerant of `T`, `yes`, `1`
/// and friends, since the stored answer and the submitted one can easily be
/// written differently while meaning the same thing.
pub fn grade_true_false(submitted: &str, correct: &str) -> Verdict {
    match (parse_boolean(submitted), parse_boolean(correct)) {
        (Some(s), Some(c)) if s == c => Verdict::Correct,
        (Some(_), Some(_)) => Verdict::Incorrect,
        // An unparseable stored answer is a data problem, not the student's
        // fault — fall back to comparing the text rather than marking it wrong.
        _ => grade_choice(submitted, correct),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_folds_case_and_collapses_whitespace() {
        assert_eq!(normalize("  The   Mitochondrion  "), "the mitochondrion");
        assert_eq!(normalize("ATP"), "atp");
        assert_eq!(normalize("\ta\n\nb  "), "a b");
    }

    #[test]
    fn normalize_drops_punctuation_that_should_never_decide_a_grade() {
        assert_eq!(normalize("mitosis."), "mitosis");
        assert_eq!(normalize("it's"), "its");
        assert_eq!(normalize("(photosynthesis)"), "photosynthesis");
        assert_eq!(normalize("e.g. this"), "e g this");
        assert_eq!(normalize("well-known"), "well known");
    }

    #[test]
    fn normalize_keeps_punctuation_that_carries_numeric_meaning() {
        assert_eq!(normalize("3.14"), "3.14");
        assert_eq!(normalize("1/2"), "1/2");
        assert_eq!(normalize("50%"), "50%");
        assert_eq!(normalize("-5"), "-5");
        assert_eq!(normalize("2+3"), "2+3");
    }

    /// Thousands separators are the reason commas are dropped rather than
    /// turned into spaces.
    #[test]
    fn normalize_makes_thousands_separators_irrelevant() {
        assert_eq!(normalize("1,000"), normalize("1000"));
        assert_eq!(normalize("1,234,567"), "1234567");
    }

    /// Word processors and PDFs substitute these silently, and a student
    /// pasting from lecture notes should not be marked wrong for it.
    #[test]
    fn normalize_folds_typographic_substitutions() {
        assert_eq!(normalize("don\u{2019}t"), normalize("don't"));
        assert_eq!(normalize("a\u{2014}b"), normalize("a-b"));
        assert_eq!(normalize("5\u{00A0}volts"), "5 volts");
        assert_eq!(normalize("\u{201C}quoted\u{201D}"), "quoted");
    }

    #[test]
    fn levenshtein_is_a_metric_on_the_obvious_cases() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("flaw", "lawn"), 2);
        // Symmetric.
        assert_eq!(levenshtein("mitosis", "mitossis"), levenshtein("mitossis", "mitosis"));
        // Multi-byte characters count as one edit, not as their byte length.
        assert_eq!(levenshtein("Ω", "O"), 1);
        assert_eq!(levenshtein("café", "cafe"), 1);
    }

    #[test]
    fn exact_answers_are_correct_regardless_of_formatting() {
        assert_eq!(grade_short_answer("Mitosis", &["mitosis"]), Verdict::Correct);
        assert_eq!(grade_short_answer("  the Krebs cycle.  ", &["The Krebs Cycle"]), Verdict::Correct);
    }

    #[test]
    fn any_one_of_several_accepted_answers_is_enough() {
        let accepted = ["krebs cycle", "citric acid cycle", "tca cycle"];
        assert_eq!(grade_short_answer("TCA cycle", &accepted), Verdict::Correct);
        assert_eq!(grade_short_answer("Citric Acid Cycle", &accepted), Verdict::Correct);
    }

    #[test]
    fn a_typo_inside_the_budget_is_still_correct() {
        // 9 chars -> budget 2.
        assert_eq!(grade_short_answer("mitochondira", &["mitochondria"]), Verdict::Correct);
        assert_eq!(grade_short_answer("photosynthesys", &["photosynthesis"]), Verdict::Correct);
    }

    /// The budget is zero for very short answers, because at that length one
    /// edit is usually a different word rather than a slip.
    #[test]
    fn short_near_misses_are_wrong_not_forgiven() {
        assert_eq!(typo_budget(3), 0);
        assert_eq!(grade_short_answer("cat", &["bat"]), Verdict::Incorrect);
        assert_eq!(grade_short_answer("ion", &["eon"]), Verdict::Incorrect);
    }

    /// The case that would otherwise waste an AI call to reach the obvious.
    #[test]
    fn a_plainly_different_one_word_answer_is_decisively_incorrect() {
        assert_eq!(grade_short_answer("meiosis", &["mitosis"]), Verdict::Incorrect);
        assert_eq!(grade_short_answer("nucleus", &["ribosome"]), Verdict::Incorrect);
    }

    #[test]
    fn numbers_compare_by_value_not_by_spelling() {
        assert_eq!(grade_short_answer("5", &["5.0"]), Verdict::Correct);
        assert_eq!(grade_short_answer("5.00", &["5"]), Verdict::Correct);
        assert_eq!(grade_short_answer("1000", &["1,000"]), Verdict::Correct);
        assert_eq!(grade_short_answer("1e3", &["1000"]), Verdict::Correct);
        assert_eq!(grade_short_answer("0.5", &["1/2"]), Verdict::Correct);
        assert_eq!(grade_short_answer("-5", &["-5.0"]), Verdict::Correct);
    }

    /// Students write powers of ten longhand constantly; marking that wrong
    /// against the same value in `e` notation rejects a correct answer.
    #[test]
    fn longhand_scientific_notation_equals_e_notation() {
        assert_eq!(grade_short_answer("6.022 x 10^23", &["6.022e23"]), Verdict::Correct);
        assert_eq!(grade_short_answer("6.022 \u{d7} 10^23", &["6.022e23"]), Verdict::Correct);
        assert_eq!(grade_short_answer("3 x 10^8", &["3e8"]), Verdict::Correct);
        assert_eq!(grade_short_answer("3*10^8", &["300000000"]), Verdict::Correct);
        assert_eq!(grade_short_answer("1.6 x 10^-19", &["1.6e-19"]), Verdict::Correct);
        // And it still catches a genuinely wrong power.
        assert_eq!(grade_short_answer("6.022 x 10^24", &["6.022e23"]), Verdict::Incorrect);
    }

    /// The suffix must not swallow a real unit that happens to start with x.
    #[test]
    fn a_trailing_unit_is_not_mistaken_for_an_exponent() {
        let n = parse_numeric(&normalize("5 x 10 apples")).unwrap();
        assert_eq!(n.value, 5.0);
        assert!(!n.unit.is_empty());
        let m = parse_numeric(&normalize("5 xenon")).unwrap();
        assert_eq!(m.value, 5.0);
        assert_eq!(m.unit, "xenon");
    }

    #[test]
    fn a_wrong_number_is_never_rescued_by_the_typo_budget() {
        // Edit distance 2 on a 2-char answer, but 42 is simply not 24.
        assert_eq!(grade_short_answer("24", &["42"]), Verdict::Incorrect);
        assert_eq!(grade_short_answer("100", &["1000"]), Verdict::Incorrect);
    }

    #[test]
    fn matching_units_are_correct_and_matching_values_with_odd_units_are_undecided() {
        assert_eq!(grade_short_answer("5 ohms", &["5 ohms"]), Verdict::Correct);
        assert_eq!(grade_short_answer("9.8 m/s2", &["9.8 m/s2"]), Verdict::Correct);
        // Right value, no unit given: a judgment call, not a free pass.
        assert_eq!(grade_short_answer("5", &["5 ohms"]), Verdict::Undecided);
        // Right value, different unit: also a judgment call.
        assert_eq!(grade_short_answer("5 volts", &["5 ohms"]), Verdict::Undecided);
    }

    /// The exact shape of a bug from an earlier session: a resistance answer
    /// written with the ohm sign.
    #[test]
    fn the_ohm_sign_round_trips() {
        assert_eq!(grade_short_answer("5 \u{3A9}", &["5 \u{3A9}"]), Verdict::Correct);
        assert_eq!(grade_short_answer("5\u{3A9}", &["5 \u{3A9}"]), Verdict::Correct);
    }

    #[test]
    fn free_text_answers_that_do_not_match_are_undecided_not_wrong() {
        let accepted = ["it increases the surface area available for gas exchange"];
        assert_eq!(
            grade_short_answer("more surface area means more gas can be exchanged", &accepted),
            Verdict::Undecided
        );
    }

    #[test]
    fn a_mostly_overlapping_answer_is_undecided() {
        assert_eq!(grade_short_answer("the krebs cycle happens", &["the krebs cycle"]), Verdict::Undecided);
    }

    #[test]
    fn an_empty_or_whitespace_answer_is_incorrect_without_any_judgment() {
        for empty in ["", "   ", "\t\n", "..."] {
            assert_eq!(grade_short_answer(empty, &["mitosis"]), Verdict::Incorrect, "input {empty:?}");
            assert_eq!(grade_choice(empty, "a"), Verdict::Incorrect);
        }
    }

    #[test]
    fn multiple_choice_is_always_decisive() {
        assert_eq!(grade_choice("Paris", &"paris"), Verdict::Correct);
        assert_eq!(grade_choice("  PARIS. ", "Paris"), Verdict::Correct);
        assert_eq!(grade_choice("London", "Paris"), Verdict::Incorrect);
        // Never Undecided: the student picked from a fixed list.
        for (a, b) in [("a", "b"), ("", "b"), ("a long option", "another long option")] {
            assert_ne!(grade_choice(a, b), Verdict::Undecided);
        }
    }

    #[test]
    fn true_false_accepts_the_many_ways_people_write_it() {
        for yes in ["true", "True", "T", "yes", "Y", "1", "correct"] {
            assert_eq!(grade_true_false(yes, "true"), Verdict::Correct, "input {yes:?}");
            assert_eq!(grade_true_false(yes, "false"), Verdict::Incorrect, "input {yes:?}");
        }
        for no in ["false", "FALSE", "f", "no", "N", "0", "incorrect"] {
            assert_eq!(grade_true_false(no, "false"), Verdict::Correct, "input {no:?}");
            assert_eq!(grade_true_false(no, "true"), Verdict::Incorrect, "input {no:?}");
        }
    }

    /// A stored answer that is not recognisably boolean is a data problem, and
    /// the student should not eat the failure.
    #[test]
    fn an_unparseable_stored_true_false_answer_falls_back_to_text_comparison() {
        assert_eq!(grade_true_false("both", "both"), Verdict::Correct);
        assert_eq!(grade_true_false("neither", "both"), Verdict::Incorrect);
    }

    #[test]
    fn no_accepted_answers_at_all_is_incorrect_rather_than_a_panic() {
        assert_eq!(grade_short_answer("anything", &[]), Verdict::Incorrect);
        assert_eq!(grade_short_answer("anything", &["", "  "]), Verdict::Incorrect);
    }

    /// The contract the callers rely on: never panic, whatever comes in.
    #[test]
    fn nothing_panics_on_adversarial_input() {
        let nasty = [
            "",
            " ",
            "\0",
            "🎓🎓🎓",
            "∫ƒ(x)dx = Ω · 5 — naïve café",
            "a\u{0301}\u{0301}\u{0301}",
            &"x".repeat(5000),
            "1/0",
            "-",
            ".",
            "e5",
            "1e",
            "+",
            "1e999999",
            "NaN",
            "inf",
        ];
        for a in nasty {
            for b in nasty {
                let _ = grade_short_answer(a, &[b]);
                let _ = grade_choice(a, b);
                let _ = grade_true_false(a, b);
                let _ = normalize(a);
                let _ = parse_numeric(&normalize(a));
            }
        }
    }

    #[test]
    fn division_by_zero_and_overflow_do_not_produce_a_match() {
        // 1/0 must not become infinity and then compare equal to something.
        let parsed = parse_numeric(&normalize("1/0"));
        assert!(parsed.is_some_and(|n| n.value.is_finite()));
        assert_eq!(grade_short_answer("1e999999", &["5"]), Verdict::Incorrect);
    }

    #[test]
    fn parse_numeric_returns_none_when_there_is_no_number() {
        assert!(parse_numeric("mitosis").is_none());
        assert!(parse_numeric("").is_none());
        assert!(parse_numeric("-").is_none());
        assert!(parse_numeric("e5").is_none());
    }

    #[test]
    fn parse_numeric_splits_value_from_unit() {
        let n = parse_numeric(&normalize("9.8 m/s2")).unwrap();
        assert!((n.value - 9.8).abs() < 1e-12);
        assert_eq!(n.unit, "m/s2");
        let bare = parse_numeric("42").unwrap();
        assert_eq!(bare.value, 42.0);
        assert_eq!(bare.unit, "");
    }

    #[test]
    fn verdict_helpers_agree_with_the_variants() {
        assert!(Verdict::Correct.is_correct());
        assert!(!Verdict::Undecided.is_correct());
        assert!(!Verdict::Incorrect.is_correct());
        assert!(Verdict::Undecided.needs_judgment());
        assert!(!Verdict::Correct.needs_judgment());
        assert!(!Verdict::Incorrect.needs_judgment());
        assert_eq!(Verdict::Correct.as_str(), "correct");
    }
}
