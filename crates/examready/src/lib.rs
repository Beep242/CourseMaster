//! How ready are you for this exam?
//!
//! A pure, dependency-free crate like `scheduler`, `srs` and `grading`. The
//! point is that the number is **explainable**: it returns the reason alongside
//! the score, and the reason names the component that is actually holding it
//! back. A readiness score nobody can interrogate is a vibe with a percent sign,
//! and a student would be right to ignore it.
//!
//! Modelled on `crates/scheduler`'s weighted-sum style for exactly that reason:
//! every weight is a named constant, every component is a 0–1 factor, and the
//! arithmetic is simple enough to check by hand.

use serde::{Deserialize, Serialize};

/// Coverage dominates. Knowing 100% of the third of the deck you have seen is
/// not being ready; it is being ready for a third of the exam.
pub const COVERAGE_WEIGHT: f64 = 0.45;
/// How much of the material has reached a durable interval.
pub const RETENTION_WEIGHT: f64 = 0.35;
/// How often you have been right lately.
pub const ACCURACY_WEIGHT: f64 = 0.20;

/// Below this many reviews, accuracy is noise and is not allowed to dominate
/// the score in either direction.
pub const MIN_REVIEWS_FOR_ACCURACY: i64 = 10;

/// An exam closer than this starts to compress the score: there is a limit to
/// how ready you can get by Thursday when you have seen a fifth of the deck.
pub const URGENCY_HORIZON_DAYS: i64 = 14;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ReadinessInput {
    pub total_cards: i64,
    /// Cards reviewed at least once.
    pub seen_cards: i64,
    /// Cards at or past the maturity interval.
    pub mature_cards: i64,
    pub total_reviews: i64,
    pub correct_reviews: i64,
    /// `None` when no exam is scheduled, in which case urgency is not applied.
    pub days_until_exam: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Readiness {
    /// 0–100, rounded to a whole number. A percentage of a model, not a
    /// prediction of a grade.
    pub score: i64,
    /// Plain-language, naming the weakest component.
    pub reason: String,
    pub coverage: f64,
    pub retention: f64,
    pub accuracy: f64,
    /// False when there is not enough history for the number to mean anything.
    pub has_enough_data: bool,
}

fn ratio(numerator: i64, denominator: i64) -> f64 {
    if denominator <= 0 {
        return 0.0;
    }
    (numerator.max(0) as f64 / denominator as f64).clamp(0.0, 1.0)
}

/// Scores readiness and says what is holding it back.
///
/// Total and panic-free: every division is guarded and every input is clamped,
/// because this runs on whatever happens to be in the database.
pub fn assess(input: ReadinessInput) -> Readiness {
    let coverage = ratio(input.seen_cards, input.total_cards);
    let retention = ratio(input.mature_cards, input.total_cards);

    // Accuracy starts neutral rather than at zero: a deck with two reviews
    // should not read as 0% accurate, nor as 100% after one lucky answer.
    let measured = ratio(input.correct_reviews, input.total_reviews);
    let enough = input.total_reviews >= MIN_REVIEWS_FOR_ACCURACY;
    let accuracy = if enough {
        measured
    } else if input.total_reviews == 0 {
        0.0
    } else {
        // Blend toward 0.5 in proportion to how little evidence there is.
        let confidence = input.total_reviews as f64 / MIN_REVIEWS_FOR_ACCURACY as f64;
        measured * confidence + 0.5 * (1.0 - confidence)
    };

    let base = coverage * COVERAGE_WEIGHT + retention * RETENTION_WEIGHT + accuracy * ACCURACY_WEIGHT;

    // An imminent exam cannot *raise* readiness, only temper a score built on
    // thin coverage — there is no time left to fix it.
    let urgency_factor = match input.days_until_exam {
        Some(days) if days <= 0 => 1.0,
        Some(days) if days < URGENCY_HORIZON_DAYS => {
            let closeness = 1.0 - (days as f64 / URGENCY_HORIZON_DAYS as f64);
            1.0 - (1.0 - coverage) * closeness * 0.25
        }
        _ => 1.0,
    };

    let score = (base * urgency_factor * 100.0).round().clamp(0.0, 100.0) as i64;
    let has_enough_data = input.total_cards > 0 && input.total_reviews > 0;

    Readiness { score, reason: reason_for(&input, coverage, retention, accuracy, has_enough_data), coverage, retention, accuracy, has_enough_data }
}

fn reason_for(input: &ReadinessInput, coverage: f64, retention: f64, accuracy: f64, has_data: bool) -> String {
    if input.total_cards == 0 {
        return "No cards in this course yet — add some and this starts to mean something.".to_string();
    }
    if !has_data {
        return format!("{} cards waiting, none reviewed yet.", input.total_cards);
    }

    // Saturating: these counts come from the database and a nonsensical
    // pair must not overflow while building a human-readable sentence.
    let unseen = input.total_cards.saturating_sub(input.seen_cards).max(0);
    let exam_note = match input.days_until_exam {
        Some(d) if d < 0 => String::new(),
        Some(0) => " Exam is today.".to_string(),
        Some(1) => " Exam is tomorrow.".to_string(),
        Some(d) if d < URGENCY_HORIZON_DAYS => format!(" Exam in {d} days."),
        _ => String::new(),
    };

    // Name the weakest of the three, since that is the one worth acting on.
    let weakest = if coverage <= retention && coverage <= accuracy {
        if unseen > 0 {
            format!("held back by coverage: {unseen} of {} cards never seen", input.total_cards)
        } else {
            "held back by coverage".to_string()
        }
    } else if retention <= accuracy {
        format!(
            "held back by retention: {} of {} cards are not yet durable",
            input.total_cards.saturating_sub(input.mature_cards).max(0),
            input.total_cards
        )
    } else {
        format!("held back by accuracy: {}% correct so far", (accuracy * 100.0).round() as i64)
    };

    format!("{weakest}.{exam_note}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(total: i64, seen: i64, mature: i64, reviews: i64, correct: i64) -> ReadinessInput {
        ReadinessInput {
            total_cards: total,
            seen_cards: seen,
            mature_cards: mature,
            total_reviews: reviews,
            correct_reviews: correct,
            days_until_exam: None,
        }
    }

    #[test]
    fn an_empty_course_scores_zero_and_says_why() {
        let r = assess(input(0, 0, 0, 0, 0));
        assert_eq!(r.score, 0);
        assert!(!r.has_enough_data);
        assert!(r.reason.contains("No cards"));
    }

    #[test]
    fn cards_with_no_reviews_are_distinguished_from_no_cards() {
        let r = assess(input(50, 0, 0, 0, 0));
        assert_eq!(r.score, 0);
        assert!(!r.has_enough_data);
        assert!(r.reason.contains("50 cards waiting"));
    }

    #[test]
    fn full_coverage_retention_and_accuracy_is_a_hundred() {
        let r = assess(input(100, 100, 100, 200, 200));
        assert_eq!(r.score, 100);
    }

    /// The headline property: knowing everything you have seen is not being
    /// ready when you have seen a fifth of it.
    #[test]
    fn perfect_accuracy_on_thin_coverage_does_not_look_ready() {
        let r = assess(input(100, 20, 20, 40, 40));
        assert!(r.score < 45, "scored {} — thin coverage must dominate", r.score);
        assert!(r.reason.contains("coverage"));
        assert!(r.reason.contains("80 of 100 cards never seen"));
    }

    #[test]
    fn the_reason_names_the_weakest_component() {
        // Coverage complete and durable, accuracy poor.
        let poor_accuracy = assess(input(100, 100, 100, 100, 40));
        assert!(poor_accuracy.reason.contains("accuracy"), "{}", poor_accuracy.reason);

        // Seen everything, but nothing has matured.
        let poor_retention = assess(input(100, 100, 0, 100, 95));
        assert!(poor_retention.reason.contains("retention"), "{}", poor_retention.reason);
        assert!(poor_retention.reason.contains("100 of 100 cards are not yet durable"));
    }

    /// One lucky answer must not read as 100% accurate, and two answers must
    /// not read as 0%.
    #[test]
    fn accuracy_is_damped_until_there_is_enough_evidence() {
        let one_right = assess(input(10, 1, 0, 1, 1));
        let one_wrong = assess(input(10, 1, 0, 1, 0));
        assert!(one_right.accuracy < 1.0, "a single correct answer is not 100%");
        assert!(one_wrong.accuracy > 0.0, "a single wrong answer is not 0%");
        // With enough evidence it is taken at face value.
        let settled = assess(input(10, 10, 0, MIN_REVIEWS_FOR_ACCURACY, MIN_REVIEWS_FOR_ACCURACY));
        assert_eq!(settled.accuracy, 1.0);
    }

    #[test]
    fn an_imminent_exam_tempers_a_thin_score_but_never_raises_one() {
        let base = assess(input(100, 30, 10, 50, 40));
        let soon = assess(ReadinessInput { days_until_exam: Some(1), ..input(100, 30, 10, 50, 40) });
        let far = assess(ReadinessInput { days_until_exam: Some(90), ..input(100, 30, 10, 50, 40) });
        assert!(soon.score < base.score, "an exam tomorrow should not flatter thin coverage");
        assert_eq!(far.score, base.score, "a distant exam changes nothing");

        // Full coverage is not penalised by proximity at all.
        let ready = assess(input(100, 100, 100, 200, 200));
        let ready_soon = assess(ReadinessInput { days_until_exam: Some(1), ..input(100, 100, 100, 200, 200) });
        assert_eq!(ready.score, ready_soon.score);
    }

    #[test]
    fn the_exam_is_mentioned_in_the_reason_when_it_is_close() {
        assert!(assess(ReadinessInput { days_until_exam: Some(0), ..input(10, 5, 1, 20, 15) }).reason.contains("today"));
        assert!(assess(ReadinessInput { days_until_exam: Some(1), ..input(10, 5, 1, 20, 15) }).reason.contains("tomorrow"));
        assert!(assess(ReadinessInput { days_until_exam: Some(5), ..input(10, 5, 1, 20, 15) }).reason.contains("in 5 days"));
        // A past exam is not worth mentioning.
        assert!(!assess(ReadinessInput { days_until_exam: Some(-3), ..input(10, 5, 1, 20, 15) }).reason.contains("Exam"));
    }

    /// This runs on whatever is in the database, including nonsense.
    #[test]
    fn nothing_panics_and_the_score_stays_in_range() {
        let values = [i64::MIN, -5, 0, 1, 10, 1000, i64::MAX];
        for &total in &values {
            for &seen in &values {
                for &mature in &values {
                    for &reviews in &[0i64, 1, 50, i64::MAX] {
                        for &days in &[None, Some(i64::MIN), Some(-1), Some(0), Some(7), Some(i64::MAX)] {
                            let r = assess(ReadinessInput {
                                total_cards: total,
                                seen_cards: seen,
                                mature_cards: mature,
                                total_reviews: reviews,
                                correct_reviews: reviews,
                                days_until_exam: days,
                            });
                            assert!((0..=100).contains(&r.score), "score {} out of range", r.score);
                            assert!(r.coverage.is_finite() && (0.0..=1.0).contains(&r.coverage));
                            assert!(r.retention.is_finite() && (0.0..=1.0).contains(&r.retention));
                            assert!(r.accuracy.is_finite() && (0.0..=1.0).contains(&r.accuracy));
                            assert!(!r.reason.is_empty());
                        }
                    }
                }
            }
        }
    }

    /// More seen cards than exist, or more correct than reviewed, must clamp
    /// rather than push the score past 100.
    #[test]
    fn inconsistent_counts_clamp_instead_of_overflowing() {
        let r = assess(input(10, 999, 999, 10, 999));
        assert!(r.score <= 100);
        assert_eq!(r.coverage, 1.0);
        assert_eq!(r.accuracy, 1.0);
    }
}
