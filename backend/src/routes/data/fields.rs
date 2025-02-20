use super::ApiState;
use crate::{
    db,
    error::{ApiError, ApiResult, ErrorMessage, OnConstraint},
    model::data::{CreateField, Field, FieldId, FieldOptions},
    Id,
};
use anyhow::anyhow;
use axum::{
    extract::{Path, State},
    routing::{patch, post},
    Json, Router,
};
use tracing::debug;

const INVALID_RANGE: ErrorMessage =
    ErrorMessage::new_static("range", "Range start bound is greater than end bound");
const FIELD_NAME_CONFLICT: ErrorMessage =
    ErrorMessage::new_static("name", "Field name already used for this table");

pub(crate) fn router() -> Router<ApiState> {
    Router::new().nest(
        "/tables/{table_id}/fields",
        Router::new()
            .route("/", post(create_field).get(get_fields))
            .route("/{field_id}", patch(update_field).delete(delete_field)),
    )
}

async fn create_field(
    State(ApiState { pool, .. }): State<ApiState>,
    Path(table_id): Path<Id>,
    Json(mut create_field): Json<CreateField>,
) -> ApiResult<Json<FieldId>> {
    let mut tx = pool.begin().await?;

    let user_id = db::debug_get_user_id(tx.as_mut()).await?;
    match db::check_table_ownership(tx.as_mut(), user_id, table_id).await? {
        db::Relation::Owned => {}
        db::Relation::NotOwned => return Err(ApiError::Forbidden),
        db::Relation::Absent => return Err(ApiError::NotFound),
    }

    validate_field_options(&mut create_field.options)?;

    let field_id = db::create_field(&pool, table_id, create_field.name, create_field.options)
        .await
        .on_constraint("meta_field_table_id_name_key", |_| {
            ApiError::unprocessable_entity([FIELD_NAME_CONFLICT])
        })?;

    tx.commit().await?;

    Ok(Json(FieldId { field_id }))
}

async fn get_fields(
    State(ApiState { pool, .. }): State<ApiState>,
    Path(table_id): Path<Id>,
) -> ApiResult<Json<Vec<Field>>> {
    let mut tx = pool.begin().await?;

    let user_id = db::debug_get_user_id(tx.as_mut()).await?;
    match db::check_table_ownership(tx.as_mut(), user_id, table_id).await? {
        db::Relation::Owned => {}
        db::Relation::NotOwned => return Err(ApiError::Forbidden),
        db::Relation::Absent => return Err(ApiError::NotFound),
    }

    let fields = db::get_fields(tx.as_mut(), table_id).await?;

    tx.commit().await?;

    Ok(Json(fields))
}

async fn update_field(
    State(ApiState { pool, .. }): State<ApiState>,
    Path(table_id): Path<Id>,
) -> ApiResult<Json<Field>> {
    todo!()
}

async fn delete_field(
    State(ApiState { pool, .. }): State<ApiState>,
    Path((table_id, field_id)): Path<(Id, Id)>,
) -> ApiResult<()> {
    let mut tx = pool.begin().await?;

    let user_id = db::debug_get_user_id(tx.as_mut()).await?;

    match db::check_table_ownership(tx.as_mut(), user_id, table_id).await? {
        db::Relation::Owned => {}
        db::Relation::NotOwned => return Err(ApiError::Forbidden),
        db::Relation::Absent => return Err(ApiError::NotFound),
    }

    match db::check_field_relation(tx.as_mut(), table_id, field_id).await? {
        db::Relation::Owned => {}
        db::Relation::NotOwned | db::Relation::Absent => return Err(ApiError::NotFound),
    }

    db::delete_field(tx.as_mut(), field_id).await?;

    tx.commit().await?;
    
    Ok(())
}

fn validate_field_options(options: &mut FieldOptions) -> ApiResult<()> {
    match options {
        FieldOptions::Integer {
            range_start,
            range_end,
            ..
        } => validate_range(*range_start, *range_end)?,
        FieldOptions::Decimal {
            range_start,
            range_end,
            number_precision,
            number_scale,
            ..
        } => {
            validate_range(*range_start, *range_end)?;
            *number_precision = number_precision.map(|n| n.max(1));
            *number_scale = number_scale.map(|n| n.max(0));
        }
        FieldOptions::Money {
            range_start,
            range_end,
            ..
        } => validate_range(*range_start, *range_end)?,
        FieldOptions::Progress { total_steps } => {
            *total_steps = (*total_steps).max(1);
        }
        FieldOptions::DateTime {
            range_start,
            range_end,
            // date_time_format,
            ..
        } => validate_range(*range_start, *range_end)?,
        FieldOptions::Interval { .. } => todo!(),
        FieldOptions::Enumeration {
            values,
            default_value,
            ..
        } => {
            if !values.contains_key(&default_value) {
                return Err(
                    anyhow!("enumeration field default value does not map to a value").into(),
                );
            }
        }
        // FieldOptions::CreationDate { date_time_format } => Ok(()),
        // FieldOptions::ModificationDate { date_time_format } => Ok(()),
        _ => {}
    };
    Ok(())
}

fn validate_range<T>(range_start: Option<T>, range_end: Option<T>) -> ApiResult<()>
where
    T: PartialOrd,
{
    if range_start
        .zip(range_end)
        .map_or(true, |(start, end)| start <= end)
    {
        Ok(())
    } else {
        Err(ApiError::unprocessable_entity([INVALID_RANGE]))
    }
}
