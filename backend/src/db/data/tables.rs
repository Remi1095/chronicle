use super::{entry_from_row, field_columns};
use crate::{
    db::Relation,
    model::data::{
        CreateTable, Field, FieldIdentifier, FieldMetadata, Table, TableData, TableIdentifier,
        UpdateTable,
    },
    Id,
};
use futures::future::join_all;
use itertools::Itertools;
use sqlx::{Acquire, PgExecutor, Postgres};

pub async fn create_table(
    conn: impl Acquire<'_, Database = Postgres>,
    user_id: Id,
    CreateTable {
        parent_id,
        name,
        description,
    }: CreateTable,
) -> sqlx::Result<Table> {
    let mut tx = conn.begin().await?;

    let table: Table = sqlx::query_as(
        r#"
            INSERT INTO meta_table (user_id, parent_id, name, description)
            VALUES ($1, $2, $3, $4) 
            RETURNING
                table_id,
                user_id,
                parent_id,
                name,
                description,
                created_at,
                updated_at
        "#,
    )
    .bind(user_id)
    .bind(parent_id)
    .bind(name)
    .bind(description)
    .fetch_one(tx.as_mut())
    .await?;

    let table_ident = TableIdentifier::new(table.table_id, "data_table");
    let parent_id_column = if let Some(parent_id) = table.parent_id {
        let parent_ident = TableIdentifier::new(parent_id, "data_table");
        format!("parent_id INT NOT NULL REFERENCES {parent_ident}(entry_id),")
    } else {
        String::new()
    };

    sqlx::query(&format!(
        r#"
            CREATE TABLE {table_ident} (
                entry_id SERIAL PRIMARY KEY,
                {parent_id_column}
                created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at TIMESTAMPTZ
            )
        "#,
    ))
    .execute(tx.as_mut())
    .await?;

    sqlx::query(&format!(r#"SELECT trigger_updated_at('{table_ident}')"#))
        .execute(tx.as_mut())
        .await?;

    tx.commit().await?;

    Ok(table)
}

pub async fn update_table(
    conn: impl Acquire<'_, Database = Postgres>,
    table_id: Id,
    UpdateTable { name, description }: UpdateTable,
) -> sqlx::Result<Table> {
    let mut tx = conn.begin().await?;

    let table = sqlx::query_as(
        r#"
            UPDATE meta_table
            SET name = $1, description = $2
            WHERE table_id = $3
            RETURNING
                table_id,
                user_id,
                parent_id,
                name,
                description,
                created_at,
                updated_at
        "#,
    )
    .bind(name)
    .bind(description)
    .bind(table_id)
    .fetch_one(tx.as_mut())
    .await?;

    tx.commit().await?;

    Ok(table)
}

pub async fn delete_table(
    conn: impl Acquire<'_, Database = Postgres>,
    table_id: Id,
) -> sqlx::Result<()> {
    let mut tx = conn.begin().await?;

    sqlx::query(
        r#"
            DELETE FROM meta_table
            WHERE table_id = $1
        "#,
    )
    .bind(table_id)
    .execute(tx.as_mut())
    .await?;

    let table_ident = TableIdentifier::new(table_id, "data_table");

    sqlx::query(&format!(r#"DROP TABLE {table_ident} CASCADE"#))
        .execute(tx.as_mut())
        .await?;

    tx.commit().await?;

    Ok(())
}

pub async fn get_table_parent_id(executor: impl PgExecutor<'_>, table_id: Id) -> sqlx::Result<Id> {
    sqlx::query_scalar(
        r#"
            SELECT parent_id
            FROM meta_table
            WHERE table_id = $1
        "#,
    )
    .bind(table_id)
    .fetch_one(executor)
    .await
}

pub async fn get_tables(executor: impl PgExecutor<'_>, user_id: Id) -> sqlx::Result<Vec<Table>> {
    sqlx::query_as(
        r#"
            SELECT
                table_id,
                user_id,
                parent_id,
                name,
                description,
                created_at,
                updated_at
            FROM meta_table
            WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .fetch_all(executor)
    .await
}

pub async fn get_table_children(
    executor: impl PgExecutor<'_>,
    table_id: Id,
) -> sqlx::Result<Vec<Table>> {
    sqlx::query_as(
        r#"
            SELECT
                table_id,
                user_id,
                parent_id,
                name,
                description,
                created_at,
                updated_at
            FROM meta_table
            WHERE parent_id = $1
        "#,
    )
    .bind(table_id)
    .fetch_all(executor)
    .await
}

pub async fn get_table_data(
    executor: impl PgExecutor<'_> + Copy,
    table_id: Id,
) -> sqlx::Result<TableData> {
    let table: Table = sqlx::query_as(
        r#"
            SELECT 
                table_id,
                user_id,
                parent_id,
                name,
                description,
                created_at,
                updated_at
            FROM meta_table
            WHERE table_id = $1
        "#,
    )
    .bind(table_id)
    .fetch_one(executor)
    .await?;

    let fields: Vec<Field> = sqlx::query_as(
        r#"
            SELECT
                field_id,
                table_id,
                name,
                ordering,
                field_kind,
                created_at,
                updated_at
            FROM meta_field
            WHERE table_id = $1
            ORDER BY field_id
        "#,
    )
    .bind(table_id)
    .fetch_all(executor)
    .await?;

    let field_idents = fields
        .iter()
        .map(|field| FieldIdentifier::new(field.field_id))
        .collect_vec();

    let select_columns = field_columns(table.parent_id.is_some(), &field_idents).join(", ");

    let table_ident = TableIdentifier::new(table_id, "data_table");
    let entries = sqlx::query::<Postgres>(&format!(
        r#"
            SELECT {select_columns}
            FROM {table_ident}
        "#
    ))
    .fetch_all(executor)
    .await?
    .into_iter()
    .map(|row| {
        entry_from_row(
            row,
            &fields
                .iter()
                .map(
                    |Field {
                         field_id,
                         field_kind,
                         ..
                     }| FieldMetadata {
                        field_id: field_id.clone(),
                        field_kind: field_kind.clone(),
                    },
                )
                .collect_vec(),
        )
    })
    .try_collect()?;

    let children_ids = sqlx::query_scalar(
        r#"
            SELECT table_id
            FROM meta_table
            WHERE parent_id = $1
         "#,
    )
    .bind(table_id)
    .fetch_all(executor)
    .await?;

    let children = join_all(
        children_ids
            .into_iter()
            .map(|child_id| get_table_data(executor, child_id)),
    )
    .await
    .into_iter()
    .try_collect()?;

    Ok(TableData {
        table,
        fields,
        entries,
        children,
    })
}

pub async fn check_table_relation(
    executor: impl PgExecutor<'_>,
    user_id: Id,
    table_id: Id,
) -> sqlx::Result<Relation> {
    sqlx::query_scalar::<_, Id>(
        r#"
            SELECT user_id
            FROM meta_table
            WHERE table_id = $1
        "#,
    )
    .bind(table_id)
    .fetch_optional(executor)
    .await
    .map(|id| match id {
        None => Relation::Absent,
        Some(id) if id == user_id => Relation::Owned,
        Some(_) => Relation::NotOwned,
    })
}
