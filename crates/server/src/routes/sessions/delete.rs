// QRAFT-CUSTOM: session soft-delete handler
use axum::{Extension, response::Json as ResponseJson};
use db::models::session::{Session, SessionError};
use utils::response::ApiResponse;

use crate::error::ApiError;
use deployment::Deployment;
use crate::DeploymentImpl;
use axum::extract::State;

pub async fn delete_session(
    Extension(session): Extension<Session>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    let pool = &deployment.db().pool;

    Session::soft_delete(pool, session.id)
        .await
        .map_err(|e| ApiError::Session(SessionError::Database(e)))?;

    Ok(ResponseJson(ApiResponse::success(())))
}
