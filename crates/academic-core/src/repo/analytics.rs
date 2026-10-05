use chrono::NaiveDate;
use sqlx::{Row, SqlitePool};

use crate::error::CoreError;
use crate::models::{DayAccuracy, DeckMastery, StudyTotals, WeakCard};

/// Interval at which a card counts as "known". 21 days is the conventional
/// maturity threshold in spaced repetition: long enough that recall is not
/// short-term memory, short enough to be reachable inside one semester.
pub const MATURE_INTERVAL_DAYS: i64 = 21;

/// A course filter that applies to every query here. `None` means the whole
/// library — the cross-course view is the useful one for a student with seven
/// courses, and per-course is the narrowing case.
fn course_clause(course_id: Option<&str>) -> &'static str {
    if course_id.is_some() {
        "AND d.course_id = ?1"
    } else {
        ""
    }
}

/// The cards you keep getting wrong, worst first.
///
/// Ranked by lapses then by accuracy rather than by accuracy alone: a card
/// failed 4 times out of 20 is a worse problem than one failed once out of one,
/// and raw accuracy would put the second on top.
pub async fn weak_cards(pool: &SqlitePool, course_id: Option<&str>, limit: i64) -> Result<Vec<WeakCard>, CoreError> {
    let sql = format!(
        "SELECT c.id AS card_id, c.front, d.name AS deck_name, co.name AS course_name,
                COUNT(r.id) AS total_reviews,
                SUM(CASE WHEN r.was_lapse = 1 THEN 1 ELSE 0 END) AS lapses,
                COALESCE(s.interval_days, 0) AS interval_days
         FROM cards c
         JOIN decks d ON d.id = c.deck_id
         LEFT JOIN courses co ON co.id = d.course_id
         JOIN card_reviews r ON r.card_id = c.id
         LEFT JOIN card_schedule s ON s.card_id = c.id
         WHERE 1 = 1 {}
         GROUP BY c.id
         HAVING lapses > 0
         ORDER BY lapses DESC, (CAST(lapses AS REAL) / COUNT(r.id)) DESC, c.front
         LIMIT ?2",
        course_clause(course_id)
    );
    let mut query = sqlx::query(&sql);
    query = query.bind(course_id.unwrap_or(""));
    let rows = query.bind(limit.clamp(1, 200)).fetch_all(pool).await?;

    rows.into_iter()
        .map(|r| {
            let total: i64 = r.try_get("total_reviews")?;
            let lapses: i64 = r.try_get("lapses")?;
            Ok(WeakCard {
                card_id: r.try_get("card_id")?,
                front: r.try_get("front")?,
                deck_name: r.try_get("deck_name")?,
                course_name: r.try_get("course_name")?,
                total_reviews: total,
                lapses,
                accuracy: if total > 0 { (total - lapses) as f64 / total as f64 } else { 0.0 },
                interval_days: r.try_get("interval_days")?,
            })
        })
        .collect()
}

/// Correct/total per local calendar day, most recent last.
///
/// Grouped on `local_date` — the client's day, not the server's UTC one — so a
/// late-night session counts toward the day the student experienced.
pub async fn accuracy_history(pool: &SqlitePool, course_id: Option<&str>, days: i64) -> Result<Vec<DayAccuracy>, CoreError> {
    let sql = format!(
        "SELECT r.local_date AS day,
                COUNT(*) AS total,
                SUM(CASE WHEN r.was_lapse = 0 THEN 1 ELSE 0 END) AS correct,
                COALESCE(SUM(r.duration_ms), 0) AS duration_ms
         FROM card_reviews r
         JOIN cards c ON c.id = r.card_id
         JOIN decks d ON d.id = c.deck_id
         WHERE 1 = 1 {}
         GROUP BY r.local_date
         ORDER BY r.local_date DESC
         LIMIT ?2",
        course_clause(course_id)
    );
    let mut query = sqlx::query(&sql);
    query = query.bind(course_id.unwrap_or(""));
    let rows = query.bind(days.clamp(1, 365)).fetch_all(pool).await?;

    let mut out: Vec<DayAccuracy> = rows
        .into_iter()
        .map(|r| {
            let total: i64 = r.try_get("total")?;
            let correct: i64 = r.try_get("correct")?;
            Ok(DayAccuracy {
                day: r.try_get("day")?,
                total,
                correct,
                accuracy: if total > 0 { correct as f64 / total as f64 } else { 0.0 },
                duration_ms: r.try_get("duration_ms")?,
            })
        })
        .collect::<Result<_, CoreError>>()?;
    // Queried newest-first so LIMIT keeps the recent days; returned
    // oldest-first because that is the order a chart draws.
    out.reverse();
    Ok(out)
}

