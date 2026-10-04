use academic_core::models::{Card, CardSearchHit, CardUpdate, Deck, DeckUpdate, NewCard, NewDeck};
use academic_core::repo::{cards, decks};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

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
