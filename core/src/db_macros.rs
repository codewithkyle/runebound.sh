//! Declarative CRUD generation for the per-entity DB tables (P5.5).
//!
//! `db.rs` used to restate each entity's column list 5–7× — in `SELECT`,
//! `INSERT (...)`, the `VALUES (?1, ?2, …)` placeholder run, the
//! `ON CONFLICT … DO UPDATE SET` block, and a hand-written `row_to_*` — with the
//! `?N` placeholders hand-numbered. `impl_entity_table!` takes the column list
//! ONCE and generates the whole CRUD set, so "add an entity" is one declaration.
//!
//! Design notes:
//! - Every generated query is a `&'static str` built with `concat!`; placeholders
//!   are positional `?` (bound in column order), so there is no `?N` to keep in
//!   sync by hand.
//! - The read path keeps the deliberate schema-drift tolerance the old `row_to_*`
//!   had: a `lenient` column falls back to a default and an `opt` column yields
//!   `None` when missing/NULL, while a `strict` column errors. A blanket
//!   `#[derive(sqlx::FromRow)]` would lose that (every column would become a hard
//!   error), which is why this is a macro and not a derive.
//! - Function names are passed explicitly (rather than synthesized) so every
//!   generated function — `find_npc_by_id`, `upsert_location`, … — stays greppable
//!   to its definition site.

