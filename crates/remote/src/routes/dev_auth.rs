use api_types::{DevLoginRequest, DevLoginResponse, ProvidersResponse};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use uuid::Uuid;

use crate::{
    AppState,
    configure_user_scope,
    db::{
        auth::AuthSessionRepository,
        organizations::OrganizationRepository,
        users::{UpsertUser, UserRepository},
    },
};

/// Deterministic UUID v5 namespace for dev auth users.
const DEV_AUTH_NAMESPACE: Uuid = Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30,
    0xc8,
]);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/dev/auth", post(dev_login))
        .route("/auth/providers", get(get_providers))
}

async fn dev_login(
    State(state): State<AppState>,
    Json(payload): Json<DevLoginRequest>,
) -> Response {
    if !state.config().auth.dev_auth() {
        return StatusCode::NOT_FOUND.into_response();
    }

    let email = payload
        .email
        .unwrap_or_else(|| "dev@localhost".to_string());
    let name = payload.name.unwrap_or_else(|| "Dev User".to_string());

    let user_id = Uuid::new_v5(&DEV_AUTH_NAMESPACE, email.as_bytes());
    let (first_name, last_name) = split_name(&name);
    let username = email.split('@').next().map(|s| s.to_string());

    let user_repo = UserRepository::new(state.pool());
    let user = match user_repo
        .upsert_user(UpsertUser {
            id: user_id,
            email: &email,
            first_name: first_name.as_deref(),
            last_name: last_name.as_deref(),
            username: username.as_deref(),
        })
        .await
    {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(?e, "dev auth: failed to upsert user");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let org_repo = OrganizationRepository::new(state.pool());
    if let Err(e) = org_repo
        .ensure_personal_org_and_admin_membership(user.id, username.as_deref())
        .await
    {
        tracing::error!(?e, "dev auth: failed to ensure personal org");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let session_repo = AuthSessionRepository::new(state.pool());
    let session = match session_repo.create(user.id, None).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(?e, "dev auth: failed to create session");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let tokens = match state.jwt().generate_tokens(&session, &user, "dev") {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(?e, "dev auth: failed to generate tokens");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if let Err(e) = session_repo
        .set_current_refresh_token(session.id, tokens.refresh_token_id)
        .await
    {
        tracing::error!(?e, "dev auth: failed to set refresh token");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    configure_user_scope(user.id, user.username.as_deref(), Some(user.email.as_str()));

    Json(DevLoginResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        user_id: user.id,
        email: user.email,
    })
    .into_response()
}

async fn get_providers(State(state): State<AppState>) -> Json<ProvidersResponse> {
    let config = &state.config().auth;
    Json(ProvidersResponse {
        github: config.github().is_some(),
        google: config.google().is_some(),
        keycloak: config.keycloak().is_some(),
        dev: config.dev_auth(),
    })
}

fn split_name(name: &str) -> (Option<String>, Option<String>) {
    let mut iter = name.split_whitespace();
    let first = iter.next().map(|s| s.to_string());
    let remainder: Vec<&str> = iter.collect();
    let last = if remainder.is_empty() {
        None
    } else {
        Some(remainder.join(" "))
    };
    (first, last)
}
