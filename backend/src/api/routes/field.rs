use std::fmt::format;

use super::ApiState;
use crate::api::{error::ApiResult, Id};
use axum::{
    extract::{Path, State},
    routing::post,
    Json, Router,
};
use axum_macros::debug_handler;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::types::PgInterval, prelude::FromRow, types::Decimal, Postgres};
use strum_macros::Display;

pub(crate) fn router() -> Router<ApiState> {
    Router::new().nest(
        "/tables/{table_id}/fields",
        Router::new().route("/", post(create_field)),
    )
}

#[derive(Serialize)]
struct FieldId {
    field_id: Id,
}

#[derive(Serialize, Deserialize, Display)]
#[serde(rename_all = "snake_case")]
enum FieldOptions {
    Text {
        is_required: bool,
    },
    Integer {
        is_required: bool,
        range_start: Option<i64>,
        ranger_end: Option<i64>,
    },
    Decimal {
        is_required: bool,
        range_start: Option<f64>,
        ranger_end: Option<f64>,
        scientific_notation: bool,
        number_precision: Option<i32>,
        number_scale: Option<i32>,
    },
    Money {
        is_required: bool,
        range_start: Option<Decimal>,
        range_end: Option<Decimal>,
    },
    Progress {
        total_steps: i32,
    },
    DateTime {
        is_required: bool,
        range_start: Option<DateTime<Utc>>,
        range_end: Option<DateTime<Utc>>,
        date_time_format: String,
    },
    Interval {
        is_required: bool,
        // range_start: Option<PgInterval>,
        // range_end: Option<PgInterval>,
    },
    WebLink {
        is_required: bool,
    },

    Email {
        is_required: bool,
    },
    Checkbox {
        default_value: bool,
    },
    Enumeration {
        is_required: bool,
        values: Vec<String>,
        default_value: i32,
    },
    CreationDate {
        is_required: bool,
        date_time_format: String,
    },
    ModificationDate {
        is_required: bool,
        date_time_format: String,
    },
    Image {
        is_required: bool,
    },
    File {
        is_required: bool,
    },
}

#[derive(FromRow)]
struct InsertField {
    field_id: Id,
    data_field_name: String,
}

#[derive(FromRow)]
struct SelectDataTableName {
    data_table_name: String,
}

#[debug_handler]
async fn create_field(
    Path(table_id): Path<Id>,
    State(ApiState { pool, .. }): State<ApiState>,
    Json(field_options): Json<FieldOptions>,
) -> ApiResult<Json<FieldId>> {
    let SelectDataTableName { data_table_name } = sqlx::query_as(
        r#"
            SELECT data_table_name
            FROM table_metadata
            WHERE table_id = $1
        "#,
    )
    .bind(table_id)
    .fetch_one(&pool)
    .await?;

    let mut transaction = pool.begin().await?;

    let InsertField {
        field_id,
        data_field_name,
    } = sqlx::query_as(
        r#"
            INSERT INTO table_field (table_id, field_options)
            VALUES ($1, $2)
            RETURNING field_id, data_field_name
        "#,
    )
    .bind(table_id)
    .bind(sqlx::types::Json(field_options))
    .fetch_one(&mut *transaction)
    .await?;

    // data_table_name and data_field_name generated by database NO INJECTION POSSIBLE

    let field_query = format!(
        r#"
            ALTER TABLE {data_table_name}
            ADD COLUMN {data_field_name}
        "#,
    );

    match field_options {
        FieldOptions::Text { is_required } => {
            field_query.push_str(" TEXT");
            if is_required {
                field_query.push_str(" NOT NULL");
            }
        }
        FieldOptions::Integer {
            is_required,
            range_start,
            ranger_end,
        } => {
            field_query.push_str(" BIGINT");
            if is_required {
                field_query.push_str(" NOT NULL");
            }
        }
        FieldOptions::Decimal {
            is_required,
            range_start,
            ranger_end,
            scientific_notation,
            number_precision,
            number_scale,
        } => {
            field_query.push_str(" DOUBLE");
            if is_required {
                field_query.push_str(" NOT NULL");
            }
        }
        FieldOptions::Money {
            is_required,
            range_start,
            range_end,
        } => {
            field_query.push_str(" numeric_money");
            if is_required {
                field_query.push_str(" NOT NULL");
            }
        }
        FieldOptions::Progress { total_steps } => {
            field_query.push_str(" INT NOT NULL");
        }
        FieldOptions::DateTime {
            is_required,
            range_start,
            range_end,
            date_time_format,
        } => {
            field_query.push_str(" TIMESTAMPTZ");
            if is_required {
                field_query.push_str(" NOT NULL");
            }
        }
        FieldOptions::Interval { is_required } => {
            field_query.push_str(" INTERVAL");
            if is_required {
                field_query.push_str(" NOT NULL");
            }
        }
        FieldOptions::WebLink { is_required } => {
            field_query.push_str(" TE");
            if is_required {
                field_query.push_str(" NOT NULL");
            }
        }
        FieldOptions::Email { is_required } => todo!(),
        FieldOptions::Checkbox { default_value } => todo!(),
        FieldOptions::Enumeration {
            is_required,
            values,
            default_value,
        } => todo!(),
        FieldOptions::CreationDate {
            is_required,
            date_time_format,
        } => todo!(),
        FieldOptions::ModificationDate {
            is_required,
            date_time_format,
        } => todo!(),
        FieldOptions::Image { is_required } => todo!(),
        FieldOptions::File { is_required } => todo!(),
    }

    Ok(Json(FieldId { field_id }))
}
