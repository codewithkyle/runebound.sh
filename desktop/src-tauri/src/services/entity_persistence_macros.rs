//! Declarative save generation for the per-entity persistence layer (P5.2b).
//!
//! `entity_persistence.rs` used to carry seven near-identical `save_*_draft`
//! methods (~90 lines each): the same skeleton — resolve the canonical slug,
//! resolve the readable vault path, carry `created_at`/`published_at` forward,
//! write the TOML store, drop the old slug on a rename, upsert the DB row, and
//! upsert the document index — wrapped around a kind-specific normalize step plus
//! a `*Frontmatter` and a `*Row` literal. The repos, store methods, and
//! `*Frontmatter`/`*Row` structs are seven distinct types with no shared trait, so
//! (like the `impl_entity_table!` db CRUD, P5.5) this is a macro: the skeleton is
//! written once and each entity declares only its irreducible parts.
//!
//! Design notes:
//! - `binder:` names the draft parameter as an *invocation-site* identifier so the
//!   caller's `normalize` block and the `frontmatter_fields`/`row_fields` value
//!   expressions (which reference both `draft` and the locals `normalize` binds)
//!   resolve under macro hygiene. Everything the skeleton owns (`store`, `slug`,
//!   `vault_path`, `now`, …) stays in the macro's own context.
//! - `frontmatter_fields`/`row_fields` list only the kind-specific columns; the
//!   shared columns (`doc_type`/`id`/`slug`/`name`/`vault_path`/`created_at`/
//!   `updated_at`/`published_at`) are injected by the skeleton. A field is either
//!   shorthand (`occupation`) or `name: expr` (`carrying: carrying_db`), so the
//!   frontmatter (Vec form) and row (db-text form) can diverge where they must.
//!   A forgotten field is a struct-literal completeness compile error.

/// Generate one `save_<kind>_draft(&Draft, &AppState) -> Result<SaveOutcome>`
/// free function. See the module docs for the field-list contract.
macro_rules! impl_entity_persistence {
    (
        save_fn: $save_fn:ident,
        draft_ty: $Draft:ty,
        binder: $draft:ident,
        frontmatter: $Frontmatter:ident,
        row: $Row:ident,
        dir: $dir:literal,
        $( vault_dir: $vault_dir:expr, )?
        kind: $kind:literal,
        repo: $repo:ident,
        store_save: $store_save:ident,
        store_load: $store_load:ident,
        store_delete: $store_delete:ident,
        normalize: { $($normalize:tt)* },
        frontmatter_fields: { $( $ff:ident $(: $fv:expr)? ),* $(,)? },
        row_fields: { $( $rf:ident $(: $rv:expr)? ),* $(,)? } $(,)?
    ) => {
        async fn $save_fn($draft: &$Draft, state: &AppState) -> Result<SaveOutcome, String> {
            if $draft.id.trim().is_empty() {
                return Err(concat!($kind, " id cannot be empty").to_string());
            }
            let name = $draft.name.trim();
            if name.is_empty() {
                return Err(concat!($kind, " name cannot be empty").to_string());
            }

            // Kind-specific field normalization + extra validation. Binds the
            // locals the frontmatter/row field lists below reference.
            $($normalize)*

            let store = EntityStore::new().map_err(|err| err.to_string())?;
            let database = state.database();
            let repo = state.$repo();
            let document_repo = state.document_repo();
            let now = now_timestamp();

            let existing = repo.find_by_id(database.as_ref(), $draft.id.trim()).await?;
            let slug = resolve_slug(
                store.root(),
                $dir,
                existing.as_ref().map(|row| row.slug.as_str()),
                name,
            );
            let created_at = existing
                .as_ref()
                .map(|row| row.created_at.clone())
                .unwrap_or_else(|| now.clone());
            // The readable `.md` dir defaults to `$dir` (flat); an optional `vault_dir:`
            // expr (Location only) may reshape it per kind. The slug + TOML store stay
            // on `$dir`. The expr can read `draft`/`kind_type` (hygiene matches the
            // `frontmatter_fields` value exprs). It only matters for a brand-new row —
            // `resolve_vault_path` preserves an existing row's on-disk folder.
            let vault_dir = {
                // `unused_mut`/`unused_assignments` fire for the six entities that omit
                // the optional `vault_dir:` arg (no reassignment) and for those that
                // supply it (the `$dir` default is overwritten before being read).
                #[allow(unused_mut, unused_assignments)]
                let mut d = $dir.to_string();
                $( d = $vault_dir; )?
                d
            };
            let (vault_path, retire_index) = resolve_vault_path(
                document_repo.as_ref(),
                database.as_ref(),
                &vault_dir,
                name,
                existing.as_ref().map(|row| ExistingRef {
                    slug: &row.slug,
                    vault_path: &row.vault_path,
                }),
            )
            .await?;
            let published_at = match existing.as_ref() {
                Some(current) => store
                    .$store_load(&current.slug)
                    .ok()
                    .flatten()
                    .and_then(|prior| prior.published_at),
                None => None,
            };

            let frontmatter = $Frontmatter {
                doc_type: $kind.to_string(),
                id: $draft.id.trim().to_string(),
                slug: slug.clone(),
                name: name.to_string(),
                vault_path: vault_path.clone(),
                $( $ff $(: $fv)? , )*
                created_at: created_at.clone(),
                updated_at: now.clone(),
                published_at,
            };
            // FS-first: the canonical TOML store is the source of truth, so write the
            // new record (and drop the old slug on a rename) before touching the DB.
            // A failure after this leaves the canonical file intact for `sync` to
            // re-project (P6.1).
            store
                .$store_save(&frontmatter)
                .map_err(|err| err.to_string())?;
            if let Some(current) = existing.as_ref()
                && current.slug != slug
            {
                store
                    .$store_delete(&current.slug)
                    .map_err(|err| err.to_string())?;
            }

            let row = $Row {
                id: $draft.id.trim().to_string(),
                slug: slug.clone(),
                name: name.to_string(),
                vault_path: vault_path.clone(),
                $( $rf $(: $rv)? , )*
                created_at: created_at.clone(),
                updated_at: now.clone(),
            };
            // Atomic DB projection: retire the stale index entry (on a readable-name
            // rename), upsert the row, and upsert the index as one transaction so the
            // index can't end up out of step with its row (P6.1).
            let mut tx = database.begin().await.map_err(|err| err.to_string())?;
            if let Some(old_vault_path) = retire_index.as_deref() {
                document_repo
                    .delete_by_vault_path_tx(&mut tx, old_vault_path)
                    .await?;
            }
            repo.upsert_tx(&mut tx, &row).await?;
            document_repo
                .upsert_index_tx(
                    &mut tx,
                    $kind,
                    &row.slug,
                    Some(&row.name),
                    &row.vault_path,
                    &row.created_at,
                    &row.updated_at,
                )
                .await?;
            tx.commit().await.map_err(|err| err.to_string())?;

            Ok(SaveOutcome {
                id: row.id,
                slug: row.slug,
                vault_path: row.vault_path,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
        }
    };
}
pub(crate) use impl_entity_persistence;
