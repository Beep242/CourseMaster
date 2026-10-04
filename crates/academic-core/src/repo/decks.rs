use sqlx::{Row, SqlitePool};

use super::new_id;
use crate::error::CoreError;
use crate::models::{Deck, DeckUpdate, NewDeck};

/// `card_count` is computed rather than cached in a column: there is no trigger
/// anywhere in this schema, so a cached count would drift the first time a card
/// was deleted by a cascade rather than through `cards::delete`.
const SELECT_DECK: &str = "SELECT d.id, d.course_id, d.name, d.description, d.color, d.created_at, d.updated_at, \
     (SELECT COUNT(*) FROM cards c WHERE c.deck_id = d.id) AS card_count FROM decks d";

fn row_to_deck(r: sqlx::sqlite::SqliteRow) -> Result<Deck, CoreError> {
    Ok(Deck {
        id: r.try_get("id")?,
        course_id: r.try_get("course_id")?,
        name: r.try_get("name")?,
        description: r.try_get("description")?,
        color: r.try_get("color")?,
        card_count: r.try_get("card_count")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

pub async fn create(pool: &SqlitePool, input: NewDeck) -> Result<Deck, CoreError> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(CoreError::Validation("deck name is required".into()));
    }
    let id = new_id();
    sqlx::query("INSERT INTO decks (id, course_id, name, description, color) VALUES (?, ?, ?, ?, COALESCE(?, '#8b5cf6'))")
        .bind(&id)
        .bind(&input.course_id)
        .bind(name)
        .bind(&input.description)
        .bind(&input.color)
        .execute(pool)
        .await?;
    get(pool, &id).await?.ok_or_else(|| CoreError::NotFound(id))
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Deck>, CoreError> {
    let row = sqlx::query(&format!("{SELECT_DECK} WHERE d.id = ?")).bind(id).fetch_optional(pool).await?;
    row.map(row_to_deck).transpose()
}

/// `course_id` of `None` lists every deck, including ones not filed under any
/// course — otherwise an unfiled deck would be invisible and unrecoverable.
pub async fn list(pool: &SqlitePool, course_id: Option<&str>) -> Result<Vec<Deck>, CoreError> {
    let rows = match course_id {
        Some(cid) => {
            sqlx::query(&format!("{SELECT_DECK} WHERE d.course_id = ? ORDER BY d.name COLLATE NOCASE"))
                .bind(cid)
                .fetch_all(pool)
                .await?
        }
        None => sqlx::query(&format!("{SELECT_DECK} ORDER BY d.name COLLATE NOCASE")).fetch_all(pool).await?,
    };
    rows.into_iter().map(row_to_deck).collect()
}

pub async fn update(pool: &SqlitePool, id: &str, patch: DeckUpdate) -> Result<Deck, CoreError> {
    let existing = get(pool, id).await?.ok_or_else(|| CoreError::NotFound(id.to_string()))?;

    let name = match patch.name {
        Some(n) if n.trim().is_empty() => return Err(CoreError::Validation("deck name is required".into())),
        Some(n) => n.trim().to_string(),
        None => existing.name,
    };
    // `Patch` distinguishes absent (keep) from an explicit null (clear); see
    // `models::Patch`.
    let course_id = patch.course_id.unwrap_or(existing.course_id);
    let description = patch.description.unwrap_or(existing.description);
    let color = patch.color.unwrap_or(existing.color);

    sqlx::query(
        "UPDATE decks SET course_id = ?, name = ?, description = ?, color = ?, \
         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?",
    )
    .bind(&course_id)
    .bind(&name)
    .bind(&description)
    .bind(&color)
    .bind(id)
    .execute(pool)
    .await?;

    get(pool, id).await?.ok_or_else(|| CoreError::NotFound(id.to_string()))
}

/// Cards go with it, via `ON DELETE CASCADE` in migration 0005 — which only
/// fires because `db::connect` sets `foreign_keys(true)`.
pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), CoreError> {
    let result = sqlx::query("DELETE FROM decks WHERE id = ?").bind(id).execute(pool).await?;
    if result.rows_affected() == 0 {
        return Err(CoreError::NotFound(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::models::{NewCard, NewCourse, NewSemester};
    use crate::repo::{cards, courses, semesters};

    async fn seed_course(pool: &SqlitePool) -> String {
        let sem = semesters::create(pool, NewSemester { name: "Fall".into(), start_date: None, end_date: None })
            .await
            .unwrap();
        courses::create(
            pool,
            NewCourse {
                semester_id: sem.id,
                name: "General Chem I".into(),
                code: Some("CHEM-107".into()),
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

    fn new_deck(course_id: Option<String>, name: &str) -> NewDeck {
        NewDeck { course_id, name: name.into(), description: None, color: None }
    }

    #[tokio::test]
    async fn create_get_list_round_trips() {
        let pool = connect_in_memory().await.unwrap();
        let course_id = seed_course(&pool).await;

        let deck = create(&pool, new_deck(Some(course_id.clone()), "  Chapter 4  ")).await.unwrap();
        assert_eq!(deck.name, "Chapter 4", "name is trimmed");
        assert_eq!(deck.color, "#8b5cf6", "falls back to the default colour");
        assert_eq!(deck.card_count, 0);

        assert_eq!(get(&pool, &deck.id).await.unwrap().unwrap().id, deck.id);
        assert_eq!(list(&pool, Some(&course_id)).await.unwrap().len(), 1);
        assert!(get(&pool, "nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_deck_needs_no_course() {
        let pool = connect_in_memory().await.unwrap();
        let deck = create(&pool, new_deck(None, "Unfiled")).await.unwrap();
        assert!(deck.course_id.is_none());
        // And must still be reachable, or it would be stranded.
        assert_eq!(list(&pool, None).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn listing_by_course_excludes_other_courses_but_listing_all_includes_unfiled() {
        let pool = connect_in_memory().await.unwrap();
        let course_id = seed_course(&pool).await;
        create(&pool, new_deck(Some(course_id.clone()), "Filed")).await.unwrap();
        create(&pool, new_deck(None, "Unfiled")).await.unwrap();

        let by_course = list(&pool, Some(&course_id)).await.unwrap();
        assert_eq!(by_course.len(), 1);
        assert_eq!(by_course[0].name, "Filed");
        assert_eq!(list(&pool, None).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn an_empty_name_is_rejected_on_create_and_on_update() {
        let pool = connect_in_memory().await.unwrap();
        assert!(create(&pool, new_deck(None, "   ")).await.is_err());
        let deck = create(&pool, new_deck(None, "Fine")).await.unwrap();
        assert!(update(&pool, &deck.id, DeckUpdate { name: Some("  ".into()), ..Default::default() }).await.is_err());
        // And the original survives the rejected update.
        assert_eq!(get(&pool, &deck.id).await.unwrap().unwrap().name, "Fine");
    }

    /// The reason `DeckUpdate` uses `Patch`: an omitted field keeps its value,
    /// an explicit null clears it. A plain `Option` cannot express both.
    #[tokio::test]
    async fn an_omitted_field_keeps_its_value_and_an_explicit_null_clears_it() {
        let pool = connect_in_memory().await.unwrap();
        let course_id = seed_course(&pool).await;
        let deck = create(
            &pool,
            NewDeck {
                course_id: Some(course_id),
                name: "Chapter 4".into(),
                description: Some("acids and bases".into()),
                color: Some("#34d399".into()),
            },
        )
        .await
        .unwrap();

        // Rename only: everything else is untouched.
        let renamed = update(&pool, &deck.id, DeckUpdate { name: Some("Chapter 5".into()), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(renamed.name, "Chapter 5");
        assert_eq!(renamed.description.as_deref(), Some("acids and bases"));
        assert!(renamed.course_id.is_some());
        assert_eq!(renamed.color, "#34d399");

        // Explicit null: unfile it and drop the description.
        let cleared = update(
            &pool,
            &deck.id,
            DeckUpdate { course_id: Some(None), description: Some(None), ..Default::default() },
        )
        .await
        .unwrap();
        assert!(cleared.course_id.is_none());
        assert!(cleared.description.is_none());
        assert_eq!(cleared.name, "Chapter 5", "the rename survived");
    }

    #[tokio::test]
    async fn card_count_reflects_reality_rather_than_a_cached_column() {
        let pool = connect_in_memory().await.unwrap();
        let deck = create(&pool, new_deck(None, "Counting")).await.unwrap();
        for i in 0..3 {
            cards::create(
                &pool,
                &deck.id,
                NewCard {
                    front: format!("q{i}"),
                    back: format!("a{i}"),
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
        }
        assert_eq!(get(&pool, &deck.id).await.unwrap().unwrap().card_count, 3);
    }

    #[tokio::test]
    async fn deleting_a_deck_takes_its_cards_with_it() {
        let pool = connect_in_memory().await.unwrap();
        let deck = create(&pool, new_deck(None, "Doomed")).await.unwrap();
        let card = cards::create(
            &pool,
            &deck.id,
            NewCard {
                front: "q".into(),
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

        delete(&pool, &deck.id).await.unwrap();
        assert!(get(&pool, &deck.id).await.unwrap().is_none());
        // The cascade only fires because db::connect sets foreign_keys(true).
        assert!(cards::get(&pool, &card.id).await.unwrap().is_none());
        assert!(delete(&pool, &deck.id).await.is_err(), "deleting twice is a NotFound, not a silent success");
    }

    /// Deleting a course takes its decks, and their cards, with it.
    #[tokio::test]
    async fn deleting_a_course_cascades_all_the_way_to_cards() {
        let pool = connect_in_memory().await.unwrap();
        let course_id = seed_course(&pool).await;
        let deck = create(&pool, new_deck(Some(course_id.clone()), "Chapter 4")).await.unwrap();
        let card = cards::create(
            &pool,
            &deck.id,
            NewCard {
                front: "q".into(),
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

        courses::delete(&pool, &course_id).await.unwrap();
        assert!(get(&pool, &deck.id).await.unwrap().is_none());
        assert!(cards::get(&pool, &card.id).await.unwrap().is_none());
    }
}
