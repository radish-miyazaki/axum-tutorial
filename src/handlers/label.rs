use crate::handlers::ValidatedJson;
use crate::repositories::label::{CreateLabel, LabelRepository};
use axum::extract::Path;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use hyper::StatusCode;
use std::sync::Arc;

pub async fn create_label<T: LabelRepository>(
    Extension(repo): Extension<Arc<T>>,
    ValidatedJson(payload): ValidatedJson<CreateLabel>,
) -> Result<impl IntoResponse, StatusCode> {
    let label = repo
        .create(payload)
        .await
        .or(Err(StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok((StatusCode::CREATED, Json(label)))
}

pub async fn all_label<T: LabelRepository>(
    Extension(repo): Extension<Arc<T>>,
) -> Result<impl IntoResponse, StatusCode> {
    let labels = repo.all().await.unwrap();
    Ok((StatusCode::OK, Json(labels)))
}

pub async fn delete_label<T: LabelRepository>(
    Path(id): Path<i32>,
    Extension(repo): Extension<Arc<T>>,
) -> StatusCode {
    repo.delete(id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}
