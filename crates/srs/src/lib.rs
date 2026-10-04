//! Spaced-repetition scheduling: when should this card come back?
//!
//! Deliberately a pure, dependency-free crate in the same spirit as
//! `crates/scheduler` — no `academic-core`, no database, no HTTP, no AI. Every
//! function here is a total function of its arguments, which is what lets the
//! whole algorithm be asserted by exact equality rather than by approximate
//! comparison against magic numbers. It is also written *before* the migration
//! that stores its output, so `card_schedule`'s columns are transcribed from
//! this `CardState` rather than guessed (SQLite cannot retype a column later).
//!
//! # Which algorithm, and why
//!
//! SM-2, not FSRS. FSRS's advantage comes from optimising 17–21 weights against
//! your own accumulated review history; on day one there is none, so it would
//! run on default weights where it is not measurably better for a single user,
//! and the optimiser would mean pulling an ML stack into a workspace that
//! recompiles under `lto = true, codegen-units = 1`. SM-2's every transition is
//! an exact integer or a two-decimal float, so its correctness is checkable by
//! reading it — which matters here, because CI never runs `cargo test`.
//!
//! # Where this deviates from the 1987 paper, and why
//!
//! The *structure* is SM-2: a repetition count, an interval in days, an ease
//! factor, first two intervals fixed, thereafter `interval × ease`.
//!
//! Two deliberate departures, both named as constants so they are tunable:
//!
//! 1. **Ease adjustments.** The paper derives them from a 0–5 quality score,
//!    which works out to −0.80 for a lapse. That collapses a card from the
//!    2.5 starting ease to the 1.3 floor after just two slips, and a card
//!    pinned at the floor barely schedules differently from a brand new one, so
//!    the algorithm stops distinguishing "hard for me" from "never seen". The
//!    gentler per-rating deltas below are the ones large-scale SM-2
//!    implementations settled on.
//! 2. **`Hard` gets its own interval multiplier.** In the paper, `Hard` and
//!    `Good` both schedule at `interval × ease` and differ only in their effect
//!    on ease — so pressing Hard shows the card again at exactly the same time
//!    as Good, which is not what a student pressing it means.
//!
//! Reviews are scheduled from the card's *stored* interval regardless of
//! whether it is answered early or late. That is SM-2's own behaviour, and it
//! keeps `review` a function of `(state, rating)` alone.

use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

/// Ease never drops below this. At the floor a mature card still grows its
/// interval by 30% per success, which is slow but not stalled.
pub const MIN_EASE: f64 = 1.3;
/// And never rises above this, so a long streak of `Easy` cannot launch a card
/// years out on its own.
pub const MAX_EASE: f64 = 3.0;
/// Where a new card starts. SM-2's own default.
pub const INITIAL_EASE: f64 = 2.5;

pub const AGAIN_EASE_DELTA: f64 = -0.20;
pub const HARD_EASE_DELTA: f64 = -0.15;
pub const GOOD_EASE_DELTA: f64 = 0.0;
pub const EASY_EASE_DELTA: f64 = 0.15;

/// First successful review: come back tomorrow.
pub const FIRST_INTERVAL_DAYS: u32 = 1;
/// Second successful review. SM-2's fixed second step.
pub const SECOND_INTERVAL_DAYS: u32 = 6;
/// `Hard` on a mature card. Above 1.0 so the interval still creeps forward —
/// a card you can answer, even with effort, is one you partly know.
pub const HARD_INTERVAL_MULTIPLIER: f64 = 1.2;
/// `Easy` on a mature card gets `interval × ease × this`.
pub const EASY_INTERVAL_BONUS: f64 = 1.3;
/// A lapsed card restarts at one day rather than being pushed out.
pub const RELEARN_INTERVAL_DAYS: u32 = 1;
/// Nothing schedules further out than this. A semester is ~120 days, so an
/// interval beyond a year is indistinguishable from "never" in practice, and
/// capping keeps stored dates sane.
pub const MAX_INTERVAL_DAYS: u32 = 365;
/// Lapses before a card is flagged as a leech — something you keep forgetting,
/// which usually means the card itself is badly written rather than that you
/// need to see it more often.
pub const DEFAULT_LEECH_THRESHOLD: u32 = 8;

