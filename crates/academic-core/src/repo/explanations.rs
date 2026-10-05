use sqlx::{Row, SqlitePool};

use super::new_id;
use crate::error::CoreError;
use crate::models::CardExplanation;

const COLUMNS: &str = "id, card_id, mistake_key, submitted, explanation, total_cost_usd, created_at";

fn row_to_explanation(r: sqlx::sqlite::SqliteRow) -> Result<CardExplanation, CoreError> {
    Ok(CardExplanation {
        id: r.try_get("id")?,
        card_id: r.try_get("card_id")?,
        mistake_key: r.try_get("mistake_key")?,
        submitted: r.try_get("submitted")?,
        explanation: r.try_get("explanation")?,
        total_cost_usd: r.try_get("total_cost_usd")?,
        created_at: r.try_get("created_at")?,
    })
}

/// The cache lookup. A hit means this exact misunderstanding has been explained
/// before and costs nothing to show again.
pub async fn find(pool: &SqlitePool, card_id: &str, mistake_key: &str) -> Result<Option<CardExplanation>, CoreError> {
    let row = sqlx::query(&format!("SELECT {COLUMNS} FROM card_explanations WHERE card_id = ? AND mistake_key = ?"))
        .bind(card_id)
        .bind(mistake_key)
        .fetch_optional(pool)
        .await?;
    row.map(row_to_explanation).transpose()
}

/// Stores an explanation. `ON CONFLICT DO UPDATE` rather than failing: two
/// requests racing on the same mistake should both succeed, with the later one
/// simply replacing the text.
pub async fn store(
    pool: &SqlitePool,
    card_id: &str,
    mistake_key: &str,
    submitted: &str,
    explanation: &str,
    total_cost_usd: Option<f64>,
) -> Result<CardExplanation, CoreError> {
    sqlx::query(
        "INSERT INTO card_explanations (id, card_id, mistake_key, submitted, explanation, total_cost_usd) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT(card_id, mistake_key) DO UPDATE SET explanation = excluded.explanation, \
           submitted = excluded.submitted, total_cost_usd = excluded.total_cost_usd",
    )
    .bind(new_id())
    .bind(card_id)
    .bind(mistake_key)
    .bind(submitted)
    .bind(explanation)
    .bind(total_cost_usd)
    .execute(pool)
    .await?;
    find(pool, card_id, mistake_key).await?.ok_or_else(|| CoreError::NotFound(card_id.to_string()))
}

pub async fn list_for_card(pool: &SqlitePool, card_id: &str) -> Result<Vec<CardExplanation>, CoreError> {
    let rows = sqlx::query(&format!("SELECT {COLUMNS} FROM card_explanations WHERE card_id = ? ORDER BY created_at"))
        .bind(card_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_explanation).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::models::{NewCard, NewDeck};
    use crate::repo::{cards, decks};

    async fn seed_card(pool: &SqlitePool) -> String {
        let deck = decks::create(pool, NewDeck { course_id: None, name: "D".into(), description: None, color: None })
            .await
            .unwrap();
        cards::create(
            pool,
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
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn a_miss_then_a_hit() {
        let pool = connect_in_memory().await.unwrap();
        let card_id = seed_card(&pool).await;
        assert!(find(&pool, &card_id, "nucleus").await.unwrap().is_none());

        store(&pool, &card_id, "nucleus", "Nucleus!", "The nucleus stores DNA.", Some(0.01)).await.unwrap();
        let hit = find(&pool, &card_id, "nucleus").await.unwrap().unwrap();
        assert_eq!(hit.explanation, "The nucleus stores DNA.");
        // The verbatim answer is kept even though the key is normalised.
        assert_eq!(hit.submitted, "Nucleus!");
        assert_eq!(hit.total_cost_usd, Some(0.01));
    }

    #[tokio::test]
    async fn storing_the_same_mistake_twice_replaces_rather_than_duplicating() {
        let pool = connect_in_memory().await.unwrap();
        let card_id = seed_card(&pool).await;
        store(&pool, &card_id, "k", "x", "first", None).await.unwrap();
        store(&pool, &card_id, "k", "x", "second", None).await.unwrap();
        assert_eq!(find(&pool, &card_id, "k").await.unwrap().unwrap().explanation, "second");
        assert_eq!(list_for_card(&pool, &card_id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn different_mistakes_on_one_card_are_cached_separately() {
        let pool = connect_in_memory().await.unwrap();
        let card_id = seed_card(&pool).await;
        store(&pool, &card_id, "nucleus", "nucleus", "about the nucleus", None).await.unwrap();
        store(&pool, &card_id, "", "", "you left it blank", None).await.unwrap();
        assert_eq!(list_for_card(&pool, &card_id).await.unwrap().len(), 2);
        assert_eq!(find(&pool, &card_id, "").await.unwrap().unwrap().explanation, "you left it blank");
    }

    #[tokio::test]
    async fn deleting_the_card_clears_its_cached_explanations() {
        let pool = connect_in_memory().await.unwrap();
        let card_id = seed_card(&pool).await;
        store(&pool, &card_id, "k", "x", "e", None).await.unwrap();
        cards::delete(&pool, &card_id).await.unwrap();
        assert!(list_for_card(&pool, &card_id).await.unwrap().is_empty());
    }
}
