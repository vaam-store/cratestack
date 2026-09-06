//! `ProcedureView`/`build_procedure` — split out of `views.rs` (which
//! covers models/enums/interfaces) to keep both files closer to this
//! repo's ~200-LoC convention, mirroring `find_many_views.rs`'s existing
//! split for `Where`/`OrderBy`/`FindMany`.

use std::collections::BTreeSet;

use cratestack_core::{Procedure, ProcedureKind};
use serde::Serialize;

use crate::naming::{procedure_wrapper_name, to_camel_case, to_pascal_case};
use crate::rtk::touch::touched_model_names;
use crate::types::ts_type;
use crate::wire_shapes::{ProcedureRevival, procedure_revival};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProcedureView {
    pub(crate) name: String,
    pub(crate) method_name: String,
    pub(crate) hook_name: String,
    pub(crate) args_name: String,
    pub(crate) return_type: String,
    pub(crate) route: String,
    pub(crate) kind: &'static str,
    pub(crate) query_key: String,
    pub(crate) mutation_key: String,
    /// `"scalar"` when the return type is a bare revivable scalar —
    /// `Decimal` or `Bytes`, at any arity. The decoded value is a raw
    /// string / integer array (or an array of them, or null), not an
    /// object, so the generated call site uses `reviveWireScalar` instead
    /// of `reviveWireFields`. `"shape"` for every other return type
    /// (cratestack#498 F2): a `Model`/`type` (optionally
    /// `Page<...>`/list/optional-wrapped), an enum, or a plain scalar
    /// needing no revival — `revival_shape_name` names the registry entry
    /// for that case (a name with no entry, e.g. `echoName(): String`, is
    /// `reviveWireFields`'s documented no-op fast path).
    pub(crate) revival_kind: &'static str,
    /// Only meaningful when `revival_kind == "scalar"` — which scalar
    /// revival the generated `reviveWireScalar(value, "...")` call asks
    /// for (`"decimal"`, `"bytes"`, or `"bytesList"`). Empty string for
    /// `"shape"`. See `crate::wire_shapes::ScalarRevival`, whose string
    /// forms are a contract with `models.ts.j2`'s runtime.
    pub(crate) revival_scalar_kind: &'static str,
    /// Only meaningful when `revival_kind == "shape"` — the
    /// (`Page<T>`-unwrapped) base return type's name, a registry key into
    /// `models.ts.j2`'s generated `wireShapes` object. Empty string for
    /// `"scalar"`.
    pub(crate) revival_shape_name: String,
    /// Only meaningful when `revival_kind == "shape"` — `true`
    /// when the return type was `Page<T>` (see `views::ModelApiView::
    /// is_paged`'s doc comment for why that needs a different runtime
    /// helper). `false` for `"scalar"`.
    pub(crate) revival_paged: bool,
    /// Issue #906: model names this procedure's own `args`/`return_type`
    /// reference — `crate::rtk::touch::touched_model_names`, sorted for a
    /// stable render order. `--rtk`'s ONLY consumer today: a query
    /// procedure's `providesTags` (specific, `{ type, id: 'LIST' }`) and a
    /// mutation procedure's `invalidatesTags` (GENERAL, `{ type }` with no
    /// `id`, so it reaches every `get<Model>(id)` entry — see
    /// `crate::rtk`'s module doc). Empty for
    /// every procedure when `--rtk` never ran the schema (harmless: no
    /// template reads this field without `--rtk`), and empty for any
    /// procedure whose own signature touches no model.
    pub(crate) touched_models: Vec<String>,
}

pub(crate) fn build_procedure(
    procedure: &Procedure,
    occupied_type_names: &BTreeSet<String>,
    enum_names: &BTreeSet<&str>,
    model_names: &BTreeSet<&str>,
) -> ProcedureView {
    let (revival_kind, revival_scalar_kind, revival_shape_name, revival_paged) =
        match procedure_revival(&procedure.return_type) {
            ProcedureRevival::Scalar(scalar) => ("scalar", scalar.as_str(), String::new(), false),
            ProcedureRevival::Shape { shape_name, paged } => ("shape", "", shape_name, paged),
        };
    ProcedureView {
        name: procedure.name.clone(),
        method_name: to_camel_case(&procedure.name),
        hook_name: to_pascal_case(&procedure.name),
        args_name: procedure_wrapper_name(procedure, occupied_type_names),
        return_type: ts_type(&procedure.return_type, enum_names),
        route: format!("/$procs/{}", procedure.name),
        kind: match procedure.kind {
            ProcedureKind::Query => "query",
            ProcedureKind::Mutation => "mutation",
        },
        query_key: format!("{}Procedure", to_camel_case(&procedure.name)),
        mutation_key: format!("{}Procedure", to_camel_case(&procedure.name)),
        revival_kind,
        revival_scalar_kind,
        revival_shape_name,
        revival_paged,
        touched_models: touched_model_names(procedure, model_names)
            .into_iter()
            .collect(),
    }
}
