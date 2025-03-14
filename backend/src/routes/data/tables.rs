use std::io::Cursor;

use super::ApiState;
use crate::{
    db,
    error::{ApiError, ApiResult, ErrorMessage, OnConstraint},
    io,
    model::data::{CreateTable, CreateTableData, FieldMetadata, Table, TableData, UpdateTable},
    Id,
};
use axum::{
    extract::{Multipart, Path, State},
    routing::{get, patch, post},
    Json, Router,
};
use axum_macros::debug_handler;
use itertools::Itertools;
use tracing::info;
use umya_spreadsheet::reader::xlsx;

const TABLE_NAME_CONFLICT: ErrorMessage =
    ErrorMessage::new_static("name", "Table name already used");

pub fn router() -> Router<ApiState> {
    Router::new().nest(
        "/tables",
        Router::new()
            .route("/", post(create_table).get(get_tables))
            .route("/{table-id}", patch(update_table).delete(delete_table))
            .route("/{table-id}/data", get(get_table_data))
            .route("/import", post(import_table_from_file)),
    )
}

/// Create an empty user table.
///
/// # Errors
/// - [`ApiError::Unauthorized`]: User not authenticated
/// - [`ApiError::UnprocessableEntity`]:
///     - [`TABLE_NAME_CONFLICT`]
///
async fn create_table(
    State(ApiState { pool, .. }): State<ApiState>,
    Json(create_table): Json<CreateTable>,
) -> ApiResult<Json<Table>> {
    let user_id = db::debug_get_user_id(&pool).await?;

    let table = db::create_table(&pool, user_id, create_table)
        .await
        .on_constraint("meta_table_user_id_name_key", |_| {
            ApiError::unprocessable_entity([TABLE_NAME_CONFLICT])
        })?;

    Ok(Json(table))
}

/// Update a table's meta data.
///
/// # Errors
/// - [`ApiError::Unauthorized`]: User not authenticated
/// - [`ApiError::Forbidden`]: User does not have access to that table
/// - [`ApiError::NotFound`]: Table not found
///
async fn update_table(
    State(ApiState { pool, .. }): State<ApiState>,
    Path(table_id): Path<Id>,
    Json(update_table): Json<UpdateTable>,
) -> ApiResult<Json<Table>> {
    let user_id = db::debug_get_user_id(&pool).await?;

    db::check_table_relation(&pool, user_id, table_id)
        .await?
        .to_api_result()?;

    let table = db::update_table(&pool, table_id, update_table)
        .await
        .on_constraint("meta_table_user_id_name_key", |_| {
            ApiError::unprocessable_entity([TABLE_NAME_CONFLICT])
        })?;

    Ok(Json(table))
}

/// Delete a table, including all fields and entries.
///
/// # Errors
/// - [`ApiError::Unauthorized`]: User not authenticated
/// - [`ApiError::Forbidden`]: User does not have access to that table
/// - [`ApiError::NotFound`]: Table not found
///
async fn delete_table(
    State(ApiState { pool, .. }): State<ApiState>,
    Path(table_id): Path<Id>,
) -> ApiResult<()> {
    let user_id = db::debug_get_user_id(&pool).await?;
    db::check_table_relation(&pool, user_id, table_id)
        .await?
        .to_api_result()?;

    db::delete_table(&pool, table_id).await?;

    Ok(())
}

/// Get all tables belonging to the user.
///
/// # Errors
/// - [`ApiError::Unauthorized`]: User not authenticated
///
async fn get_tables(State(ApiState { pool, .. }): State<ApiState>) -> ApiResult<Json<Vec<Table>>> {
    let user_id = db::debug_get_user_id(&pool).await?;

    let tables = db::get_tables(&pool, user_id).await?;
    Ok(Json(tables))
}

/// Get all the meta data, fields, and entries of a table.
///
/// Used for displaying the table in the user interface.
async fn get_table_data(
    State(ApiState { pool, .. }): State<ApiState>,
    Path(table_id): Path<Id>,
) -> ApiResult<Json<TableData>> {
    let user_id = db::debug_get_user_id(&pool).await?;
    db::check_table_relation(&pool, user_id, table_id)
        .await?
        .to_api_result()?;

    let data_table = db::get_table_data(&pool, table_id).await?;

    Ok(Json(data_table))
}

#[debug_handler]
async fn import_table_from_file(
    State(ApiState { pool, .. }): State<ApiState>,
    mut multipart: Multipart,
) -> ApiResult<Json<Vec<TableData>>> {
    let user_id = db::debug_get_user_id(&pool).await?;

    let Some(field) = multipart.next_field().await.unwrap() else {
        return Err(ApiError::BadRequest);
    };

    let name = field.name().unwrap_or("unknown").to_string();
    let filename = field.file_name().unwrap_or("unknown.xlsx");
    let content_type = field.content_type().unwrap_or("unknown");

    println!("name {name}");
    println!("filename {filename}");
    println!("content_type {content_type}");

    let data = field.bytes().await.unwrap();

    let spreadsheet = xlsx::read_reader(Cursor::new(data), true).map_err(anyhow::Error::from)?;

    let create_tables = io::import_table_from_excel(spreadsheet);
    info!("{create_tables:?}");

    let mut tx = pool.begin().await?;

    let mut tables = Vec::new();

    for CreateTableData {
        table,
        fields,
        entries,
    } in create_tables
    {
        let table = db::create_table(tx.as_mut(), user_id, table).await?;
        let fields = db::create_fields(tx.as_mut(), table.table_id, fields).await?;
        let entries = db::create_entries(
            tx.as_mut(),
            table.table_id,
            fields
                .iter()
                .map(|field| FieldMetadata::from_field(field.clone()))
                .collect_vec(),
            entries,
        )
        .await?;
        tables.push(TableData {
            table,
            fields,
            entries,
        })
    }

    tx.commit().await?;

    Ok(Json(tables))
}
