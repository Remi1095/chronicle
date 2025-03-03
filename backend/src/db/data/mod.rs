mod entries;
mod fields;
mod tables;

use std::iter;

use crate::{
    error::{ApiError, ApiResult},
    model::data::{TableData, Entry, Field, FieldKind, FullTable},
    Id,
};
use itertools::Itertools;
use sqlx::{types::Json, Acquire, Postgres};
pub use {entries::*, fields::*, tables::*};

pub enum Relation {
    Owned,
    NotOwned,
    Absent,
}

impl Relation {
    pub fn to_api_result(self) -> ApiResult<()> {
        match self {
            Relation::Owned => Ok(()),
            Relation::NotOwned => Err(ApiError::Forbidden),
            Relation::Absent => Err(ApiError::NotFound),
        }
    }
}

pub async fn get_table_data(
    connection: impl Acquire<'_, Database = Postgres>,
    table_id: Id,
) -> sqlx::Result<TableData> {
    let mut tx = connection.begin().await?;

    let FullTable {
        table,
        data_table_name,
    } = sqlx::query_as(
        r#"
            SELECT 
                table_id,
                user_id,
                name,
                description,
                created_at,
                updated_at,
                data_table_name
            FROM meta_table
            WHERE table_id = $1
        "#,
    )
    .bind(table_id)
    .fetch_one(tx.as_mut())
    .await?;

    let fields: Vec<Field> = sqlx::query_as(
        r#"
            SELECT
                field_id,
                table_id,
                name,
                field_kind,
                created_at,
                updated_at
            FROM meta_field
            WHERE table_id = $1
            ORDER BY field_id
        "#,
    )
    .bind(table_id)
    .fetch_all(tx.as_mut())
    .await?;

    let field_data: Vec<(Id, String, Json<FieldKind>)> = sqlx::query_as(
        r#"
            SELECT field_id, data_field_name, field_kind
            FROM meta_field
            WHERE table_id = $1
        "#,
    )
    .bind(table_id)
    .fetch_all(tx.as_mut())
    .await?;

    let select_columns = field_data
        .iter()
        .map(|(_, name, _)| name.as_str())
        .chain(["entry_id", "created_at", "updated_at"])
        .join(", ");

    let entries = sqlx::query::<Postgres>(&format!(
        r#"
            SELECT {select_columns}
            FROM {data_table_name}
        "#
    ))
    .fetch_all(tx.as_mut())
    .await?
    .into_iter()
    .map(|row| Entry::from_row(row, &field_data).unwrap())
    .collect_vec();

    Ok(TableData {
        table,
        fields,
        entries,
    })
}
