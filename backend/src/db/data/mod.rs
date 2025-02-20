mod entries;
mod fields;
mod tables;

use crate::{
    model::data::{Cell, DataTable, Entry, Field, FieldOptions, Table},
    Id,
};
use itertools::Itertools;
use sqlx::{postgres::PgRow, Acquire, FromRow, Postgres, Row};
pub use {entries::*, fields::*, tables::*};

// All SELECT statements lock selected rows during the transaction.
// A regular connection will lock only for the duration of the function.

pub enum Relation {
    Owned,
    NotOwned,
    Absent,
}

pub async fn get_data_table(
    connection: impl Acquire<'_, Database = Postgres>,
    table_id: Id,
) -> sqlx::Result<DataTable> {
    let mut tx = connection.begin().await?;

    let row = sqlx::query(
        r#"
            SELECT 
                data_table_name
                table_id,
                user_id,
                name,
                description,
                created_at,
                updated_at
            FROM meta_table
            WHERE table_id = $1
        "#,
    )
    .bind(table_id)
    .fetch_one(tx.as_mut())
    .await?;

    let data_table_name: String = row.get("data_table_name");
    let table = Table::from_row(&row)?;

    // let table: Table = sqlx::query_as(
    //     r#"
    //         SELECT
    //             table_id,
    //             user_id,
    //             name,
    //             description,
    //             created_at,
    //             updated_at
    //         FROM meta_table
    //         WHERE table_id = $1
    //     "#,
    // )
    // .fetch_one(tx.as_mut())
    // .await?;

    let fields: Vec<Field> = sqlx::query_as(
        r#"
            SELECT
                field_id,
                table_id,
                name,
                options,
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

    let data_field_names: Vec<String> = sqlx::query_scalar(
        r#"
            SELECT data_field_name
            FROM meta_field
            WHERE table_id = $1
            ORDER BY field_id
        "#,
    )
    .bind(table_id)
    .fetch_all(tx.as_mut())
    .await?;

    let data_field_parameters = data_field_names.iter().join(", ");

    let entries = sqlx::query::<Postgres>(&format!(
        r#"
            SELECT {data_field_parameters}, entry_id
            FROM {data_table_name}
        "#
    ))
    .fetch_all(tx.as_mut())
    .await?
    .into_iter()
    .map(|row| {
        Ok(Entry {
            entry_id: row.try_get("entry_id")?,
            cells: data_field_names
                .iter()
                .zip(fields.iter())
                .map(|(identifier, field)| cell_from_row(&row, &identifier, &field.options.0))
                .collect::<sqlx::Result<_>>()?,
        })
    })
    .collect::<sqlx::Result<Vec<_>>>()?;

    Ok(DataTable {
        table,
        fields,
        entries,
    })
}

fn cell_from_row(row: &PgRow, index: &str, field_options: &FieldOptions) -> sqlx::Result<Cell> {
    Ok(match field_options {
        FieldOptions::Text { .. } | FieldOptions::WebLink { .. } | FieldOptions::Email { .. } => {
            Cell::String(row.try_get(index)?)
        }
        FieldOptions::Integer { .. }
        | FieldOptions::Progress { .. }
        | FieldOptions::Enumeration { .. } => Cell::Integer(row.try_get(index)?),
        FieldOptions::Decimal { .. } => Cell::Float(row.try_get(index)?),
        FieldOptions::Money { .. } => Cell::Decimal(row.try_get(index)?),
        FieldOptions::DateTime { .. } => Cell::DateTime(row.try_get(index)?),
        FieldOptions::Interval { .. } => Cell::Interval(row.try_get(index)?),
        FieldOptions::Checkbox => Cell::Boolean(row.try_get(index)?),
        FieldOptions::Image { .. } => Cell::Image(row.try_get(index)?),
        FieldOptions::File { .. } => Cell::File(row.try_get(index)?),
    })
}
