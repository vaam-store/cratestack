//! Column-shape IR nodes: nullability, type, default, plus the
//! `destructiveness_on_add` rule shared by `AddColumn` / `CreateTable`
//! flows.

use serde::{Deserialize, Serialize};

use super::Destructiveness;

/// Column data shared by `CreateTable` and `AddColumn`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub ty: ColumnType,
    pub arity: ColumnArity,
    pub default: Option<ColumnDefault>,
    pub primary_key: bool,
}

/// Column nullability and shape.
///
/// `List` corresponds to a `.cstack` list field (`String[]`). The
/// Postgres emitter renders it as a SQL array; the SQLite emitter
/// rejects it at emit time (SQLite has no array type and the right
/// answer is a relation table or a JSON column, both of which require
/// schema-level decisions the diff engine cannot make).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnArity {
    Required,
    Optional,
    List,
}

/// Column type. The diff engine keeps the `.cstack` scalar name as a
/// string and defers dialect mapping to the emitter — this way new
/// scalars do not require touching the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    /// `.cstack` built-in scalar (`String`, `Int`, `Uuid`, …).
    Scalar(String),
    /// User-defined enum declared via `enum Name { … }`.
    Enum(String),
    /// User-defined composite type declared via `type Name { … }`.
    ///
    /// As of #230, `cratestack-parser` rejects a model field whose type
    /// resolves to a `type` declaration (`reject_type_decl_as_model_field_type`
    /// in `cratestack-parser/src/validate/type_names.rs`), so this variant is
    /// unreachable for any `Schema` produced by the parser: no `CREATE TYPE`
    /// op exists for it (only enums get one — see `emit::postgres::enums`),
    /// so a column typed this way could never round-trip through real DDL.
    ///
    /// That rejection now exempts `@computed` fields, which may be typed
    /// with a `type` block (`docs/design/computed-fields.md` §"Schema
    /// surface"). This variant stays unreachable anyway: a computed field
    /// is resolved at response time and never becomes a column at all —
    /// `super::super::convert`'s `is_computed_field` guard drops it before
    /// `field_to_column` runs, so it never reaches this enum.
    ///
    /// It is intentionally kept rather than deleted: `diff()` and
    /// `project_model()` in this crate take a plain `&Schema` and don't
    /// re-run parser validation, and `Schema` is also deserialized directly
    /// from an on-disk `snapshot.json` (`read_snapshot`) — so a
    /// hand-constructed or hand-edited `Schema` could still reach this path.
    /// Keeping the variant (and `emit::postgres::columns::render_type`'s
    /// branch for it) means that hypothetical caller still gets a
    /// deterministic composite-type-name rendering rather than a panic or a
    /// silently wrong scalar fallback, even though it remains just as
    /// unbacked by a real `CREATE TYPE` as before this fix. Composite-type
    /// support (a real `CreateType`/`DropType` op, `SqlValue` encode/decode)
    /// is tracked as a separate, larger effort — see #230's option (b).
    UserDefined(String),
    /// `Vector(n)` — a fixed-dimension float vector (see
    /// `docs/design/extensions.md` §6). A dedicated variant rather
    /// than folding `n` into `Scalar`'s string, since the Postgres
    /// emitter needs the dimension to render `vector(n)` and the
    /// SQLite emitter needs no dialect-specific info at all (every
    /// column there is `BLOB` regardless of scalar).
    Vector(u32),
    /// `Geography` / `Geometry` — a PostGIS spatial column (see
    /// `docs/design/extensions.md` §6b and cratestack#842). A dedicated
    /// variant for the same reason as [`ColumnType::Vector`]: the
    /// Postgres emitter needs the subtype and SRID to render the type
    /// modifier, and folding them into `Scalar`'s string would make the
    /// snapshot's column type unparseable without re-deriving the
    /// grammar here.
    Spatial {
        /// `true` for `Geography` (spheroidal), `false` for `Geometry`
        /// (planar). The two are distinct Postgres types, not a
        /// modifier on one type, so a change between them is a real
        /// column-type change the diff must see.
        geography: bool,
        /// The canonicalised geometry subtype — `Polygon` in
        /// `Geography(Polygon, 4326)`. `None` for the unmodified form,
        /// which PostGIS accepts as "any subtype".
        subtype: Option<String>,
        /// The SRID. `None` when the schema didn't write one, deferring
        /// to PostGIS's own default rather than inventing one here.
        srid: Option<u32>,
    },
}

/// Column default value, captured as the developer wrote it. The
/// emitter is responsible for any dialect-specific quoting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnDefault {
    /// Literal (e.g. `0`, `'pending'`, `true`).
    Literal(String),
    /// Database function (e.g. `now()`, `gen_random_uuid()`).
    Function(String),
    /// `@default(dbgenerated())` — a marker, not a value. It asserts
    /// that the column already has (or will separately be given) a
    /// real Postgres-level default set some other way: hand-authored
    /// migration SQL, a trigger, `GENERATED ... AS IDENTITY`, etc.
    /// cratestack has no way to verify that claim from the `.cstack`
    /// schema alone, so emitters must never invent a `DEFAULT` clause
    /// for it — see `Op::destructiveness` and
    /// `crate::ir::unverified_dbgenerated_columns` for the
    /// corresponding safety checks.
    DbGenerated,
}

impl Column {
    /// Whether adding this column to an existing table is safe,
    /// blocking, or otherwise.
    ///
    /// * Optional columns are always safe — every existing row gets
    ///   `NULL` for the new column.
    /// * Required columns with a default are safe — Postgres and
    ///   SQLite both backfill the default into every existing row.
    /// * Required columns without a default are **blocking** — the
    ///   migration cannot succeed on a non-empty table; the user must
    ///   either set a default in the schema or split the change in two
    ///   (add optional, backfill, promote) — note a pre-script cannot
    ///   help here, since the column does not exist when it runs. See
    ///   [`crate::ir::blocking_reasons`].
    pub(crate) fn destructiveness_on_add(&self) -> Destructiveness {
        match self.arity {
            ColumnArity::Optional | ColumnArity::List => Destructiveness::Safe,
            ColumnArity::Required => {
                // `DbGenerated` is a marker, not a real DDL default —
                // it backfills nothing, so it must not count as "has
                // a default" here any more than no default at all.
                let has_real_default = matches!(
                    self.default,
                    Some(ColumnDefault::Literal(_)) | Some(ColumnDefault::Function(_))
                );
                if has_real_default || self.primary_key {
                    Destructiveness::Safe
                } else {
                    Destructiveness::Blocking
                }
            }
        }
    }
}