pub async fn study_totals(pool: &SqlitePool, course_id: Option<&str>) -> Result<StudyTotals, CoreError> {
    let sql = format!(
        "SELECT COUNT(*) AS reviews,
                COUNT(DISTINCT r.local_date) AS days_studied,
                COALESCE(SUM(r.duration_ms), 0) AS duration_ms,
                SUM(CASE WHEN r.was_lapse = 0 THEN 1 ELSE 0 END) AS correct
         FROM card_reviews r
         JOIN cards c ON c.id = r.card_id
         JOIN decks d ON d.id = c.deck_id
         WHERE 1 = 1 {}",
        course_clause(course_id)
    );
    let row = sqlx::query(&sql).bind(course_id.unwrap_or("")).fetch_one(pool).await?;
    let reviews: i64 = row.try_get("reviews")?;
    let correct: i64 = row.try_get("correct").unwrap_or(0);
    Ok(StudyTotals {
        reviews,
        correct,
        accuracy: if reviews > 0 { correct as f64 / reviews as f64 } else { 0.0 },
        days_studied: row.try_get("days_studied")?,
        duration_ms: row.try_get("duration_ms")?,
    })
}

/// Per-deck progress: how much of it exists, how much has been seen, and how
/// much has reached the maturity threshold.
///
/// Counted over `cards`, not over reviews, so a deck of 200 cards with 3
/// reviewed reads as 1.5% rather than 100% — coverage is the number that
/// actually predicts an exam.
pub async fn deck_mastery(pool: &SqlitePool, course_id: Option<&str>) -> Result<Vec<DeckMastery>, CoreError> {
    let sql = format!(
        "SELECT d.id AS deck_id, d.name AS deck_name, co.name AS course_name,
                COUNT(c.id) AS total_cards,
                SUM(CASE WHEN s.card_id IS NOT NULL THEN 1 ELSE 0 END) AS seen_cards,
                SUM(CASE WHEN COALESCE(s.interval_days, 0) >= ?2 THEN 1 ELSE 0 END) AS mature_cards
         FROM decks d
         LEFT JOIN courses co ON co.id = d.course_id
         LEFT JOIN cards c ON c.deck_id = d.id
         LEFT JOIN card_schedule s ON s.card_id = c.id
         WHERE 1 = 1 {}
         GROUP BY d.id
         ORDER BY d.name COLLATE NOCASE",
        course_clause(course_id)
    );
    let rows = sqlx::query(&sql)
        .bind(course_id.unwrap_or(""))
        .bind(MATURE_INTERVAL_DAYS)
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|r| {
            let total: i64 = r.try_get("total_cards")?;
            let mature: i64 = r.try_get("mature_cards")?;
            Ok(DeckMastery {
                deck_id: r.try_get("deck_id")?,
                deck_name: r.try_get("deck_name")?,
                course_name: r.try_get("course_name")?,
                total_cards: total,
                seen_cards: r.try_get("seen_cards")?,
                mature_cards: mature,
                mastery: if total > 0 { mature as f64 / total as f64 } else { 0.0 },
            })
        })
        .collect()
}

