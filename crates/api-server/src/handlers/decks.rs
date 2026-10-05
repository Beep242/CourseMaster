use academic_core::models::{
    CandidateEdits, Card, CardCandidate, CardImport, CardSearchHit, CardUpdate, Deck, DeckUpdate, NewCard, NewDeck,
};
use academic_core::repo::{card_imports, cards, decks};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::state::AppState;

/// Every handler here takes `AuthUser`. The router carries no auth layer, so a
/// handler that omits it is public — see `crates/api-server/src/auth.rs`.
pub async fn create_deck(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(input): Json<NewDeck>,
) -> Result<Json<Deck>, ApiError> {
    Ok(Json(decks::create(&state.pool, input).await?))
}

#[derive(Debug, Deserialize)]
pub struct DeckQuery {
    pub course_id: Option<String>,
}

pub async fn list_decks(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<DeckQuery>,
) -> Result<Json<Vec<Deck>>, ApiError> {
    Ok(Json(decks::list(&state.pool, query.course_id.as_deref()).await?))
}

/// 404s a missing deck rather than returning `null` with a 200. The existing
/// `get_course` returns `Option`, but that predates this and a null body is not
/// something the UI can act on.
pub async fn get_deck(State(state): State<AppState>, _user: AuthUser, Path(id): Path<String>) -> Result<Json<Deck>, ApiError> {
    decks::get(&state.pool, &id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("no deck {id}")))
}

pub async fn update_deck(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
    Json(patch): Json<DeckUpdate>,
) -> Result<Json<Deck>, ApiError> {
    Ok(Json(decks::update(&state.pool, &id, patch).await?))
}

pub async fn delete_deck(State(state): State<AppState>, _user: AuthUser, Path(id): Path<String>) -> Result<(), ApiError> {
    Ok(decks::delete(&state.pool, &id).await?)
}

pub async fn list_cards(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(deck_id): Path<String>,
) -> Result<Json<Vec<Card>>, ApiError> {
    Ok(Json(cards::list_by_deck(&state.pool, &deck_id).await?))
}

pub async fn create_card(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(deck_id): Path<String>,
    Json(input): Json<NewCard>,
) -> Result<Json<Card>, ApiError> {
    // Checked up front so adding a card to a deck that does not exist is a 404
    // rather than a foreign-key error surfacing as a 500.
    if decks::get(&state.pool, &deck_id).await?.is_none() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, format!("no deck {deck_id}")));
    }
    Ok(Json(cards::create(&state.pool, &deck_id, input).await?))
}

pub async fn update_card(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
    Json(patch): Json<CardUpdate>,
) -> Result<Json<Card>, ApiError> {
    Ok(Json(cards::update(&state.pool, &id, patch).await?))
}

pub async fn delete_card(State(state): State<AppState>, _user: AuthUser, Path(id): Path<String>) -> Result<(), ApiError> {
    Ok(cards::delete(&state.pool, &id).await?)
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub limit: Option<i64>,
}

pub async fn search_cards(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<CardSearchHit>>, ApiError> {
    Ok(Json(cards::search(&state.pool, &query.q, query.limit.unwrap_or(100)).await?))
}

#[derive(Debug, Deserialize)]
pub struct GenerateCardsBody {
    pub material: String,
    #[serde(default)]
    pub source_label: Option<String>,
    #[serde(default)]
    pub count: Option<usize>,
}

/// Runs the generation inline rather than on a background worker. One AI call
/// is capped at 180s by the provider and the client shows a pending state, so
/// a job queue would add a table and a poll loop to solve a problem this does
/// not yet have. Moving it off the request thread is its own increment.
pub async fn generate_cards(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(deck_id): Path<String>,
    Json(body): Json<GenerateCardsBody>,
) -> Result<Json<CardImport>, ApiError> {
    let deck = decks::get(&state.pool, &deck_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("no deck {deck_id}")))?;
    Ok(Json(
        document_engine::generate_cards(
            &state.pool,
            state.ai.as_ref(),
            &deck_id,
            body.source_label.as_deref(),
            &deck.name,
            &body.material,
            body.count,
        )
        .await?,
    ))
}

pub async fn list_imports(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(deck_id): Path<String>,
) -> Result<Json<Vec<CardImport>>, ApiError> {
    Ok(Json(card_imports::list_by_deck(&state.pool, &deck_id).await?))
}

pub async fn list_candidates(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(import_id): Path<String>,
) -> Result<Json<Vec<CardCandidate>>, ApiError> {
    Ok(Json(card_imports::list_candidates(&state.pool, &import_id).await?))
}

#[derive(Debug, Deserialize)]
pub struct ApproveCandidateBody {
    #[serde(default)]
    pub edits: Option<CandidateEdits>,
}

#[derive(Debug, Serialize)]
pub struct ApproveCandidateResponse {
    pub card_id: String,
}

pub async fn approve_candidate(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<ApproveCandidateBody>,
) -> Result<Json<ApproveCandidateResponse>, ApiError> {
    let card_id = card_imports::approve_candidate(&state.pool, &id, body.edits).await?;
    Ok(Json(ApproveCandidateResponse { card_id }))
}

pub async fn reject_candidate(State(state): State<AppState>, _user: AuthUser, Path(id): Path<String>) -> Result<(), ApiError> {
    Ok(card_imports::reject_candidate(&state.pool, &id).await?)
}

#[derive(Debug, Serialize)]
pub struct ApproveAllResponse {
    pub approved: u64,
}

pub async fn approve_all(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(import_id): Path<String>,
) -> Result<Json<ApproveAllResponse>, ApiError> {
    Ok(Json(ApproveAllResponse { approved: card_imports::approve_all(&state.pool, &import_id).await? }))
}
