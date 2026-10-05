use sqlx::{Row, SqlitePool};

use super::new_id;
use crate::error::CoreError;
use crate::models::{
    CandidateEdits, CardCandidate, CardImport, CardKind, ImportSourceKind, ImportStatus, ReviewStatus,
};

const SELECT_IMPORT: &str = "SELECT i.id, i.deck_id, i.source_kind, i.source_label, i.source_text, i.status, \
    i.error_message, i.total_cost_usd, i.created_at, i.updated_at, \
    (SELECT COUNT(*) FROM card_candidates c WHERE c.import_id = i.id AND c.review_status = 'pending') AS pending_count, \
    (SELECT COUNT(*) FROM card_candidates c WHERE c.import_id = i.id AND c.review_status IN ('approved','edited')) AS approved_count \
    FROM card_imports i";

const CANDIDATE_COLUMNS: &str = "id, import_id, order_index, kind, front, back, options_json, explanation, \
    tags_json, source_excerpt, review_status, resulting_card_id, created_at";

fn json_list(raw: Option<String>) -> Option<Vec<String>> {
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
}

fn encode_list(list: &Option<Vec<String>>) -> Option<String> {
    list.as_ref().map(|v| serde_json::to_string(v).unwrap_or_else(|_| "[]".to_string()))
}