/// What the student said about how the recall went. Four buttons rather than
/// SM-2's 0–5 scale: six options is more self-assessment granularity than
/// anyone reliably produces, and four is what the UI can label with a
/// projected interval each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rating {
    /// Did not recall it. Counts as a lapse.
    Again,
    /// Recalled it, with difficulty.
    Hard,
    /// Recalled it.
    Good,
    /// Recalled it immediately.
    Easy,
}

impl Rating {
    pub fn as_str(&self) -> &'static str {
        match self {
            Rating::Again => "again",
            Rating::Hard => "hard",
            Rating::Good => "good",
            Rating::Easy => "easy",
        }
    }

    /// Unrecognised input becomes `Again`: the safe direction to fail, since it
    /// shows the card again sooner rather than hiding it for months.
    pub fn parse(s: &str) -> Self {
        match s {
            "hard" => Rating::Hard,
            "good" => Rating::Good,
            "easy" => Rating::Easy,
            _ => Rating::Again,
        }
    }

    fn ease_delta(self) -> f64 {
        match self {
            Rating::Again => AGAIN_EASE_DELTA,
            Rating::Hard => HARD_EASE_DELTA,
            Rating::Good => GOOD_EASE_DELTA,
            Rating::Easy => EASY_EASE_DELTA,
        }
    }

    fn is_lapse(self) -> bool {
        self == Rating::Again
    }
}

/// Everything the scheduler needs to know about one card's history. This is
/// exactly what migration 0007's `card_schedule` row stores.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CardState {
    /// Consecutive successful reviews. Reset to 0 by a lapse.
    pub repetitions: u32,
    /// Days between the last review and the next one.
    pub interval_days: u32,
    pub ease_factor: f64,
    pub due_date: NaiveDate,
    /// Lifetime lapse count — never reset, since it is a property of the card's
    /// whole history and is what leech detection reads.
    pub lapses: u32,
}

impl CardState {
    /// A card that has never been reviewed: due immediately.
    pub fn new(today: NaiveDate) -> Self {
        Self { repetitions: 0, interval_days: 0, ease_factor: INITIAL_EASE, due_date: today, lapses: 0 }
    }

    pub fn is_due(&self, today: NaiveDate) -> bool {
        self.due_date <= today
    }

    /// Never reviewed, as distinct from reviewed and then lapsed — the two want
    /// different treatment in a queue that caps new cards per day.
    pub fn is_new(&self) -> bool {
        self.repetitions == 0 && self.lapses == 0
    }

    pub fn is_leech(&self, threshold: u32) -> bool {
        self.lapses >= threshold
    }
}

fn clamp_ease(ease: f64) -> f64 {
    // `f64::clamp` passes a NaN *value* straight through (it only panics on NaN
    // bounds), so a non-finite ease — a hand-edited row, a bad import — would
    // otherwise be written back to the database and then poison every
    // subsequent review of that card. Reset to the default instead: losing one
    // card's accumulated ease is recoverable, a NaN in the schedule is not.
    if !ease.is_finite() {
        return INITIAL_EASE;
    }
    // Rounded to two decimals so a long chain of reviews cannot accumulate
    // float drift that makes a stored value disagree with a recomputed one.
    let rounded = (ease * 100.0).round() / 100.0;
    rounded.clamp(MIN_EASE, MAX_EASE)
}

fn clamp_interval(days: u32) -> u32 {
    days.clamp(1, MAX_INTERVAL_DAYS)
}

