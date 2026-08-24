use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// Claims minted by PortFolio's `/auth/token?app=coursemaster` bridge
/// endpoint (`services/crossAppToken.js` in the PortFolio repo) — the same
/// mechanism StockMan, TruthSeeker, and BPass already trust. Verifying the
/// signature/issuer/audience/expiry here is enough; there is no callback to
/// PortFolio on the hot path.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BridgeClaims {
    pub sub: String,
    pub email: String,
    #[serde(default)]
    pub role: String,
    pub exp: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthUser {
    pub sub: String,
    pub email: String,
}

pub struct AuthRejection(StatusCode, &'static str);

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

/// This app is single-user by design (a personal academic planner, not a
/// multi-tenant product) — a valid bridge token alone isn't enough, the
/// email it carries must match the configured owner. Anyone else with a
/// PortFolio account is authenticated but not authorized.
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header_value = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthRejection(StatusCode::UNAUTHORIZED, "missing Authorization header"))?;

        let token = header_value
            .strip_prefix("Bearer ")
            .ok_or(AuthRejection(StatusCode::UNAUTHORIZED, "expected a Bearer token"))?;

        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[state.jwt_issuer.clone()]);
        validation.set_audience(&[state.jwt_audience.clone()]);

        let data = decode::<BridgeClaims>(token, &DecodingKey::from_secret(state.jwt_secret.as_bytes()), &validation)
            .map_err(|_| AuthRejection(StatusCode::UNAUTHORIZED, "invalid or expired session — please sign in again"))?;

        if data.claims.email != state.owner_email {
            return Err(AuthRejection(StatusCode::FORBIDDEN, "this CourseMaster instance belongs to a different account"));
        }

        Ok(AuthUser { sub: data.claims.sub, email: data.claims.email })
    }
}