/// Days on which anything was reviewed, newest first — the input to a streak.
pub async fn study_days(pool: &SqlitePool) -> Result<Vec<NaiveDate>, CoreError> {
    let rows: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT local_date FROM card_reviews ORDER BY local_date DESC").fetch_all(pool).await?;
    Ok(rows.into_iter().filter_map(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::models::{NewCard, NewCourse, NewDeck, NewSemester};
    use crate::repo::{cards, courses, decks, reviews, semesters};
    use srs::Rating;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    async fn seed(pool: &SqlitePool) -> (String, String, Vec<String>) {
        let sem = semesters::create(pool, NewSemester { name: "F".into(), start_date: None, end_date: None })
            .await
            .unwrap();
        let course = courses::create(
            pool,
            NewCourse {
                semester_id: sem.id,
                name: "Chem".into(),
                code: None,
                professor_name: None,
                professor_email: None,
                credit_hours: None,
                color: None,
                external_org_unit_id: None,
            },
        )
        .await
        .unwrap();
        let deck = decks::create(
            pool,
            NewDeck { course_id: Some(course.id.clone()), name: "Ch4".into(), description: None, color: None },
        )
        .await
        .unwrap();
        let mut ids = Vec::new();
        for i in 0..4 {
            ids.push(
                cards::create(
                    pool,
                    &deck.id,
                    NewCard {
                        front: format!("q{i}"),
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
                .unwrap()
                .id,
            );
        }
        (course.id, deck.id, ids)
    }

    #[tokio::test]
    async fn everything_is_empty_before_any_review() {
        let pool = connect_in_memory().await.unwrap();
        seed(&pool).await;
        assert!(weak_cards(&pool, None, 10).await.unwrap().is_empty());
        assert!(accuracy_history(&pool, None, 30).await.unwrap().is_empty());
        let totals = study_totals(&pool, None).await.unwrap();
        assert_eq!(totals.reviews, 0);
        assert_eq!(totals.accuracy, 0.0, "no reviews must not be 100% accurate");
        // Mastery still reports the deck, at zero — a deck you have not started
        // is not absent from your progress, it is 0%.
        let mastery = deck_mastery(&pool, None).await.unwrap();
        assert_eq!(mastery.len(), 1);
        assert_eq!(mastery[0].total_cards, 4);
        assert_eq!(mastery[0].seen_cards, 0);
        assert_eq!(mastery[0].mastery, 0.0);
    }

    #[tokio::test]
    async fn weak_cards_rank_by_lapses_then_accuracy_and_exclude_clean_cards() {
        let pool = connect_in_memory().await.unwrap();
        let (_, _, ids) = seed(&pool).await;
        // ids[0]: 3 lapses. ids[1]: 1 lapse. ids[2]: never wrong.
        for _ in 0..3 {
            reviews::record_review(&pool, &ids[0], Rating::Again, day(2026, 10, 5), "2026-10-05", None).await.unwrap();
        }
        reviews::record_review(&pool, &ids[1], Rating::Again, day(2026, 10, 5), "2026-10-05", None).await.unwrap();
        reviews::record_review(&pool, &ids[2], Rating::Good, day(2026, 10, 5), "2026-10-05", None).await.unwrap();

        let weak = weak_cards(&pool, None, 10).await.unwrap();
        assert_eq!(weak.len(), 2, "a card never answered wrong is not a weak card");
        assert_eq!(weak[0].card_id, ids[0]);
        assert_eq!(weak[0].lapses, 3);
        assert_eq!(weak[1].card_id, ids[1]);
        assert_eq!(weak[0].deck_name, "Ch4");
        assert_eq!(weak[0].course_name.as_deref(), Some("Chem"));
    }

    #[tokio::test]
    async fn accuracy_history_groups_by_the_students_local_day_oldest_first() {
        let pool = connect_in_memory().await.unwrap();
        let (_, _, ids) = seed(&pool).await;
        reviews::record_review(&pool, &ids[0], Rating::Good, day(2026, 10, 5), "2026-10-05", Some(1000)).await.unwrap();
        reviews::record_review(&pool, &ids[1], Rating::Again, day(2026, 10, 5), "2026-10-05", Some(2000)).await.unwrap();
        reviews::record_review(&pool, &ids[2], Rating::Good, day(2026, 10, 6), "2026-10-06", Some(500)).await.unwrap();

        let history = accuracy_history(&pool, None, 30).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].day, "2026-10-05", "oldest first, the order a chart draws");
        assert_eq!(history[0].total, 2);
        assert_eq!(history[0].correct, 1);
        assert!((history[0].accuracy - 0.5).abs() < 1e-9);
        assert_eq!(history[0].duration_ms, 3000);
        assert_eq!(history[1].day, "2026-10-06");
        assert_eq!(history[1].accuracy, 1.0);
    }

    #[tokio::test]
    async fn study_totals_count_distinct_days_not_reviews() {
        let pool = connect_in_memory().await.unwrap();
        let (_, _, ids) = seed(&pool).await;
        for id in ids.iter().take(3) {
            reviews::record_review(&pool, id, Rating::Good, day(2026, 10, 5), "2026-10-05", Some(1000)).await.unwrap();
        }
        reviews::record_review(&pool, &ids[0], Rating::Again, day(2026, 10, 6), "2026-10-06", Some(4000)).await.unwrap();

        let t = study_totals(&pool, None).await.unwrap();
        assert_eq!(t.reviews, 4);
        assert_eq!(t.correct, 3);
        assert_eq!(t.days_studied, 2);
        assert_eq!(t.duration_ms, 7000);
        assert!((t.accuracy - 0.75).abs() < 1e-9);
    }

    /// Mastery is over the deck's cards, not over what has been reviewed — a
    /// deck of 4 with 1 mature card is 25%, not 100%.
    #[tokio::test]
    async fn mastery_is_coverage_not_accuracy() {
        let pool = connect_in_memory().await.unwrap();
        let (_, _, ids) = seed(&pool).await;
        // Drive one card past the maturity threshold.
        let mut d = 5;
        for _ in 0..5 {
            reviews::record_review(&pool, &ids[0], Rating::Easy, day(2026, 10, d), "x", None).await.unwrap();
            d += 1;
        }
        let m = &deck_mastery(&pool, None).await.unwrap()[0];
        assert_eq!(m.total_cards, 4);
        assert_eq!(m.seen_cards, 1);
        assert_eq!(m.mature_cards, 1);
        assert!((m.mastery - 0.25).abs() < 1e-9, "1 of 4 cards mature is 25%, not 100%");
    }

    #[tokio::test]
    async fn the_course_filter_narrows_every_query() {
        let pool = connect_in_memory().await.unwrap();
        let (course_id, _, ids) = seed(&pool).await;
        // A second, unrelated course.
        let other_deck =
            decks::create(&pool, NewDeck { course_id: None, name: "Other".into(), description: None, color: None })
                .await
                .unwrap();
        let other_card = cards::create(
            &pool,
            &other_deck.id,
            NewCard {
                front: "other".into(),
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
        reviews::record_review(&pool, &ids[0], Rating::Again, day(2026, 10, 5), "2026-10-05", None).await.unwrap();
        reviews::record_review(&pool, &other_card.id, Rating::Again, day(2026, 10, 5), "2026-10-05", None).await.unwrap();

        assert_eq!(weak_cards(&pool, None, 10).await.unwrap().len(), 2);
        assert_eq!(weak_cards(&pool, Some(&course_id), 10).await.unwrap().len(), 1);
        assert_eq!(study_totals(&pool, Some(&course_id)).await.unwrap().reviews, 1);
        assert_eq!(deck_mastery(&pool, Some(&course_id)).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn study_days_are_distinct_and_newest_first() {
        let pool = connect_in_memory().await.unwrap();
        let (_, _, ids) = seed(&pool).await;
        for (i, date) in ["2026-10-05", "2026-10-05", "2026-10-07"].iter().enumerate() {
            reviews::record_review(&pool, &ids[i], Rating::Good, day(2026, 10, 5), date, None).await.unwrap();
        }
        let days = study_days(&pool).await.unwrap();
        assert_eq!(days, vec![day(2026, 10, 7), day(2026, 10, 5)]);
    }
}