/// How many days out `rating` would push this card, without applying it.
///
/// This exists so the UI can label each grade button with its real outcome,
/// and it is the *same* code path `review` uses — a separate "preview"
/// implementation would be free to drift out of agreement with the scheduler,
/// which is the kind of bug nobody notices for months.
pub fn project_interval_days(state: &CardState, rating: Rating) -> u32 {
    if rating.is_lapse() {
        return RELEARN_INTERVAL_DAYS;
    }
    let ease = clamp_ease(state.ease_factor + rating.ease_delta());
    let next = match state.repetitions {
        0 => FIRST_INTERVAL_DAYS,
        1 => SECOND_INTERVAL_DAYS,
        _ => {
            let base = state.interval_days.max(1) as f64;
            let grown = match rating {
                Rating::Hard => base * HARD_INTERVAL_MULTIPLIER,
                Rating::Good => base * ease,
                Rating::Easy => base * ease * EASY_INTERVAL_BONUS,
                Rating::Again => unreachable!("handled by the is_lapse branch above"),
            };
            grown.round() as u32
        }
    };
    clamp_interval(next)
}

/// Applies one review and returns the card's new scheduling state.
///
/// Total and deterministic: the same `(state, rating, today)` always produces
/// the same output, and no input can panic.
pub fn review(state: &CardState, rating: Rating, today: NaiveDate) -> CardState {
    let interval_days = project_interval_days(state, rating);
    let ease_factor = clamp_ease(state.ease_factor + rating.ease_delta());

    let (repetitions, lapses) = if rating.is_lapse() {
        // Back to the start of the ladder, but the lapse is remembered forever.
        (0, state.lapses.saturating_add(1))
    } else {
        (state.repetitions.saturating_add(1), state.lapses)
    };

    CardState {
        repetitions,
        interval_days,
        ease_factor,
        due_date: today + Duration::days(interval_days as i64),
        lapses,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn today() -> NaiveDate {
        day(2026, 10, 3)
    }

    #[test]
    fn a_new_card_is_due_immediately_and_counts_as_new() {
        let state = CardState::new(today());
        assert_eq!(state.repetitions, 0);
        assert_eq!(state.interval_days, 0);
        assert_eq!(state.ease_factor, INITIAL_EASE);
        assert!(state.is_due(today()));
        assert!(state.is_new());
        assert!(!state.is_leech(DEFAULT_LEECH_THRESHOLD));
    }

    #[test]
    fn the_first_two_successful_intervals_are_the_fixed_sm2_steps() {
        let first = review(&CardState::new(today()), Rating::Good, today());
        assert_eq!(first.interval_days, 1);
        assert_eq!(first.repetitions, 1);
        assert_eq!(first.due_date, day(2026, 10, 4));

        let second = review(&first, Rating::Good, day(2026, 10, 4));
        assert_eq!(second.interval_days, 6);
        assert_eq!(second.repetitions, 2);
        assert_eq!(second.due_date, day(2026, 10, 10));
    }

    #[test]
    fn a_mature_good_review_multiplies_the_interval_by_ease() {
        // Good leaves ease untouched, so 10 days at ease 2.5 -> 25.
        let state = CardState { repetitions: 3, interval_days: 10, ease_factor: 2.5, due_date: today(), lapses: 0 };
        let next = review(&state, Rating::Good, today());
        assert_eq!(next.interval_days, 25);
        assert_eq!(next.ease_factor, 2.5);
        assert_eq!(next.repetitions, 4);
    }

    #[test]
    fn hard_moves_the_card_forward_but_less_than_good_and_lowers_ease() {
        let state = CardState { repetitions: 3, interval_days: 10, ease_factor: 2.5, due_date: today(), lapses: 0 };
        let hard = review(&state, Rating::Hard, today());
        // 10 * 1.2, not 10 * ease.
        assert_eq!(hard.interval_days, 12);
        assert_eq!(hard.ease_factor, 2.35);
        // The deviation from the paper that this constant exists for: Hard and
        // Good must not land on the same day.
        assert!(hard.interval_days < review(&state, Rating::Good, today()).interval_days);
        // Still forward progress, not a repeat.
        assert!(hard.interval_days > state.interval_days);
        assert_eq!(hard.repetitions, 4);
        assert_eq!(hard.lapses, 0);
    }

    #[test]
    fn easy_applies_both_the_ease_increase_and_the_bonus() {
        let state = CardState { repetitions: 3, interval_days: 10, ease_factor: 2.5, due_date: today(), lapses: 0 };
        let easy = review(&state, Rating::Easy, today());
        // ease 2.65, then 10 * 2.65 * 1.3 = 34.45 -> 34.
        assert_eq!(easy.ease_factor, 2.65);
        assert_eq!(easy.interval_days, 34);
    }

    #[test]
    fn again_resets_repetitions_counts_a_lapse_and_relearns_tomorrow() {
        let state = CardState { repetitions: 5, interval_days: 60, ease_factor: 2.5, due_date: today(), lapses: 1 };
        let lapsed = review(&state, Rating::Again, today());
        assert_eq!(lapsed.repetitions, 0);
        assert_eq!(lapsed.interval_days, RELEARN_INTERVAL_DAYS);
        assert_eq!(lapsed.due_date, day(2026, 10, 4));
        assert_eq!(lapsed.lapses, 2);
        assert_eq!(lapsed.ease_factor, 2.30);
        // A lapsed card is not "new" — it has history, which matters to a queue
        // that caps new cards per day.
        assert!(!lapsed.is_new());
    }

    #[test]
    fn a_lapse_then_a_success_climbs_the_ladder_from_the_bottom_again() {
        let mature = CardState { repetitions: 5, interval_days: 60, ease_factor: 2.5, due_date: today(), lapses: 0 };
        let lapsed = review(&mature, Rating::Again, today());
        let recovered = review(&lapsed, Rating::Good, day(2026, 10, 4));
        assert_eq!(recovered.interval_days, FIRST_INTERVAL_DAYS);
        assert_eq!(recovered.repetitions, 1);
        // Ease is not restored by recovering — the lapse is remembered.
        assert_eq!(recovered.ease_factor, 2.30);
    }

    #[test]
    fn ease_is_floored_and_never_stalls_a_card_completely() {
        let mut state = CardState { repetitions: 3, interval_days: 10, ease_factor: MIN_EASE, due_date: today(), lapses: 0 };
        // Repeated Hard cannot push ease below the floor.
        for _ in 0..20 {
            state = review(&state, Rating::Hard, today());
            assert!(state.ease_factor >= MIN_EASE, "ease fell below the floor: {}", state.ease_factor);
        }
        // And a card at the floor still schedules forward.
        let before = CardState { repetitions: 3, interval_days: 10, ease_factor: MIN_EASE, due_date: today(), lapses: 0 };
        assert!(review(&before, Rating::Good, today()).interval_days > before.interval_days);
    }

    #[test]
    fn ease_is_capped_so_an_easy_streak_cannot_run_away() {
        let mut state = CardState::new(today());
        for _ in 0..40 {
            state = review(&state, Rating::Easy, today());
            assert!(state.ease_factor <= MAX_EASE, "ease rose above the cap: {}", state.ease_factor);
        }
    }

    #[test]
    fn intervals_are_capped_at_a_year() {
        let state = CardState { repetitions: 9, interval_days: 300, ease_factor: 3.0, due_date: today(), lapses: 0 };
        let next = review(&state, Rating::Easy, today());
        assert_eq!(next.interval_days, MAX_INTERVAL_DAYS);
        assert_eq!(next.due_date, today() + Duration::days(365));
    }

    /// The preview shown on a grade button must be what pressing it actually
    /// does, for every rating and a wide spread of states.
    #[test]
    fn the_projected_interval_always_matches_what_review_applies() {
        for repetitions in [0u32, 1, 2, 5, 40] {
            for interval_days in [0u32, 1, 6, 10, 99, 364, 365, 400] {
                for ease in [MIN_EASE, 1.8, 2.5, 2.9, MAX_EASE] {
                    let state = CardState { repetitions, interval_days, ease_factor: ease, due_date: today(), lapses: 0 };
                    for rating in [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy] {
                        assert_eq!(
                            project_interval_days(&state, rating),
                            review(&state, rating, today()).interval_days,
                            "preview disagreed with review for {state:?} {rating:?}"
                        );
                    }
                }
            }
        }
    }

    /// No input may panic, produce a zero-day interval, or schedule into the
    /// past — a card due yesterday would never leave the queue.
    #[test]
    fn no_state_and_rating_combination_produces_an_invalid_schedule() {
        for repetitions in [0u32, 1, 3, u32::MAX] {
            for interval_days in [0u32, 1, 7, u32::MAX] {
                for ease in [0.0, -5.0, MIN_EASE, 2.5, 1000.0, f64::MAX, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                    for lapses in [0u32, 7, u32::MAX] {
                        let state = CardState { repetitions, interval_days, ease_factor: ease, due_date: today(), lapses };
                        for rating in [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy] {
                            let next = review(&state, rating, today());
                            assert!(next.interval_days >= 1, "interval must move the card forward");
                            assert!(next.interval_days <= MAX_INTERVAL_DAYS);
                            assert!(next.ease_factor >= MIN_EASE && next.ease_factor <= MAX_EASE);
                            assert!(next.due_date > today(), "a reviewed card must not still be due today");
                            // u32::MAX lapses must not wrap back to zero.
                            assert!(next.lapses >= state.lapses);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_nan_ease_cannot_poison_the_schedule() {
        // `clamp` panics on a NaN bound, and `f64::NAN as u32` is 0 — a stored
        // NaN (from a hand-edited row, say) must not produce a 0-day interval.
        let state = CardState { repetitions: 3, interval_days: 10, ease_factor: f64::NAN, due_date: today(), lapses: 0 };
        let next = review(&state, Rating::Good, today());
        assert!(next.ease_factor.is_finite());
        assert_eq!(next.ease_factor, INITIAL_EASE, "a corrupt ease resets rather than propagating");
        assert!(next.interval_days >= 1);
        // The subtler half: `f64::NAN as u32` saturates to 0, so without the
        // guard the interval would have been clamped up from zero and the card
        // would silently have been rescheduled as if brand new.
        assert_eq!(project_interval_days(&state, Rating::Good), next.interval_days);
    }

    #[test]
    fn leeches_are_flagged_at_the_threshold_not_past_it() {
        let at = CardState { repetitions: 0, interval_days: 1, ease_factor: MIN_EASE, due_date: today(), lapses: 8 };
        assert!(at.is_leech(DEFAULT_LEECH_THRESHOLD));
        let below = CardState { lapses: 7, ..at };
        assert!(!below.is_leech(DEFAULT_LEECH_THRESHOLD));
    }

    #[test]
    fn due_dates_compare_inclusively() {
        let state = CardState { repetitions: 1, interval_days: 1, ease_factor: 2.5, due_date: day(2026, 10, 3), lapses: 0 };
        assert!(state.is_due(day(2026, 10, 3)), "a card due today is due");
        assert!(state.is_due(day(2026, 10, 4)), "an overdue card is due");
        assert!(!state.is_due(day(2026, 10, 2)));
    }

    #[test]
    fn ratings_round_trip_through_their_wire_strings() {
        for rating in [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy] {
            assert_eq!(Rating::parse(rating.as_str()), rating);
        }
        // Unknown input fails toward showing the card again, not hiding it.
        assert_eq!(Rating::parse("wat"), Rating::Again);
        assert_eq!(Rating::parse(""), Rating::Again);
    }
}
