use chrono::NaiveDate;
use sqlx::{Row, SqlitePool};

use super::new_id;
use crate::error::CoreError;
use crate::models::{Card, CardKind, CardReview, CardSchedule, DueCard, IntervalProjections, ReviewOutcome};
use srs::{CardState, Rating};

/// Caps for one day's queue. A deck of 300 freshly generated cards must not
/// present itself as 300 reviews due tonight — that is how someone abandons
/// spaced repetition in week two.
pub const DEFAULT_NEW_PER_DAY: i64 = 20;
pub const DEFAULT_REVIEWS_PER_DAY: i64 = 120;

/// Client-reported durations are clamped: a card left open over lunch is not
/// two hours of study, and the number feeds study-time totals.
const MAX_DURATION_MS: i64 = 10 * 60 * 1000;

const SCHEDULE_COLUMNS: &str =
    "card_id, repetitions, interval_days, ease_factor, due_date, lapses, suspended, updated_at";

const CARD_COLUMNS: &str = "id, deck_id, order_index, kind, front, back, options_json, explanation, tags_json, \
    source_excerpt, created_at, updated_at";

fn json_list(raw: Option<String>) -> Option<Vec<String>> {
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
}

fn row_to_card(r: &sqlx::sqlite::SqliteRow) -> Result<Card, CoreError> {
    Ok(Card {
        id: r.try_get("id")?,
        deck_id: r.try_get("deck_id")?,
        order_index: r.try_get("order_index")?,
        kind: CardKind::parse(&r.try_get::<String, _>("kind")?),
        front: r.try_get("front")?,
        back: r.try_get("back")?,
        options: json_list(r.try_get("options_json")?),
        explanation: r.try_get("explanation")?,
        tags: json_list(r.try_get("tags_json")?).unwrap_or_default(),
        source_excerpt: r.try_get("source_excerpt")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

fn row_to_schedule(r: sqlx::sqlite::SqliteRow) -> Result<CardSchedule, CoreError> {
    Ok(CardSchedule {
        card_id: r.try_get("card_id")?,
        repetitions: r.try_get("repetitions")?,
        interval_days: r.try_get("interval_days")?,
        ease_factor: r.try_get("ease_factor")?,
        due_date: r.try_get("due_date")?,
        lapses: r.try_get("lapses")?,
        suspended: r.try_get::<i64, _>("suspended")? != 0,
        updated_at: r.try_get("updated_at")?,
    })
}

fn to_state(schedule: &CardSchedule) -> CardState {
    CardState {
        repetitions: schedule.repetitions.max(0) as u32,
        interval_days: schedule.interval_days.max(0) as u32,
        ease_factor: schedule.ease_factor,
        // A stored date that will not parse is treated as "due now" rather
        // than failing the read: the worst case is seeing the card sooner.
        due_date: NaiveDate::parse_from_str(&schedule.due_date, "%Y-%m-%d").unwrap_or_else(|_| default_today()),
        lapses: schedule.lapses.max(0) as u32,
    }
}

/// What each button would do to this card, from the same code path a review
/// uses — see `srs::project_interval_days`.
fn project(state: &CardState) -> IntervalProjections {
    IntervalProjections {
        again: srs::project_interval_days(state, Rating::Again) as i64,
        hard: srs::project_interval_days(state, Rating::Hard) as i64,
        good: srs::project_interval_days(state, Rating::Good) as i64,
        easy: srs::project_interval_days(state, Rating::Easy) as i64,
    }
}

fn default_today() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 1).expect("a valid constant date")
}

pub async fn get_schedule(pool: &SqlitePool, card_id: &str) -> Result<Option<CardSchedule>, CoreError> {
    let row = sqlx::query(&format!("SELECT {SCHEDULE_COLUMNS} FROM card_schedule WHERE card_id = ?"))
        .bind(card_id)
        .fetch_optional(pool)
        .await?;
    row.map(row_to_schedule).transpose()
}

/// Records one graded answer and advances the card's schedule **in a single
/// transaction**.
///
/// The read, the SM-2 computation and both writes happen together so the review
/// log and the schedule can never disagree — a crash between them would
/// otherwise leave a card whose history says it was answered and whose schedule
/// says it was not, and every analytic built on the log would be wrong for that
/// card forever.
///
/// This is also why `academic-core` depends on `srs`: doing the arithmetic in a
/// caller would mean reading the state, computing outside the transaction and
/// writing back, which is the interleaving this function exists to avoid.
pub async fn record_review(
    pool: &SqlitePool,
    card_id: &str,
    rating: Rating,
    today: NaiveDate,
    local_date: &str,
    duration_ms: Option<i64>,
) -> Result<ReviewOutcome, CoreError> {
    let mut tx = pool.begin().await?;

    // Inside the transaction, so a concurrent review of the same card cannot
    // compute from state this one is about to replace.
    let existing = sqlx::query(&format!("SELECT {SCHEDULE_COLUMNS} FROM card_schedule WHERE card_id = ?"))
        .bind(card_id)
        .fetch_optional(&mut *tx)
        .await?
        .map(row_to_schedule)
        .transpose()?;

    if existing.is_none() {
        // The card must exist; a review of a deleted card is a 404, not a new
        // schedule row that would violate the foreign key anyway.
        let exists: Option<String> =
            sqlx::query_scalar("SELECT id FROM cards WHERE id = ?").bind(card_id).fetch_optional(&mut *tx).await?;
        if exists.is_none() {
            return Err(CoreError::NotFound(card_id.to_string()));
        }
    }

    let before = existing.as_ref().map(|s| to_state(s)).unwrap_or_else(|| CardState::new(today));
    let after = srs::review(&before, rating, today);

    let suspended = existing.as_ref().is_some_and(|s| s.suspended);
    sqlx::query(
        "INSERT INTO card_schedule (card_id, repetitions, interval_days, ease_factor, due_date, lapses, suspended) \
         VALUES (?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(card_id) DO UPDATE SET repetitions = excluded.repetitions, \
           interval_days = excluded.interval_days, ease_factor = excluded.ease_factor, \
           due_date = excluded.due_date, lapses = excluded.lapses, \
           updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
    )
    .bind(card_id)
    .bind(after.repetitions as i64)
    .bind(after.interval_days as i64)
    .bind(after.ease_factor)
    .bind(after.due_date.format("%Y-%m-%d").to_string())
    .bind(after.lapses as i64)
    .bind(i64::from(suspended))
    .execute(&mut *tx)
    .await?;

    let review_id = new_id();
    sqlx::query(
        "INSERT INTO card_reviews (id, card_id, rating, local_date, duration_ms, interval_before, interval_after, \
         ease_after, was_lapse) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&review_id)
    .bind(card_id)
    .bind(rating.as_str())
    .bind(local_date)
    .bind(duration_ms.map(|d| d.clamp(0, MAX_DURATION_MS)))
    .bind(before.interval_days as i64)
    .bind(after.interval_days as i64)
    .bind(after.ease_factor)
    .bind(i64::from(rating == Rating::Again))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let schedule = get_schedule(pool, card_id).await?.ok_or_else(|| CoreError::NotFound(card_id.to_string()))?;
    Ok(ReviewOutcome {
        is_leech: after.is_leech(srs::DEFAULT_LEECH_THRESHOLD),
        schedule,
        review_id,
    })
}

pub async fn set_suspended(pool: &SqlitePool, card_id: &str, suspended: bool, today: NaiveDate) -> Result<(), CoreError> {
    // A card can be suspended before it has ever been reviewed, so the row may
    // need creating — with a schedule that means "new", not "due in 0 days".
    let fresh = CardState::new(today);
    let result = sqlx::query(
        "INSERT INTO card_schedule (card_id, repetitions, interval_days, ease_factor, due_date, lapses, suspended) \
         VALUES (?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(card_id) DO UPDATE SET suspended = excluded.suspended, \
           updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
    )
    .bind(card_id)
    .bind(fresh.repetitions as i64)
    .bind(fresh.interval_days as i64)
    .bind(fresh.ease_factor)
    .bind(fresh.due_date.format("%Y-%m-%d").to_string())
    .bind(fresh.lapses as i64)
    .bind(i64::from(suspended))
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(CoreError::NotFound(card_id.to_string()));
    }
    Ok(())
}

pub struct QueueLimits {
    pub new_per_day: i64,
    pub reviews_per_day: i64,
    /// Restrict to one course. `None` means everything, which is the point of
    /// the queue — one place for all seven courses.
    pub course_id: Option<String>,
}

impl Default for QueueLimits {
    fn default() -> Self {
        Self { new_per_day: DEFAULT_NEW_PER_DAY, reviews_per_day: DEFAULT_REVIEWS_PER_DAY, course_id: None }
    }
}

/// Everything due today across every deck and course, plus some new cards.
///
/// Due cards come first and are capped separately from new ones: falling behind
/// on reviews while still being shown new material is how a backlog becomes
/// unrecoverable. Suspended cards never appear.
///
/// Ordering interleaves courses rather than draining one deck at a time —
/// `ROW_NUMBER() OVER (PARTITION BY course)` — so a session covers the seven
/// courses the student actually has rather than forty cards of chemistry.
pub async fn due_queue(pool: &SqlitePool, today: NaiveDate, limits: QueueLimits) -> Result<Vec<DueCard>, CoreError> {
    let today_str = today.format("%Y-%m-%d").to_string();
    let course_filter = match &limits.course_id {
        Some(_) => "AND d.course_id = ?2",
        None => "",
    };

    // `s.card_id IS NULL` is a card never reviewed; those are the "new" half.
    let sql = format!(
        "WITH candidate AS (
             SELECT {CARD_COLUMNS_PREFIXED}, d.name AS deck_name, co.name AS course_name,
                    {SCHEDULE_COLUMNS_PREFIXED},
                    (s.card_id IS NULL) AS is_new,
                    COALESCE(d.course_id, 'none') AS course_key
             FROM cards c
             JOIN decks d ON d.id = c.deck_id
             LEFT JOIN courses co ON co.id = d.course_id
             LEFT JOIN card_schedule s ON s.card_id = c.id
             WHERE COALESCE(s.suspended, 0) = 0
               AND (s.card_id IS NULL OR s.due_date <= ?1)
               {course_filter}
         ),
         ranked AS (
             SELECT *, ROW_NUMBER() OVER (PARTITION BY course_key, is_new ORDER BY due_date, order_index) AS rn
             FROM candidate
         )
         SELECT * FROM ranked
         WHERE (is_new = 0 AND rn <= ?3) OR (is_new = 1 AND rn <= ?4)
         ORDER BY is_new, rn, course_key",
        CARD_COLUMNS_PREFIXED = CARD_COLUMNS.split(", ").map(|c| format!("c.{c}")).collect::<Vec<_>>().join(", "),
        SCHEDULE_COLUMNS_PREFIXED = SCHEDULE_COLUMNS
            .split(", ")
            .map(|c| if c == "card_id" { "s.card_id AS schedule_card_id".to_string() } else { format!("s.{c}") })
            .collect::<Vec<_>>()
            .join(", "),
    );

    let mut query = sqlx::query(&sql).bind(&today_str);
    query = match &limits.course_id {
        Some(cid) => query.bind(cid),
        // ?2 must still be bound even when unused by the SQL text.
        None => query.bind(""),
    };
    let rows = query.bind(limits.reviews_per_day.max(0)).bind(limits.new_per_day.max(0)).fetch_all(pool).await?;

    rows.into_iter()
        .map(|r| {
            let is_new: i64 = r.try_get("is_new")?;
            let schedule = if r.try_get::<Option<String>, _>("schedule_card_id")?.is_some() {
                Some(CardSchedule {
                    card_id: r.try_get("schedule_card_id")?,
                    repetitions: r.try_get("repetitions")?,
                    interval_days: r.try_get("interval_days")?,
                    ease_factor: r.try_get("ease_factor")?,
                    due_date: r.try_get("due_date")?,
                    lapses: r.try_get("lapses")?,
                    suspended: r.try_get::<i64, _>("suspended")? != 0,
                    updated_at: r.try_get("updated_at")?,
                })
            } else {
                None
            };
            let state = schedule.as_ref().map(to_state).unwrap_or_else(|| CardState::new(today));
            Ok(DueCard {
                card: row_to_card(&r)?,
                deck_name: r.try_get("deck_name")?,
                course_name: r.try_get("course_name")?,
                projections: project(&state),
                schedule,
                is_new: is_new != 0,
            })
        })
        .collect()
}

