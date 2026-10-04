use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// Claims minted by PortFolio's `/api/auth/token?app=coursemaster` bridge
/// endpoint (`services/crossAppToken.js` in the PortFolio repo) — the same
/// mechanism StockMan, TruthSeeker, and BPass already trust. Verifying the
/// signature/issuer/audience/expiry here is enough; there is no callback to
/// PortFolio on the hot path.
///
/// `iss` and `aud` are declared as required `String`s deliberately, on both
/// counts. **Required**, because a token that simply omits them would
/// otherwise be accepted (see `BridgeVerifier::verify`). **`String` and not a
/// sequence**, because the minting code sets `aud` to a single audience name
/// — typing it as a list would reject every real token and lock the owner out.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BridgeClaims {
    pub sub: String,
    pub email: String,
    #[serde(default)]
    pub role: String,
    pub iss: String,
    pub aud: String,
    pub exp: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthUser {
    pub sub: String,
    pub email: String,
}

#[derive(Debug)]
pub struct AuthRejection(StatusCode, &'static str);

impl AuthRejection {
    #[cfg(test)]
    fn status(&self) -> StatusCode {
        self.0
    }
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

/// Everything needed to decide whether one bearer token may act as the owner.
/// Split out of the extractor so the rules below are unit-testable without
/// standing up an `AppState`, a database, or an HTTP request.
pub struct BridgeVerifier<'a> {
    pub secret: &'a str,
    pub issuer: &'a str,
    pub audience: &'a str,
    pub owner_email: &'a str,
}

impl BridgeVerifier<'_> {
    pub fn verify(&self, token: &str) -> Result<AuthUser, AuthRejection> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[self.issuer]);
        validation.set_audience(&[self.audience]);
        // `Validation::new` requires only `exp`, and jsonwebtoken skips the
        // issuer/audience checks for a claim that is absent rather than
        // failing — so without this line a correctly-signed token that simply
        // omits `iss` and `aud` passes every check here. That matters because
        // the signing secret is shared with PortFolio, StockMan, TruthSeeker
        // and BPass: a token minted for another app in the suite must not be
        // usable against this one.
        validation.set_required_spec_claims(&["exp", "iss", "aud"]);

        let data = decode::<BridgeClaims>(token, &DecodingKey::from_secret(self.secret.as_bytes()), &validation)
            .map_err(|_| AuthRejection(StatusCode::UNAUTHORIZED, "invalid or expired session — please sign in again"))?;

        if data.claims.email != self.owner_email {
            return Err(AuthRejection(StatusCode::FORBIDDEN, "this CourseMaster instance belongs to a different account"));
        }

        Ok(AuthUser { sub: data.claims.sub, email: data.claims.email })
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

        BridgeVerifier {
            secret: &state.jwt_secret,
            issuer: &state.jwt_issuer,
            audience: &state.jwt_audience,
            owner_email: &state.owner_email,
        }
        .verify(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde_json::json;

    const SECRET: &str = "test-secret-shared-across-the-suite";
    const ISSUER: &str = "iambeep.com";
    const AUDIENCE: &str = "coursemaster";
    const OWNER: &str = "owner@example.com";

    fn verifier() -> BridgeVerifier<'static> {
        BridgeVerifier { secret: SECRET, issuer: ISSUER, audience: AUDIENCE, owner_email: OWNER }
    }

    fn future() -> i64 {
        chrono::Utc::now().timestamp() + 900
    }

    /// Signs an arbitrary claim set, so a test can mint the malformed tokens
    /// that `BridgeClaims` itself cannot represent (one with no `aud`, say).
    fn sign(claims: serde_json::Value, secret: &str) -> String {
        encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(secret.as_bytes())).unwrap()
    }

    fn owner_claims() -> serde_json::Value {
        json!({ "sub": "42", "email": OWNER, "role": "admin", "iss": ISSUER, "aud": AUDIENCE, "exp": future() })
    }

    #[test]
    fn accepts_a_well_formed_owner_token() {
        let user = verifier().verify(&sign(owner_claims(), SECRET)).unwrap();
        assert_eq!(user.sub, "42");
        assert_eq!(user.email, OWNER);
    }

    /// The regression this file exists for: jsonwebtoken does not fail a claim
    /// it cannot find, so an omitted `aud` used to sail through.
    #[test]
    fn rejects_a_token_with_no_audience_claim() {
        let claims = json!({ "sub": "42", "email": OWNER, "iss": ISSUER, "exp": future() });
        let err = verifier().verify(&sign(claims, SECRET)).unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn rejects_a_token_with_no_issuer_claim() {
        let claims = json!({ "sub": "42", "email": OWNER, "aud": AUDIENCE, "exp": future() });
        let err = verifier().verify(&sign(claims, SECRET)).unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    /// A token minted for a sibling app, signed with the same shared secret.
    #[test]
    fn rejects_a_token_for_another_app_in_the_suite() {
        let mut claims = owner_claims();
        claims["aud"] = json!("truthseeker");
        let err = verifier().verify(&sign(claims, SECRET)).unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn rejects_an_unexpected_issuer() {
        let mut claims = owner_claims();
        claims["iss"] = json!("evil.example.com");
        let err = verifier().verify(&sign(claims, SECRET)).unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn rejects_a_token_signed_with_the_wrong_secret() {
        let err = verifier().verify(&sign(owner_claims(), "not-the-real-secret")).unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn rejects_an_expired_token() {
        let mut claims = owner_claims();
        claims["exp"] = json!(chrono::Utc::now().timestamp() - 3600);
        let err = verifier().verify(&sign(claims, SECRET)).unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    /// `Validation` carries a 60s default `leeway`, so a token a few seconds
    /// past `exp` is still accepted. That is deliberate (it absorbs clock skew
    /// between this host and PortFolio's) and the bridge TTL is 900s, but it
    /// surprised a first draft of the test above — so it is pinned here rather
    /// than left to be rediscovered.
    #[test]
    fn a_token_just_past_expiry_is_still_within_the_default_leeway() {
        let mut claims = owner_claims();
        claims["exp"] = json!(chrono::Utc::now().timestamp() - 5);
        assert!(verifier().verify(&sign(claims, SECRET)).is_ok());
    }

    /// Authenticated but not authorized — the single-owner gate, which is a
    /// 403 rather than a 401 because signing in again would not help.
    #[test]
    fn rejects_a_valid_token_for_a_different_account() {
        let mut claims = owner_claims();
        claims["email"] = json!("someone.else@example.com");
        let err = verifier().verify(&sign(claims, SECRET)).unwrap_err();
        assert_eq!(err.status(), StatusCode::FORBIDDEN);
    }

    /// `role` is `#[serde(default)]`, so a mint that stops sending it must not
    /// start locking the owner out.
    #[test]
    fn tolerates_a_missing_role_claim() {
        let claims = json!({ "sub": "7", "email": OWNER, "iss": ISSUER, "aud": AUDIENCE, "exp": future() });
        assert_eq!(verifier().verify(&sign(claims, SECRET)).unwrap().sub, "7");
    }

    /// Guards the wire shape: PortFolio mints `aud` as a single string. If it
    /// ever sends a list, this test is where that shows up as a deliberate
    /// decision rather than as a production lockout.
    #[test]
    fn an_audience_sent_as_a_list_is_not_currently_accepted() {
        let mut claims = owner_claims();
        claims["aud"] = json!([AUDIENCE]);
        assert!(verifier().verify(&sign(claims, SECRET)).is_err());
    }
}