fn row_to_import(r: sqlx::sqlite::SqliteRow) -> Result<CardImport, CoreError> {
    Ok(CardImport {
        id: r.try_get("id")?,
        deck_id: r.try_get("deck_id")?,
        source_kind: ImportSourceKind::parse(&r.try_get::<String, _>("source_kind")?),
        source_label: r.try_get("source_label")?,
        source_text: r.try_get("source_text")?,
        status: ImportStatus::parse(&r.try_get::<String, _>("status")?),
        error_message: r.try_get("error_message")?,
        total_cost_usd: r.try_get("total_cost_usd")?,
        pending_count: r.try_get("pending_count")?,
        approved_count: r.try_get("approved_count")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

fn row_to_candidate(r: sqlx::sqlite::SqliteRow) -> Result<CardCandidate, CoreError> {
    Ok(CardCandidate {
        id: r.try_get("id")?,
        import_id: r.try_get("import_id")?,
        order_index: r.try_get("order_index")?,
        kind: CardKind::parse(&r.try_get::<String, _>("kind")?),
        front: r.try_get("front")?,
        back: r.try_get("back")?,
        options: json_list(r.try_get("options_json")?),
        explanation: r.try_get("explanation")?,
        tags: json_list(r.try_get("tags_json")?).unwrap_or_default(),
        source_excerpt: r.try_get("source_excerpt")?,
        review_status: ReviewStatus::parse(&r.try_get::<String, _>("review_status")?),
        resulting_card_id: r.try_get("resulting_card_id")?,
        created_at: r.try_get("created_at")?,
    })
}

/// Created *before* the AI call, so a generation that crashes or times out
/// leaves a visible `generating`/`failed` row rather than nothing at all.
pub async fn create(
    pool: &SqlitePool,
    deck_id: &str,
    source_kind: ImportSourceKind,
    source_label: Option<&str>,
    source_text: &str,
) -> Result<CardImport, CoreError> {
    if source_text.trim().is_empty() {
        return Err(CoreError::Validation("there is no material to generate cards from".into()));
    }
    let id = new_id();
    sqlx::query("INSERT INTO card_imports (id, deck_id, source_kind, source_label, source_text) VALUES (?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(deck_id)
        .bind(source_kind.as_str())
        .bind(source_label)
        .bind(source_text)
        .execute(pool)
        .await?;
    get(pool, &id).await?.ok_or_else(|| CoreError::NotFound(id))
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<CardImport>, CoreError> {
    let row = sqlx::query(&format!("{SELECT_IMPORT} WHERE i.id = ?")).bind(id).fetch_optional(pool).await?;
    row.map(row_to_import).transpose()
}

pub async fn list_by_deck(pool: &SqlitePool, deck_id: &str) -> Result<Vec<CardImport>, CoreError> {
    let rows = sqlx::query(&format!("{SELECT_IMPORT} WHERE i.deck_id = ? ORDER BY i.created_at DESC"))
        .bind(deck_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_import).collect()
}

pub async fn set_status(
    pool: &SqlitePool,
    id: &str,
    status: ImportStatus,
    error_message: Option<&str>,
    total_cost_usd: Option<f64>,
) -> Result<(), CoreError> {
    sqlx::query(
        "UPDATE card_imports SET status = ?, error_message = ?, \
         total_cost_usd = COALESCE(?, total_cost_usd), \
         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?",
    )
    .bind(status.as_str())
    .bind(error_message)
    .bind(total_cost_usd)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub struct NewCandidate {
    pub kind: CardKind,
    pub front: String,
    pub back: String,
    pub options: Option<Vec<String>>,
    pub explanation: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source_excerpt: Option<String>,
}

/// One transaction for the batch, like `practice_tests::insert_questions`: a
/// partially-stored generation is worse than a failed one, because the student
/// cannot tell which cards are missing.
pub async fn insert_candidates(pool: &SqlitePool, import_id: &str, items: Vec<NewCandidate>) -> Result<u64, CoreError> {
    let mut tx = pool.begin().await?;
    let mut written = 0u64;
    for (index, item) in items.into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO card_candidates (id, import_id, order_index, kind, front, back, options_json, explanation, \
             tags_json, source_excerpt) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(import_id)
        .bind(index as i64)
        .bind(item.kind.as_str())
        .bind(&item.front)
        .bind(&item.back)
        .bind(encode_list(&item.options))
        .bind(&item.explanation)
        .bind(encode_list(&item.tags))
        .bind(&item.source_excerpt)
        .execute(&mut *tx)
        .await?;
        written += 1;
    }
    tx.commit().await?;
    Ok(written)
}

pub async fn list_candidates(pool: &SqlitePool, import_id: &str) -> Result<Vec<CardCandidate>, CoreError> {
    let rows = sqlx::query(&format!(
        "SELECT {CANDIDATE_COLUMNS} FROM card_candidates WHERE import_id = ? ORDER BY order_index"
    ))
    .bind(import_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(row_to_candidate).collect()
}

pub async fn get_candidate(pool: &SqlitePool, id: &str) -> Result<Option<CardCandidate>, CoreError> {
    let row = sqlx::query(&format!("SELECT {CANDIDATE_COLUMNS} FROM card_candidates WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(row_to_candidate).transpose()
}

/// Promotes one candidate into a real `cards` row.
///
/// This is the only path from a generated candidate to a card — the generator
/// itself never writes to `cards`, exactly as `approve_extraction` is the only
/// path from an extraction to an assignment. The insert and the status update
/// share a transaction so a crash can never leave an approved candidate whose
/// card does not exist.
pub async fn approve_candidate(pool: &SqlitePool, id: &str, edits: Option<CandidateEdits>) -> Result<String, CoreError> {
    let candidate = get_candidate(pool, id).await?.ok_or_else(|| CoreError::NotFound(id.to_string()))?;
    if candidate.review_status != ReviewStatus::Pending {
        return Err(CoreError::Validation(format!(
            "this card was already {}",
            candidate.review_status.as_str()
        )));
    }

    let front = edits.as_ref().and_then(|e| e.front.clone()).unwrap_or(candidate.front);
    let back = edits.as_ref().and_then(|e| e.back.clone()).unwrap_or(candidate.back);
    if front.trim().is_empty() || back.trim().is_empty() {
        return Err(CoreError::Validation("a card needs both a front and a back".into()));
    }
    let explanation = edits.as_ref().and_then(|e| e.explanation.clone()).or(candidate.explanation);
    let tags = edits.as_ref().and_then(|e| e.tags.clone()).unwrap_or(candidate.tags);
    let was_edited = edits.is_some();

    let deck_id: String = sqlx::query_scalar("SELECT deck_id FROM card_imports WHERE id = ?")
        .bind(&candidate.import_id)
        .fetch_one(pool)
        .await?;

    // Appended to the end of the deck, so approving in review order keeps that
    // order in the deck.
    let next_order: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(order_index) + 1, 0) FROM cards WHERE deck_id = ?")
        .bind(&deck_id)
        .fetch_one(pool)
        .await?;

    let card_id = new_id();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO cards (id, deck_id, order_index, kind, front, back, options_json, explanation, tags_json, source_excerpt) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&card_id)
    .bind(&deck_id)
    .bind(next_order)
    .bind(candidate.kind.as_str())
    .bind(front.trim())
    .bind(back.trim())
    .bind(encode_list(&candidate.options))
    .bind(&explanation)
    .bind(encode_list(&Some(tags)))
    .bind(&candidate.source_excerpt)
    .execute(&mut *tx)
    .await?;

    let new_status = if was_edited { ReviewStatus::Edited } else { ReviewStatus::Approved };
    sqlx::query("UPDATE card_candidates SET review_status = ?, resulting_card_id = ? WHERE id = ?")
        .bind(new_status.as_str())
        .bind(&card_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(card_id)
}

pub async fn reject_candidate(pool: &SqlitePool, id: &str) -> Result<(), CoreError> {
    let result = sqlx::query("UPDATE card_candidates SET review_status = 'rejected' WHERE id = ? AND review_status = 'pending'")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(CoreError::NotFound(id.to_string()));
    }
    Ok(())
}

/// Approves every still-pending candidate in one import. Returns how many
/// became cards. Each goes through `approve_candidate`, so "approve all" and
/// approving one at a time cannot diverge.
pub async fn approve_all(pool: &SqlitePool, import_id: &str) -> Result<u64, CoreError> {
    let pending: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM card_candidates WHERE import_id = ? AND review_status = 'pending' ORDER BY order_index",
    )
    .bind(import_id)
    .fetch_all(pool)
    .await?;
    let mut approved = 0u64;
    for id in pending {
        approve_candidate(pool, &id, None).await?;
        approved += 1;
    }
    Ok(approved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::models::NewDeck;
    use crate::repo::{cards, decks};

    async fn seed(pool: &SqlitePool) -> (String, String) {
        let deck = decks::create(pool, NewDeck { course_id: None, name: "Deck".into(), description: None, color: None })
            .await
            .unwrap();
        let import = create(pool, &deck.id, ImportSourceKind::Paste, Some("Ch4 notes"), "some material").await.unwrap();
        (deck.id, import.id)
    }

    fn candidate(front: &str, back: &str) -> NewCandidate {
        NewCandidate {
            kind: CardKind::Basic,
            front: front.into(),
            back: back.into(),
            options: None,
            explanation: Some("because".into()),
            tags: Some(vec!["t".into()]),
            source_excerpt: Some("from the notes".into()),
        }
    }

    #[tokio::test]
    async fn an_import_starts_generating_and_records_its_source() {
        let pool = connect_in_memory().await.unwrap();
        let (_, import_id) = seed(&pool).await;
        let imp = get(&pool, &import_id).await.unwrap().unwrap();
        assert_eq!(imp.status, ImportStatus::Generating);
        assert_eq!(imp.source_label.as_deref(), Some("Ch4 notes"));
        assert_eq!(imp.source_text, "some material");
        assert_eq!(imp.pending_count, 0);
        assert!(imp.total_cost_usd.is_none());
    }

    #[tokio::test]
    async fn empty_material_is_rejected_before_any_ai_call() {
        let pool = connect_in_memory().await.unwrap();
        let deck = decks::create(&pool, NewDeck { course_id: None, name: "D".into(), description: None, color: None })
            .await
            .unwrap();
        assert!(create(&pool, &deck.id, ImportSourceKind::Paste, None, "   ").await.is_err());
    }

    #[tokio::test]
    async fn candidates_are_staged_and_counted_without_touching_cards() {
        let pool = connect_in_memory().await.unwrap();
        let (deck_id, import_id) = seed(&pool).await;
        insert_candidates(&pool, &import_id, vec![candidate("q1", "a1"), candidate("q2", "a2")]).await.unwrap();

        let imp = get(&pool, &import_id).await.unwrap().unwrap();
        assert_eq!(imp.pending_count, 2);
        assert_eq!(imp.approved_count, 0);
        // The whole point: generating does not create cards.
        assert!(cards::list_by_deck(&pool, &deck_id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn approving_promotes_a_candidate_into_a_real_card() {
        let pool = connect_in_memory().await.unwrap();
        let (deck_id, import_id) = seed(&pool).await;
        insert_candidates(&pool, &import_id, vec![candidate("q1", "a1")]).await.unwrap();
        let cand = list_candidates(&pool, &import_id).await.unwrap().remove(0);

        let card_id = approve_candidate(&pool, &cand.id, None).await.unwrap();
        let card = cards::get(&pool, &card_id).await.unwrap().unwrap();
        assert_eq!(card.front, "q1");
        assert_eq!(card.deck_id, deck_id);
        assert_eq!(card.explanation.as_deref(), Some("because"));
        assert_eq!(card.tags, vec!["t".to_string()]);
        // Provenance survives into the real card, which is what later grounds
        // an explanation in the student's own material.
        assert_eq!(card.source_excerpt.as_deref(), Some("from the notes"));

        let after = get_candidate(&pool, &cand.id).await.unwrap().unwrap();
        assert_eq!(after.review_status, ReviewStatus::Approved);
        assert_eq!(after.resulting_card_id.as_deref(), Some(card_id.as_str()));
    }

    #[tokio::test]
    async fn approving_with_edits_records_that_it_was_edited() {
        let pool = connect_in_memory().await.unwrap();
        let (_, import_id) = seed(&pool).await;
        insert_candidates(&pool, &import_id, vec![candidate("bad wording", "a1")]).await.unwrap();
        let cand = list_candidates(&pool, &import_id).await.unwrap().remove(0);

        let edits = CandidateEdits { front: Some("better wording".into()), ..Default::default() };
        let card_id = approve_candidate(&pool, &cand.id, Some(edits)).await.unwrap();
        assert_eq!(cards::get(&pool, &card_id).await.unwrap().unwrap().front, "better wording");
        assert_eq!(get_candidate(&pool, &cand.id).await.unwrap().unwrap().review_status, ReviewStatus::Edited);
    }

    #[tokio::test]
    async fn a_candidate_cannot_be_approved_twice() {
        let pool = connect_in_memory().await.unwrap();
        let (deck_id, import_id) = seed(&pool).await;
        insert_candidates(&pool, &import_id, vec![candidate("q1", "a1")]).await.unwrap();
        let cand = list_candidates(&pool, &import_id).await.unwrap().remove(0);

        approve_candidate(&pool, &cand.id, None).await.unwrap();
        assert!(approve_candidate(&pool, &cand.id, None).await.is_err());
        // And double-approving must not have produced a second card.
        assert_eq!(cards::list_by_deck(&pool, &deck_id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn an_edit_that_blanks_a_card_is_refused_and_creates_nothing() {
        let pool = connect_in_memory().await.unwrap();
        let (deck_id, import_id) = seed(&pool).await;
        insert_candidates(&pool, &import_id, vec![candidate("q1", "a1")]).await.unwrap();
        let cand = list_candidates(&pool, &import_id).await.unwrap().remove(0);

        let edits = CandidateEdits { back: Some("   ".into()), ..Default::default() };
        assert!(approve_candidate(&pool, &cand.id, Some(edits)).await.is_err());
        assert!(cards::list_by_deck(&pool, &deck_id).await.unwrap().is_empty());
        // Still reviewable afterwards rather than stuck.
        assert_eq!(get_candidate(&pool, &cand.id).await.unwrap().unwrap().review_status, ReviewStatus::Pending);
    }

    #[tokio::test]
    async fn rejecting_leaves_no_card_and_cannot_be_repeated() {
        let pool = connect_in_memory().await.unwrap();
        let (deck_id, import_id) = seed(&pool).await;
        insert_candidates(&pool, &import_id, vec![candidate("q1", "a1")]).await.unwrap();
        let cand = list_candidates(&pool, &import_id).await.unwrap().remove(0);

        reject_candidate(&pool, &cand.id).await.unwrap();
        assert!(cards::list_by_deck(&pool, &deck_id).await.unwrap().is_empty());
        assert_eq!(get_candidate(&pool, &cand.id).await.unwrap().unwrap().review_status, ReviewStatus::Rejected);
        assert!(reject_candidate(&pool, &cand.id).await.is_err());
        // A rejected candidate must not be approvable afterwards.
        assert!(approve_candidate(&pool, &cand.id, None).await.is_err());
    }

    #[tokio::test]
    async fn approve_all_takes_only_the_pending_ones_and_keeps_review_order() {
        let pool = connect_in_memory().await.unwrap();
        let (deck_id, import_id) = seed(&pool).await;
        insert_candidates(
            &pool,
            &import_id,
            vec![candidate("q1", "a1"), candidate("q2", "a2"), candidate("q3", "a3")],
        )
        .await
        .unwrap();
        let listed = list_candidates(&pool, &import_id).await.unwrap();
        reject_candidate(&pool, &listed[1].id).await.unwrap();

        assert_eq!(approve_all(&pool, &import_id).await.unwrap(), 2);
        let deck_cards = cards::list_by_deck(&pool, &deck_id).await.unwrap();
        assert_eq!(deck_cards.iter().map(|c| c.front.as_str()).collect::<Vec<_>>(), vec!["q1", "q3"]);

        let imp = get(&pool, &import_id).await.unwrap().unwrap();
        assert_eq!(imp.pending_count, 0);
        assert_eq!(imp.approved_count, 2);
        // Running it again is a no-op rather than a duplicate.
        assert_eq!(approve_all(&pool, &import_id).await.unwrap(), 0);
        assert_eq!(cards::list_by_deck(&pool, &deck_id).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn status_and_cost_are_recorded_and_cost_is_not_wiped_by_a_later_update() {
        let pool = connect_in_memory().await.unwrap();
        let (_, import_id) = seed(&pool).await;
        set_status(&pool, &import_id, ImportStatus::ReadyForReview, None, Some(0.0123)).await.unwrap();
        let imp = get(&pool, &import_id).await.unwrap().unwrap();
        assert_eq!(imp.status, ImportStatus::ReadyForReview);
        assert_eq!(imp.total_cost_usd, Some(0.0123));

        // COALESCE: a later status change with no cost must keep the recorded one.
        set_status(&pool, &import_id, ImportStatus::Completed, None, None).await.unwrap();
        assert_eq!(get(&pool, &import_id).await.unwrap().unwrap().total_cost_usd, Some(0.0123));
    }

    #[tokio::test]
    async fn a_failed_generation_keeps_its_error_visible() {
        let pool = connect_in_memory().await.unwrap();
        let (_, import_id) = seed(&pool).await;
        set_status(&pool, &import_id, ImportStatus::Failed, Some("AI request timed out after 180s"), None).await.unwrap();
        let imp = get(&pool, &import_id).await.unwrap().unwrap();
        assert_eq!(imp.status, ImportStatus::Failed);
        assert!(imp.error_message.unwrap().contains("timed out"));
    }

    /// Deleting a card the student approved must not erase the record that it
    /// was reviewed — hence ON DELETE SET NULL rather than CASCADE.
    #[tokio::test]
    async fn deleting_an_approved_card_leaves_the_review_record_intact() {
        let pool = connect_in_memory().await.unwrap();
        let (_, import_id) = seed(&pool).await;
        insert_candidates(&pool, &import_id, vec![candidate("q1", "a1")]).await.unwrap();
        let cand = list_candidates(&pool, &import_id).await.unwrap().remove(0);
        let card_id = approve_candidate(&pool, &cand.id, None).await.unwrap();

        cards::delete(&pool, &card_id).await.unwrap();
        let after = get_candidate(&pool, &cand.id).await.unwrap().unwrap();
        assert_eq!(after.review_status, ReviewStatus::Approved);
        assert!(after.resulting_card_id.is_none(), "the link is cleared, the review record survives");
    }

    #[tokio::test]
    async fn deleting_a_deck_takes_its_imports_and_candidates() {
        let pool = connect_in_memory().await.unwrap();
        let (deck_id, import_id) = seed(&pool).await;
        insert_candidates(&pool, &import_id, vec![candidate("q1", "a1")]).await.unwrap();

        decks::delete(&pool, &deck_id).await.unwrap();
        assert!(get(&pool, &import_id).await.unwrap().is_none());
        assert!(list_candidates(&pool, &import_id).await.unwrap().is_empty());
    }
}