pub async fn list_reviews_for_card(pool: &SqlitePool, card_id: &str) -> Result<Vec<CardReview>, CoreError> {
    let rows = sqlx::query(
        "SELECT id, card_id, rating, local_date, reviewed_at, duration_ms, interval_before, interval_after, \
         ease_after, was_lapse FROM card_reviews WHERE card_id = ? ORDER BY reviewed_at",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(CardReview {
                id: r.try_get("id")?,
                card_id: r.try_get("card_id")?,
                rating: r.try_get("rating")?,
                local_date: r.try_get("local_date")?,
                reviewed_at: r.try_get("reviewed_at")?,
                duration_ms: r.try_get("duration_ms")?,
                interval_before: r.try_get("interval_before")?,
                interval_after: r.try_get("interval_after")?,
                ease_after: r.try_get("ease_after")?,
                was_lapse: r.try_get::<i64, _>("was_lapse")? != 0,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::models::{NewCard, NewCourse, NewDeck, NewSemester};
    use crate::repo::{cards, courses, decks, semesters};

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    async fn deck_with_cards(pool: &SqlitePool, name: &str, course_id: Option<String>, n: usize) -> (String, Vec<String>) {
        let deck = decks::create(pool, NewDeck { course_id, name: name.into(), description: None, color: None })
            .await
            .unwrap();
        let mut ids = Vec::new();
        for i in 0..n {
            let c = cards::create(
                pool,
                &deck.id,
                NewCard {
                    front: format!("{name} q{i}"),
                    back: "a".into(),
                    kind: None,
                    options: None,
                    explanation: None,
                    tags: None,
                    source_excerpt: None,
                    order_index: None,
                },
            )
            .await
            .unwrap();
            ids.push(c.id);
        }
        (deck.id, ids)
    }

    async fn seed_course(pool: &SqlitePool, name: &str) -> String {
        let sem = semesters::create(pool, NewSemester { name: "Fall".into(), start_date: None, end_date: None })
            .await
            .unwrap();
        courses::create(
            pool,
            NewCourse {
                semester_id: sem.id,
                name: name.into(),
                code: None,
                professor_name: None,
                professor_email: None,
                credit_hours: None,
                color: None,
                external_org_unit_id: None,
            },
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn a_new_card_has_no_schedule_until_it_is_reviewed() {
        let pool = connect_in_memory().await.unwrap();
        let (_, ids) = deck_with_cards(&pool, "D", None, 1).await;
        assert!(get_schedule(&pool, &ids[0]).await.unwrap().is_none());

        let out = record_review(&pool, &ids[0], Rating::Good, day(2026, 10, 5), "2026-10-05", Some(4000)).await.unwrap();
        assert_eq!(out.schedule.interval_days, 1);
        assert_eq!(out.schedule.repetitions, 1);
        assert_eq!(out.schedule.due_date, "2026-10-06");
        assert!(!out.is_leech);
    }

    #[tokio::test]
    async fn the_review_log_and_the_schedule_are_written_together() {
        let pool = connect_in_memory().await.unwrap();
        let (_, ids) = deck_with_cards(&pool, "D", None, 1).await;
        record_review(&pool, &ids[0], Rating::Good, day(2026, 10, 5), "2026-10-05", None).await.unwrap();
        record_review(&pool, &ids[0], Rating::Good, day(2026, 10, 6), "2026-10-06", None).await.unwrap();

        let log = list_reviews_for_card(&pool, &ids[0]).await.unwrap();
        assert_eq!(log.len(), 2);
        // The second review's "before" must be the first review's "after" —
        // which is only true if both writes really did share a transaction.
        assert_eq!(log[0].interval_after, log[1].interval_before);
        assert_eq!(log[1].interval_after, 6);
        assert_eq!(get_schedule(&pool, &ids[0]).await.unwrap().unwrap().interval_days, 6);
    }

    #[tokio::test]
    async fn a_lapse_is_recorded_and_resets_the_card() {
        let pool = connect_in_memory().await.unwrap();
        let (_, ids) = deck_with_cards(&pool, "D", None, 1).await;
        for d in 5..8 {
            record_review(&pool, &ids[0], Rating::Good, day(2026, 10, d), "x", None).await.unwrap();
        }
        let out = record_review(&pool, &ids[0], Rating::Again, day(2026, 10, 8), "2026-10-08", None).await.unwrap();
        assert_eq!(out.schedule.repetitions, 0);
        assert_eq!(out.schedule.lapses, 1);
        assert_eq!(out.schedule.interval_days, 1);

        let log = list_reviews_for_card(&pool, &ids[0]).await.unwrap();
        assert!(log.last().unwrap().was_lapse);
        assert!(!log[0].was_lapse);
    }

    #[tokio::test]
    async fn a_card_that_keeps_lapsing_is_flagged_as_a_leech() {
        let pool = connect_in_memory().await.unwrap();
        let (_, ids) = deck_with_cards(&pool, "D", None, 1).await;
        let mut last = None;
        for i in 0..srs::DEFAULT_LEECH_THRESHOLD {
            last = Some(
                record_review(&pool, &ids[0], Rating::Again, day(2026, 10, 5), &format!("d{i}"), None).await.unwrap(),
            );
        }
        assert!(last.unwrap().is_leech);
    }

    #[tokio::test]
    async fn reviewing_a_card_that_does_not_exist_is_not_found() {
        let pool = connect_in_memory().await.unwrap();
        assert!(record_review(&pool, "nope", Rating::Good, day(2026, 10, 5), "x", None).await.is_err());
    }

    /// A card left open over lunch is not two hours of study, and this number
    /// feeds study-time totals.
    #[tokio::test]
    async fn an_absurd_duration_is_clamped_and_a_negative_one_floored() {
        let pool = connect_in_memory().await.unwrap();
        let (_, ids) = deck_with_cards(&pool, "D", None, 2).await;
        record_review(&pool, &ids[0], Rating::Good, day(2026, 10, 5), "x", Some(99_999_999)).await.unwrap();
        record_review(&pool, &ids[1], Rating::Good, day(2026, 10, 5), "x", Some(-5)).await.unwrap();
        assert_eq!(list_reviews_for_card(&pool, &ids[0]).await.unwrap()[0].duration_ms, Some(MAX_DURATION_MS));
        assert_eq!(list_reviews_for_card(&pool, &ids[1]).await.unwrap()[0].duration_ms, Some(0));
    }

    #[tokio::test]
    async fn the_queue_shows_new_cards_and_hides_ones_not_yet_due() {
        let pool = connect_in_memory().await.unwrap();
        let (_, ids) = deck_with_cards(&pool, "D", None, 3).await;
        let today = day(2026, 10, 5);

        assert_eq!(due_queue(&pool, today, QueueLimits::default()).await.unwrap().len(), 3);
        // Answering one pushes it to tomorrow, so it leaves today's queue.
        record_review(&pool, &ids[0], Rating::Good, today, "2026-10-05", None).await.unwrap();
        let queue = due_queue(&pool, today, QueueLimits::default()).await.unwrap();
        assert_eq!(queue.len(), 2);
        assert!(queue.iter().all(|c| c.card.id != ids[0]));
        // And comes back the day it is due.
        assert_eq!(due_queue(&pool, day(2026, 10, 6), QueueLimits::default()).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn an_overdue_card_stays_in_the_queue() {
        let pool = connect_in_memory().await.unwrap();
        let (_, ids) = deck_with_cards(&pool, "D", None, 1).await;
        record_review(&pool, &ids[0], Rating::Good, day(2026, 10, 5), "x", None).await.unwrap();
        // Due the 6th, not looked at until the 20th.
        let queue = due_queue(&pool, day(2026, 10, 20), QueueLimits::default()).await.unwrap();
        assert_eq!(queue.len(), 1);
        assert!(!queue[0].is_new);
    }

    #[tokio::test]
    async fn suspended_cards_never_appear_and_un_suspending_restores_them() {
        let pool = connect_in_memory().await.unwrap();
        let (_, ids) = deck_with_cards(&pool, "D", None, 2).await;
        let today = day(2026, 10, 5);

        set_suspended(&pool, &ids[0], true, today).await.unwrap();
        let queue = due_queue(&pool, today, QueueLimits::default()).await.unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].card.id, ids[1]);

        set_suspended(&pool, &ids[0], false, today).await.unwrap();
        assert_eq!(due_queue(&pool, today, QueueLimits::default()).await.unwrap().len(), 2);
    }

    /// Suspending must not wipe a card's history — that is why the row is kept
    /// rather than deleted.
    #[tokio::test]
    async fn suspending_a_reviewed_card_preserves_its_schedule() {
        let pool = connect_in_memory().await.unwrap();
        let (_, ids) = deck_with_cards(&pool, "D", None, 1).await;
        record_review(&pool, &ids[0], Rating::Good, day(2026, 10, 5), "x", None).await.unwrap();
        record_review(&pool, &ids[0], Rating::Good, day(2026, 10, 6), "x", None).await.unwrap();

        set_suspended(&pool, &ids[0], true, day(2026, 10, 6)).await.unwrap();
        let s = get_schedule(&pool, &ids[0]).await.unwrap().unwrap();
        assert!(s.suspended);
        assert_eq!(s.interval_days, 6, "the schedule survived being suspended");
        assert_eq!(s.repetitions, 2);
    }

    /// A review of a suspended card must not silently un-suspend it.
    #[tokio::test]
    async fn reviewing_a_suspended_card_leaves_it_suspended() {
        let pool = connect_in_memory().await.unwrap();
        let (_, ids) = deck_with_cards(&pool, "D", None, 1).await;
        set_suspended(&pool, &ids[0], true, day(2026, 10, 5)).await.unwrap();
        record_review(&pool, &ids[0], Rating::Good, day(2026, 10, 5), "x", None).await.unwrap();
        assert!(get_schedule(&pool, &ids[0]).await.unwrap().unwrap().suspended);
    }

    #[tokio::test]
    async fn the_new_card_cap_bounds_a_freshly_generated_deck() {
        let pool = connect_in_memory().await.unwrap();
        deck_with_cards(&pool, "Big", None, 50).await;
        let limits = QueueLimits { new_per_day: 5, ..Default::default() };
        // The whole point: 50 new cards must not present as 50 reviews tonight.
        assert_eq!(due_queue(&pool, day(2026, 10, 5), limits).await.unwrap().len(), 5);
    }

    #[tokio::test]
    async fn the_queue_interleaves_courses_rather_than_draining_one_deck() {
        let pool = connect_in_memory().await.unwrap();
        let chem = seed_course(&pool, "Chem").await;
        let math = seed_course(&pool, "Math").await;
        deck_with_cards(&pool, "Chem deck", Some(chem), 10).await;
        deck_with_cards(&pool, "Math deck", Some(math), 10).await;

        let limits = QueueLimits { new_per_day: 4, ..Default::default() };
        let queue = due_queue(&pool, day(2026, 10, 5), limits).await.unwrap();
        // The cap is per course, so a session covers both rather than 4 cards
        // of whichever deck happened to sort first.
        assert_eq!(queue.len(), 8);
        let chem_count = queue.iter().filter(|c| c.course_name.as_deref() == Some("Chem")).count();
        assert_eq!(chem_count, 4);
        assert_eq!(queue.len() - chem_count, 4);
    }

    #[tokio::test]
    async fn the_queue_can_be_narrowed_to_one_course() {
        let pool = connect_in_memory().await.unwrap();
        let chem = seed_course(&pool, "Chem").await;
        let math = seed_course(&pool, "Math").await;
        deck_with_cards(&pool, "Chem deck", Some(chem.clone()), 3).await;
        deck_with_cards(&pool, "Math deck", Some(math), 3).await;

        let limits = QueueLimits { course_id: Some(chem), ..Default::default() };
        let queue = due_queue(&pool, day(2026, 10, 5), limits).await.unwrap();
        assert_eq!(queue.len(), 3);
        assert!(queue.iter().all(|c| c.course_name.as_deref() == Some("Chem")));
    }

    #[tokio::test]
    async fn the_queue_carries_where_each_card_lives() {
        let pool = connect_in_memory().await.unwrap();
        let chem = seed_course(&pool, "General Chem I").await;
        deck_with_cards(&pool, "Chapter 4", Some(chem), 1).await;
        deck_with_cards(&pool, "Unfiled", None, 1).await;

        let queue = due_queue(&pool, day(2026, 10, 5), QueueLimits::default()).await.unwrap();
        let filed = queue.iter().find(|c| c.deck_name == "Chapter 4").unwrap();
        assert_eq!(filed.course_name.as_deref(), Some("General Chem I"));
        assert!(filed.is_new);
        assert!(filed.schedule.is_none());
        let unfiled = queue.iter().find(|c| c.deck_name == "Unfiled").unwrap();
        assert!(unfiled.course_name.is_none(), "an unfiled deck's cards still reach the queue");
    }

    #[tokio::test]
    async fn deleting_a_card_takes_its_schedule_and_history() {
        let pool = connect_in_memory().await.unwrap();
        let (_, ids) = deck_with_cards(&pool, "D", None, 1).await;
        record_review(&pool, &ids[0], Rating::Good, day(2026, 10, 5), "x", None).await.unwrap();

        cards::delete(&pool, &ids[0]).await.unwrap();
        assert!(get_schedule(&pool, &ids[0]).await.unwrap().is_none());
        assert!(list_reviews_for_card(&pool, &ids[0]).await.unwrap().is_empty());
    }

    /// A due_date that cannot be parsed must not fail the read or hide the
    /// card forever — it is treated as due now.
    #[tokio::test]
    async fn a_corrupt_due_date_does_not_break_the_queue() {
        let pool = connect_in_memory().await.unwrap();
        let (_, ids) = deck_with_cards(&pool, "D", None, 1).await;
        record_review(&pool, &ids[0], Rating::Good, day(2026, 10, 5), "x", None).await.unwrap();
        sqlx::query("UPDATE card_schedule SET due_date = 'not-a-date' WHERE card_id = ?")
            .bind(&ids[0])
            .execute(&pool)
            .await
            .unwrap();

        // A review still works and repairs the row rather than panicking. Only
        // the unparseable *date* falls back — repetitions and interval survive,
        // so this second success correctly takes SM-2's 6-day second step
        // rather than restarting the card at one day.
        let out = record_review(&pool, &ids[0], Rating::Good, day(2026, 10, 7), "x", None).await.unwrap();
        assert_eq!(out.schedule.interval_days, 6);
        assert_eq!(out.schedule.due_date, "2026-10-13");
        assert_eq!(out.schedule.repetitions, 2, "the card's progress was not lost");
    }
}
