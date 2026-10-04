use sqlx::{Row, SqlitePool};

use super::new_id;
use crate::error::CoreError;
use crate::models::{Card, CardKind, CardSearchHit, CardUpdate, NewCard};

const COLUMNS: &str = "id, deck_id, order_index, kind, front, back, options_json, explanation, tags_json, \
    source_excerpt, created_at, updated_at";

/// Both JSON columns are read the same way `practice_questions.options_json` is
/// (repo/practice_tests.rs): a malformed blob degrades to the empty case rather
/// than failing the whole read, since one bad row should not make a deck
/// unopenable.
fn json_list(raw: Option<String>) -> Option<Vec<String>> {
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
}

fn row_to_card(r: sqlx::sqlite::SqliteRow) -> Result<Card, CoreError> {
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

fn encode_list(list: &Option<Vec<String>>) -> Option<String> {
    list.as_ref().map(|v| serde_json::to_string(v).unwrap_or_else(|_| "[]".to_string()))
}

fn validate(front: &str, back: &str) -> Result<(), CoreError> {
    if front.trim().is_empty() {
        return Err(CoreError::Validation("a card needs a front".into()));
    }
    if back.trim().is_empty() {
        return Err(CoreError::Validation("a card needs a back".into()));
    }
    Ok(())
}

/// Appends to the end of the deck when `order_index` is not given, so bulk
/// inserts keep the order they arrived in without the caller tracking it.
async fn next_order_index(pool: &SqlitePool, deck_id: &str) -> Result<i64, CoreError> {
    let max: Option<i64> = sqlx::query_scalar("SELECT MAX(order_index) FROM cards WHERE deck_id = ?")
        .bind(deck_id)
        .fetch_one(pool)
        .await?;
    Ok(max.map_or(0, |m| m + 1))
}

pub async fn create(pool: &SqlitePool, deck_id: &str, input: NewCard) -> Result<Card, CoreError> {
    validate(&input.front, &input.back)?;
    let order_index = match input.order_index {
        Some(i) => i,
        None => next_order_index(pool, deck_id).await?,
    };
    let id = new_id();
    sqlx::query(
        "INSERT INTO cards (id, deck_id, order_index, kind, front, back, options_json, explanation, tags_json, source_excerpt) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(deck_id)
    .bind(order_index)
    .bind(input.kind.unwrap_or(CardKind::Basic).as_str())
    .bind(input.front.trim())
    .bind(input.back.trim())
    .bind(encode_list(&input.options))
    .bind(&input.explanation)
    .bind(encode_list(&input.tags))
    .bind(&input.source_excerpt)
    .execute(pool)
    .await?;
    get(pool, &id).await?.ok_or_else(|| CoreError::NotFound(id))
}

/// One transaction for the whole batch, like
/// `practice_tests::insert_questions` — a half-saved set of approved cards
/// would be worse than none, since the student cannot tell which are missing.
pub async fn create_many(pool: &SqlitePool, deck_id: &str, inputs: Vec<NewCard>) -> Result<u64, CoreError> {
    for input in &inputs {
        validate(&input.front, &input.back)?;
    }
    let mut order_index = next_order_index(pool, deck_id).await?;
    let mut tx = pool.begin().await?;
    let mut written = 0u64;
    for input in inputs {
        sqlx::query(
            "INSERT INTO cards (id, deck_id, order_index, kind, front, back, options_json, explanation, tags_json, source_excerpt) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(deck_id)
        .bind(input.order_index.unwrap_or(order_index))
        .bind(input.kind.unwrap_or(CardKind::Basic).as_str())
        .bind(input.front.trim())
        .bind(input.back.trim())
        .bind(encode_list(&input.options))
        .bind(&input.explanation)
        .bind(encode_list(&input.tags))
        .bind(&input.source_excerpt)
        .execute(&mut *tx)
        .await?;
        order_index += 1;
        written += 1;
    }
    tx.commit().await?;
    Ok(written)
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Card>, CoreError> {
    let row = sqlx::query(&format!("SELECT {COLUMNS} FROM cards WHERE id = ?")).bind(id).fetch_optional(pool).await?;
    row.map(row_to_card).transpose()
}

pub async fn list_by_deck(pool: &SqlitePool, deck_id: &str) -> Result<Vec<Card>, CoreError> {
    let rows = sqlx::query(&format!("SELECT {COLUMNS} FROM cards WHERE deck_id = ? ORDER BY order_index, created_at"))
        .bind(deck_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_card).collect()
}

pub async fn update(pool: &SqlitePool, id: &str, patch: CardUpdate) -> Result<Card, CoreError> {
    let existing = get(pool, id).await?.ok_or_else(|| CoreError::NotFound(id.to_string()))?;

    let front = patch.front.unwrap_or(existing.front);
    let back = patch.back.unwrap_or(existing.back);
    validate(&front, &back)?;

    let kind = patch.kind.unwrap_or(existing.kind);
    let order_index = patch.order_index.unwrap_or(existing.order_index);
    let deck_id = patch.deck_id.unwrap_or(existing.deck_id);
    // `Patch` so a student can delete a bad generated explanation or a wrong
    // set of distractors, not merely overwrite them.
    let options = patch.options.unwrap_or(existing.options);
    let explanation = patch.explanation.unwrap_or(existing.explanation);
    let tags = patch.tags.unwrap_or(Some(existing.tags));

    sqlx::query(
        "UPDATE cards SET deck_id = ?, order_index = ?, kind = ?, front = ?, back = ?, options_json = ?, \
         explanation = ?, tags_json = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?",
    )
    .bind(&deck_id)
    .bind(order_index)
    .bind(kind.as_str())
    .bind(front.trim())
    .bind(back.trim())
    .bind(encode_list(&options))
    .bind(&explanation)
    .bind(encode_list(&tags))
    .bind(id)
    .execute(pool)
    .await?;

    get(pool, id).await?.ok_or_else(|| CoreError::NotFound(id.to_string()))
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), CoreError> {
    let result = sqlx::query("DELETE FROM cards WHERE id = ?").bind(id).execute(pool).await?;
    if result.rows_affected() == 0 {
        return Err(CoreError::NotFound(id.to_string()));
    }
    Ok(())
}

/// Case-insensitive substring search across every card in every deck, carrying
/// the deck and course name so a hit is identifiable.
///
/// A `LIKE` scan rather than an FTS5 index on purpose. FTS5 *is* available (the
/// workspace's sqlx `sqlite` feature bundles SQLite built with it), but an
/// external-content index needs either this schema's first triggers or three
/// extra writes on every card mutation to stay in sync — real complexity to
/// beat a scan that is imperceptible over the low thousands of rows one student
/// produces. Revisit when a scan is actually slow.
pub async fn search(pool: &SqlitePool, query: &str, limit: i64) -> Result<Vec<CardSearchHit>, CoreError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    // `\` escapes the LIKE wildcards so searching for a literal `%` or `_`
    // does not match everything.
    let escaped = trimmed.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let pattern = format!("%{escaped}%");

    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS}, d.name AS deck_name, co.name AS course_name \
         FROM cards c JOIN decks d ON d.id = c.deck_id \
         LEFT JOIN courses co ON co.id = d.course_id \
         WHERE c.front LIKE ?1 ESCAPE '\\' OR c.back LIKE ?1 ESCAPE '\\' OR c.tags_json LIKE ?1 ESCAPE '\\' \
         ORDER BY d.name COLLATE NOCASE, c.order_index LIMIT ?2",
        COLUMNS = COLUMNS.split(", ").map(|c| format!("c.{c}")).collect::<Vec<_>>().join(", ")
    ))
    .bind(&pattern)
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|r| {
            let deck_name: String = r.try_get("deck_name")?;
            let course_name: Option<String> = r.try_get("course_name")?;
            Ok(CardSearchHit { card: row_to_card(r)?, deck_name, course_name })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::models::{NewCourse, NewDeck, NewSemester};
    use crate::repo::{courses, decks, semesters};

    fn card(front: &str, back: &str) -> NewCard {
        NewCard {
            front: front.into(),
            back: back.into(),
            kind: None,
            options: None,
            explanation: None,
            tags: None,
            source_excerpt: None,
            order_index: None,
        }
    }

    async fn seed_deck(pool: &SqlitePool) -> String {
        decks::create(pool, NewDeck { course_id: None, name: "Deck".into(), description: None, color: None })
            .await
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn create_and_read_round_trips_with_every_optional_field() {
        let pool = connect_in_memory().await.unwrap();
        let deck_id = seed_deck(&pool).await;

        let created = create(
            &pool,
            &deck_id,
            NewCard {
                front: "  What is Avogadro's number?  ".into(),
                back: "  6.022e23  ".into(),
                kind: Some(CardKind::MultipleChoice),
                options: Some(vec!["6.022e23".into(), "3.14".into()]),
                explanation: Some("Particles per mole.".into()),
                tags: Some(vec!["stoichiometry".into()]),
                source_excerpt: Some("Chapter 3, page 51".into()),
                order_index: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(created.front, "What is Avogadro's number?", "front is trimmed");
        assert_eq!(created.back, "6.022e23");
        assert_eq!(created.kind, CardKind::MultipleChoice);
        assert_eq!(created.options.as_ref().unwrap().len(), 2);
        assert_eq!(created.tags, vec!["stoichiometry".to_string()]);
        assert_eq!(created.source_excerpt.as_deref(), Some("Chapter 3, page 51"));
        assert_eq!(get(&pool, &created.id).await.unwrap().unwrap().id, created.id);
    }

    #[tokio::test]
    async fn a_hand_written_card_needs_no_source_excerpt() {
        let pool = connect_in_memory().await.unwrap();
        let deck_id = seed_deck(&pool).await;
        let c = create(&pool, &deck_id, card("q", "a")).await.unwrap();
        // The NOT NULL that migration 0005 deliberately does not have.
        assert!(c.source_excerpt.is_none());
        assert!(c.options.is_none());
        assert!(c.tags.is_empty());
        assert_eq!(c.kind, CardKind::Basic);
    }

    #[tokio::test]
    async fn a_blank_front_or_back_is_rejected() {
        let pool = connect_in_memory().await.unwrap();
        let deck_id = seed_deck(&pool).await;
        assert!(create(&pool, &deck_id, card("   ", "a")).await.is_err());
        assert!(create(&pool, &deck_id, card("q", "")).await.is_err());
        assert!(list_by_deck(&pool, &deck_id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn cards_append_in_order_and_list_in_it() {
        let pool = connect_in_memory().await.unwrap();
        let deck_id = seed_deck(&pool).await;
        for i in 0..4 {
            create(&pool, &deck_id, card(&format!("q{i}"), "a")).await.unwrap();
        }
        let listed = list_by_deck(&pool, &deck_id).await.unwrap();
        assert_eq!(listed.iter().map(|c| c.front.as_str()).collect::<Vec<_>>(), vec!["q0", "q1", "q2", "q3"]);
        assert_eq!(listed.iter().map(|c| c.order_index).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
    }

    #[tokio::test]
    async fn create_many_is_one_transaction_and_continues_the_ordering() {
        let pool = connect_in_memory().await.unwrap();
        let deck_id = seed_deck(&pool).await;
        create(&pool, &deck_id, card("first", "a")).await.unwrap();

        let written = create_many(&pool, &deck_id, vec![card("second", "b"), card("third", "c")]).await.unwrap();
        assert_eq!(written, 2);
        let listed = list_by_deck(&pool, &deck_id).await.unwrap();
        assert_eq!(listed.iter().map(|c| c.front.as_str()).collect::<Vec<_>>(), vec!["first", "second", "third"]);

        // One invalid card rejects the whole batch rather than saving a
        // partial set the student cannot identify.
        let before = list_by_deck(&pool, &deck_id).await.unwrap().len();
        assert!(create_many(&pool, &deck_id, vec![card("ok", "ok"), card("", "bad")]).await.is_err());
        assert_eq!(list_by_deck(&pool, &deck_id).await.unwrap().len(), before);
    }

    /// The reason `CardUpdate` uses `Patch` rather than the house `Option`
    /// convention: a student editing a generated card must be able to delete a
    /// wrong explanation, not just overwrite it with different wrong text.
    #[tokio::test]
    async fn an_explicit_null_clears_a_field_an_omitted_one_keeps_it() {
        let pool = connect_in_memory().await.unwrap();
        let deck_id = seed_deck(&pool).await;
        let c = create(
            &pool,
            &deck_id,
            NewCard {
                front: "q".into(),
                back: "a".into(),
                kind: Some(CardKind::MultipleChoice),
                options: Some(vec!["a".into(), "b".into()]),
                explanation: Some("because".into()),
                tags: Some(vec!["t".into()]),
                source_excerpt: None,
                order_index: None,
            },
        )
        .await
        .unwrap();

        let edited = update(&pool, &c.id, CardUpdate { front: Some("q2".into()), ..Default::default() }).await.unwrap();
        assert_eq!(edited.front, "q2");
        assert_eq!(edited.explanation.as_deref(), Some("because"), "omitted means keep");
        assert_eq!(edited.options.as_ref().unwrap().len(), 2);
        assert_eq!(edited.tags, vec!["t".to_string()]);

        let cleared = update(
            &pool,
            &c.id,
            CardUpdate { explanation: Some(None), options: Some(None), tags: Some(None), ..Default::default() },
        )
        .await
        .unwrap();
        assert!(cleared.explanation.is_none(), "explicit null means clear");
        assert!(cleared.options.is_none());
        assert!(cleared.tags.is_empty());
        assert_eq!(cleared.front, "q2", "the earlier edit survived");
    }

    #[tokio::test]
    async fn an_update_cannot_blank_a_card_out() {
        let pool = connect_in_memory().await.unwrap();
        let deck_id = seed_deck(&pool).await;
        let c = create(&pool, &deck_id, card("q", "a")).await.unwrap();
        assert!(update(&pool, &c.id, CardUpdate { back: Some("  ".into()), ..Default::default() }).await.is_err());
        assert_eq!(get(&pool, &c.id).await.unwrap().unwrap().back, "a");
    }

    #[tokio::test]
    async fn a_card_can_move_to_another_deck() {
        let pool = connect_in_memory().await.unwrap();
        let from = seed_deck(&pool).await;
        let to = decks::create(&pool, NewDeck { course_id: None, name: "Other".into(), description: None, color: None })
            .await
            .unwrap()
            .id;
        let c = create(&pool, &from, card("q", "a")).await.unwrap();

        update(&pool, &c.id, CardUpdate { deck_id: Some(to.clone()), ..Default::default() }).await.unwrap();
        assert!(list_by_deck(&pool, &from).await.unwrap().is_empty());
        assert_eq!(list_by_deck(&pool, &to).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn delete_is_not_found_the_second_time() {
        let pool = connect_in_memory().await.unwrap();
        let deck_id = seed_deck(&pool).await;
        let c = create(&pool, &deck_id, card("q", "a")).await.unwrap();
        delete(&pool, &c.id).await.unwrap();
        assert!(get(&pool, &c.id).await.unwrap().is_none());
        assert!(delete(&pool, &c.id).await.is_err());
    }

    #[tokio::test]
    async fn search_spans_decks_and_carries_where_each_hit_lives() {
        let pool = connect_in_memory().await.unwrap();
        let sem = semesters::create(&pool, NewSemester { name: "Fall".into(), start_date: None, end_date: None })
            .await
            .unwrap();
        let course = courses::create(
            &pool,
            NewCourse {
                semester_id: sem.id,
                name: "General Chem I".into(),
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
        let filed = decks::create(
            &pool,
            NewDeck { course_id: Some(course.id), name: "Chapter 3".into(), description: None, color: None },
        )
        .await
        .unwrap();
        let unfiled =
            decks::create(&pool, NewDeck { course_id: None, name: "Scratch".into(), description: None, color: None })
                .await
                .unwrap();

        create(&pool, &filed.id, card("What is a mole?", "6.022e23 particles")).await.unwrap();
        create(&pool, &unfiled.id, card("Define enthalpy", "Heat content at constant pressure")).await.unwrap();

        let hits = search(&pool, "mole", 50).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].deck_name, "Chapter 3");
        assert_eq!(hits[0].course_name.as_deref(), Some("General Chem I"));

        // Matches the back as well as the front, and is case-insensitive.
        assert_eq!(search(&pool, "ENTHALPY", 50).await.unwrap().len(), 1);
        assert_eq!(search(&pool, "constant PRESSURE", 50).await.unwrap().len(), 1);
        // A card in an unfiled deck still has no course name.
        assert!(search(&pool, "enthalpy", 50).await.unwrap()[0].course_name.is_none());
        assert!(search(&pool, "nothing matches this", 50).await.unwrap().is_empty());
        assert!(search(&pool, "   ", 50).await.unwrap().is_empty());
    }

    /// `%` and `_` are LIKE wildcards; without escaping, searching for one
    /// would match every card in the library.
    #[tokio::test]
    async fn search_treats_wildcards_as_literal_text() {
        let pool = connect_in_memory().await.unwrap();
        let deck_id = seed_deck(&pool).await;
        create(&pool, &deck_id, card("Yield was 95% overall", "a")).await.unwrap();
        create(&pool, &deck_id, card("No percent sign here", "b")).await.unwrap();

        assert_eq!(search(&pool, "%", 50).await.unwrap().len(), 1, "a bare % must not match everything");
        assert_eq!(search(&pool, "95%", 50).await.unwrap().len(), 1);
        assert_eq!(search(&pool, "_", 50).await.unwrap().len(), 0, "a bare _ must not match any single char");
    }

    #[tokio::test]
    async fn search_limit_is_clamped_to_something_sane() {
        let pool = connect_in_memory().await.unwrap();
        let deck_id = seed_deck(&pool).await;
        for i in 0..5 {
            create(&pool, &deck_id, card(&format!("term {i}"), "a")).await.unwrap();
        }
        assert_eq!(search(&pool, "term", 2).await.unwrap().len(), 2);
        // A nonsense limit must not mean "no results" or an error.
        assert_eq!(search(&pool, "term", 0).await.unwrap().len(), 1);
        assert_eq!(search(&pool, "term", -5).await.unwrap().len(), 1);
    }

    /// A corrupt JSON blob should cost that one field, not the whole read.
    #[tokio::test]
    async fn a_malformed_json_column_degrades_instead_of_failing_the_row() {
        let pool = connect_in_memory().await.unwrap();
        let deck_id = seed_deck(&pool).await;
        let c = create(&pool, &deck_id, card("q", "a")).await.unwrap();
        sqlx::query("UPDATE cards SET options_json = 'not json', tags_json = '{oops' WHERE id = ?")
            .bind(&c.id)
            .execute(&pool)
            .await
            .unwrap();

        let read = get(&pool, &c.id).await.unwrap().unwrap();
        assert!(read.options.is_none());
        assert!(read.tags.is_empty());
        assert_eq!(read.front, "q");
    }
}