/// Build a `LIKE` pattern matching `raw` as a literal substring, escaping the SQL
/// wildcards `\ % _` so a query containing them (e.g. `50%` or `a_b`) can't match
/// unintended rows. Pair with `ESCAPE '\'` in the query — every generated
/// `search_*_by_name` does. (P5.6)
pub(crate) fn like_contains(raw: &str) -> String {
    let escaped = raw
        .trim()
        .to_ascii_lowercase()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

/// Read one column into a typed `Row` field by access mode. Used only inside
/// [`impl_entity_table!`]; expands at the macro call site (so `try_get`/`context`
/// resolve in `db.rs`).
///
/// - `strict` — error with `"<table>.<col> missing"` if absent (corrupt row).
/// - `lenient` — fall back to `$default` (schema-drift tolerance).
/// - `opt` — yield `None` (nullable `Option<String>` column).
macro_rules! entity_col {
    (strict, $row:ident, $table:literal, $col:ident) => {
        $row.try_get(stringify!($col)).context(concat!(
            $table,
            ".",
            stringify!($col),
            " missing"
        ))?
    };
    (lenient, $row:ident, $table:literal, $col:ident, $default:expr) => {
        $row.try_get(stringify!($col)).unwrap_or_else(|_| $default)
    };
    (opt, $row:ident, $table:literal, $col:ident) => {
        $row.try_get(stringify!($col)).ok()
    };
}
pub(crate) use entity_col;

/// Generate the full CRUD function set for one entity table. See the module docs.
///
/// `columns` lists every column between `id` (always the first, strict, conflict
/// key) and the trailing `created_at` / `updated_at` (always strict), in the order
/// they are bound. Each entry is `<mode> <col> [= <default>]`:
/// `strict name`, `lenient occupation = "Unknown".to_string()`, `opt kind_custom`.
macro_rules! impl_entity_table {
    (
        table: $table:literal,
        row: $Row:ident,
        upsert: $upsert:ident,
        find_by_id: $find_by_id:ident,
        find_by_slug: $find_by_slug:ident,
        find_by_name_or_slug: $find_by_name_or_slug:ident,
        list: $list:ident,
        search_by_name: $search_by_name:ident,
        delete_by_id: $delete_by_id:ident,
        row_to: $row_to:ident,
        columns: [ $( $mode:ident $col:ident $(= $default:expr)? ),+ $(,)? ] $(,)?
    ) => {
        fn $row_to(row: sqlx::sqlite::SqliteRow) -> Result<$Row> {
            Ok($Row {
                id: row.try_get("id").context(concat!($table, ".id missing"))?,
                $(
                    $col: $crate::db_macros::entity_col!($mode, row, $table, $col $(, $default)?),
                )+
                created_at: row
                    .try_get("created_at")
                    .context(concat!($table, ".created_at missing"))?,
                updated_at: row
                    .try_get("updated_at")
                    .context(concat!($table, ".updated_at missing"))?,
            })
        }

        pub async fn $upsert(pool: &SqlitePool, row: &$Row) -> Result<()> {
            // id + the listed columns + created_at + updated_at, all bound below.
            let column_count = 1 + [$( stringify!($col), )+].len() + 2;
            let placeholders = vec!["?"; column_count].join(", ");
            // Column list + ON CONFLICT clause are static (`{}` is only the VALUES run).
            let sql = format!(
                concat!(
                    "INSERT INTO ", $table, " (id",
                    $( ", ", stringify!($col), )+
                    ", created_at, updated_at) VALUES ({}) ON CONFLICT(id) DO UPDATE SET ",
                    $( stringify!($col), " = excluded.", stringify!($col), ", ", )+
                    "updated_at = excluded.updated_at"
                ),
                placeholders
            );
            sqlx::query(&sql)
                .bind(&row.id)
                $( .bind(&row.$col) )+
                .bind(&row.created_at)
                .bind(&row.updated_at)
                .execute(pool)
                .await
                .context(concat!("failed to upsert ", $table))?;
            Ok(())
        }

        pub async fn $find_by_id(pool: &SqlitePool, id: &str) -> Result<Option<$Row>> {
            let row = sqlx::query(concat!(
                "SELECT id", $( ", ", stringify!($col), )+
                ", created_at, updated_at FROM ", $table, " WHERE id = ?1"
            ))
            .bind(id)
            .fetch_optional(pool)
            .await
            .context(concat!("failed to query ", $table, " by id"))?;
            row.map($row_to).transpose()
        }

        pub async fn $find_by_slug(pool: &SqlitePool, slug: &str) -> Result<Option<$Row>> {
            let row = sqlx::query(concat!(
                "SELECT id", $( ", ", stringify!($col), )+
                ", created_at, updated_at FROM ", $table, " WHERE slug = ?1"
            ))
            .bind(slug)
            .fetch_optional(pool)
            .await
            .context(concat!("failed to query ", $table, " by slug"))?;
            row.map($row_to).transpose()
        }

        pub async fn $find_by_name_or_slug(
            pool: &SqlitePool,
            input: &str,
        ) -> Result<Option<$Row>> {
            let normalized = input.trim().to_ascii_lowercase();
            let row = sqlx::query(concat!(
                "SELECT id", $( ", ", stringify!($col), )+
                ", created_at, updated_at FROM ", $table,
                " WHERE lower(name) = ?1 OR lower(slug) = ?2",
                " ORDER BY CASE WHEN lower(name) = ?1 THEN 0 ELSE 1 END, id ASC",
                " LIMIT 1"
            ))
            .bind(&normalized)
            .bind(&normalized)
            .fetch_optional(pool)
            .await
            .context(concat!("failed to find ", $table, " by name or slug"))?;
            row.map($row_to).transpose()
        }

        pub async fn $list(pool: &SqlitePool) -> Result<Vec<$Row>> {
            let rows = sqlx::query(concat!(
                "SELECT id", $( ", ", stringify!($col), )+
                ", created_at, updated_at FROM ", $table,
                " ORDER BY name COLLATE NOCASE ASC, id ASC"
            ))
            .fetch_all(pool)
            .await
            .context(concat!("failed to list ", $table))?;
            rows.into_iter().map($row_to).collect()
        }

        pub async fn $search_by_name(
            pool: &SqlitePool,
            query: &str,
            limit: i64,
        ) -> Result<Vec<$Row>> {
            let pattern = $crate::db_macros::like_contains(query);
            let rows = sqlx::query(concat!(
                "SELECT id", $( ", ", stringify!($col), )+
                ", created_at, updated_at FROM ", $table,
                " WHERE lower(name) LIKE ?1 ESCAPE '\\'",
                " ORDER BY name COLLATE NOCASE ASC, id ASC",
                " LIMIT ?2"
            ))
            .bind(pattern)
            .bind(limit)
            .fetch_all(pool)
            .await
            .context(concat!("failed to search ", $table, " by name"))?;
            rows.into_iter().map($row_to).collect()
        }

        pub async fn $delete_by_id(pool: &SqlitePool, id: &str) -> Result<()> {
            sqlx::query(concat!("DELETE FROM ", $table, " WHERE id = ?1"))
                .bind(id)
                .execute(pool)
                .await
                .context(concat!("failed to delete ", $table, " row"))?;
            Ok(())
        }
    };
}
pub(crate) use impl_entity_table;
