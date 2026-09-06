use std::collections::{BTreeSet, HashMap};

use cratestack_core::route_naming;
use cratestack_core::{EnumDecl, Field, Model, TypeArity};
use serde::Serialize;

use crate::naming::{escape_ts_string, pluralize, to_camel_case, to_kebab_case, ts_identifier};
use crate::rtk::naming::rtk_endpoint_names;
use crate::types::{
    computed_params_fields, has_parameterized_computed_fields, is_paged_model, model_allows_create,
    primary_key_field, ts_type,
};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EnumView {
    pub(crate) name: String,
    pub(crate) union: String,
    pub(crate) values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct InterfaceView {
    pub(crate) name: String,
    pub(crate) has_fields: bool,
    pub(crate) fields: Vec<FieldView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FieldView {
    pub(crate) property: String,
    pub(crate) wire_name: String,
    pub(crate) type_name: String,
    pub(crate) optional: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModelApiView {
    pub(crate) name: String,
    pub(crate) api_name: String,
    pub(crate) accessor: String,
    /// Issue #610: `README.md.j2`'s `--swr` section needs this model's
    /// `swr` per-model file path (`{{ package_name }}/swr/models/{{
    /// file_stem }}`) for its optimistic-concurrency example — the same
    /// `to_kebab_case(&model.name)` `crate::swr::context` uses to name
    /// that file, kept in sync by sharing the helper rather than
    /// duplicating the transform.
    pub(crate) file_stem: String,
    pub(crate) route: String,
    pub(crate) primary_key_type: String,
    /// The primary key field's OWN property name (e.g. `id`, or `sku` for
    /// a model whose `@id` field isn't called `id`) — issue #906's `--rtk`
    /// needs this to read a list result item's id when building a
    /// `providesTags` entry (`item.{{ model.primary_key_name }}`);
    /// nothing before `--rtk` needed the NAME rather than just the TYPE.
    pub(crate) primary_key_name: String,
    /// cratestack#743: extends the pre-existing (create-only)
    /// `model_allows_create`-based gate to all five CRUD verbs, sourced
    /// from `cratestack_core::model_internal_actions` — the one shared
    /// source of truth every codegen surface consults. `allows_create`
    /// keeps its original `model_allows_create(model)` half (a model
    /// with no `@@allow("create", ...)`/`@@allow("all", ...)` rule at
    /// all fail-closes on the server, so the client shouldn't expose a
    /// `.create()` that can only ever 403 — unaffected by this
    /// change, see the design's non-goal against touching that
    /// presence-based heuristic) ANDed with the new suppression check,
    /// so a model with no `@@internal` at all renders byte-identically
    /// to before. `allows_list`/`allows_get`/`allows_update`/
    /// `allows_delete` are new: those four verbs had no gate at all
    /// before this feature.
    pub(crate) allows_list: bool,
    pub(crate) allows_get: bool,
    pub(crate) allows_create: bool,
    pub(crate) allows_update: bool,
    pub(crate) allows_delete: bool,
    pub(crate) create_input_name: String,
    pub(crate) update_input_name: String,
    pub(crate) list_return_type: String,
    /// `true` when `list_return_type` is `Page<{Model}>` rather than
    /// `{Model}[]` — the generated `list()` method needs to know this to
    /// pick `revivePagedWireFields` (applies this model's decimal shape
    /// to the envelope's `.items`) over plain `reviveWireFields`
    /// (cratestack#499: a `Page<T>` envelope's own keys — `items`/
    /// `totalCount`/`pageInfo` — are never themselves `T`'s fields, so
    /// `T`'s shape can't be applied to the envelope directly).
    pub(crate) is_paged: bool,
    /// This model's own registry key into the generated `wireShapes`
    /// object (`models.ts.j2`) — always just `name` (a model's shape is
    /// always registered under its own schema name), spliced verbatim into
    /// `rest-client.ts.j2`/`rpc-client.ts.j2`'s `reviveWireFields(value,
    /// '{{ revival_shape_name }}')` call at every CRUD method that decodes
    /// a server response. See `crate::decimal`'s module doc for why this
    /// replaced a flat, name-keyed `decimalKeys: string[]` (cratestack#499
    /// review: that scheme had a reachable field-name-collision hazard).
    pub(crate) revival_shape_name: String,
    pub(crate) list_query_key: String,
    pub(crate) get_query_key: String,
    pub(crate) create_mutation_key: String,
    pub(crate) update_mutation_key: String,
    pub(crate) delete_mutation_key: String,
    /// The generated `<Model>ComputedParams` interface's own name (e.g.
    /// `ImageComputedParams`) when `model` declares at least one
    /// parameterized `@computed(params: <Type>?)` field
    /// (`docs/design/computed-fields.md`), else `None`. This is the
    /// per-model type-system gate: templates instantiate
    /// `CratestackQueryRequestConfig<{{ computed_params_interface }}>`
    /// only when this is `Some`, and fall back to the bare (default-`never`)
    /// generic otherwise — so passing `computedParams` on a model with no
    /// parameterized computed field is a `tsc` error, matching the
    /// server's own 422 for the same case. The interface's own field list
    /// is built separately by [`build_computed_params_interface`] and
    /// pushed alongside every other generated interface (never carried on
    /// this view) — this field only carries the *name* a query-config
    /// generic instantiation references.
    pub(crate) computed_params_interface: Option<String>,
    /// Issue #906: this model's five `--rtk` `createApi` endpoint-map
    /// keys (`crate::rtk::naming::rtk_endpoint_names`). Computed once here
    /// — not re-derived by string concatenation inside
    /// `templates/src/rtk-{rest,rpc}.ts.j2` — so `crate::rtk::collisions`'s
    /// pre-render collision check and the actually-rendered object key are
    /// GUARANTEED to agree: a template-side `list{{ model.name }}`
    /// literal would diverge from `rtk_endpoint_names`' camelCase
    /// normalization the moment a model name contains an underscore
    /// (`User_Group` → literal `listUser_Group` vs. normalized
    /// `listUserGroup`), which would make the collision check compare
    /// against a string nothing actually renders.
    pub(crate) rtk_list_key: String,
    pub(crate) rtk_get_key: String,
    pub(crate) rtk_create_key: String,
    pub(crate) rtk_update_key: String,
    pub(crate) rtk_delete_key: String,
}

#[derive(Clone, Copy)]
pub(crate) enum InterfaceKind {
    Plain,
    Patch,
    Model,
}

pub(crate) fn build_enum_view(enum_decl: &EnumDecl) -> EnumView {
    let values = enum_decl
        .variants
        .iter()
        .map(|variant| variant.name.clone())
        .collect::<Vec<_>>();
    let union = values
        .iter()
        .map(|value| format!("'{}'", escape_ts_string(value)))
        .collect::<Vec<_>>()
        .join(" | ");
    EnumView {
        name: enum_decl.name.clone(),
        union,
        values,
    }
}

pub(crate) fn build_interface(
    name: &str,
    fields: &[&Field],
    kind: InterfaceKind,
    enum_names: &BTreeSet<&str>,
) -> InterfaceView {
    InterfaceView {
        name: name.to_owned(),
        has_fields: !fields.is_empty(),
        fields: fields
            .iter()
            .map(|field| {
                let optional = match kind {
                    InterfaceKind::Patch | InterfaceKind::Model => true,
                    InterfaceKind::Plain => field.ty.arity == TypeArity::Optional,
                };
                FieldView {
                    property: ts_identifier(&field.name),
                    wire_name: field.name.clone(),
                    type_name: ts_type(&field.ty, enum_names),
                    optional,
                }
            })
            .collect(),
    }
}

pub(crate) fn build_model_api(model: &Model) -> ModelApiView {
    let primary_key = primary_key_field(model).expect("validated schemas always have an id field");
    // cratestack#345: this route must match the server's real Axum route
    // registration exactly (`cratestack-macros::axum::model::routes`), so
    // it's derived through the shared canonical algorithm rather than
    // this crate's own `to_snake_case`/`pluralize` (which exist for
    // client-only identifier naming — accessor/hook/method names below —
    // and are not wire-format contracts).
    let route = format!("/{}", route_naming::model_route_segment(&model.name));
    let accessor = pluralize(&to_camel_case(&model.name));
    let is_paged = is_paged_model(model);
    let internal = cratestack_core::model_internal_actions(model);
    let rtk_names = rtk_endpoint_names(&model.name);
    ModelApiView {
        name: model.name.clone(),
        api_name: format!("{}Api", model.name),
        accessor,
        file_stem: to_kebab_case(&model.name),
        route,
        primary_key_type: ts_type(&primary_key.ty, &BTreeSet::new()),
        primary_key_name: ts_identifier(&primary_key.name),
        allows_list: !internal.contains("list"),
        allows_get: !internal.contains("get"),
        allows_create: model_allows_create(model) && !internal.contains("create"),
        allows_update: !internal.contains("update"),
        allows_delete: !internal.contains("delete"),
        create_input_name: format!("Create{}Input", model.name),
        update_input_name: format!("Update{}Input", model.name),
        list_return_type: if is_paged {
            format!("Page<{}>", model.name)
        } else {
            format!("{}[]", model.name)
        },
        is_paged,
        revival_shape_name: model.name.clone(),
        list_query_key: format!("{}List", to_camel_case(&model.name)),
        get_query_key: format!("{}Detail", to_camel_case(&model.name)),
        create_mutation_key: format!("{}Create", to_camel_case(&model.name)),
        update_mutation_key: format!("{}Update", to_camel_case(&model.name)),
        delete_mutation_key: format!("{}Delete", to_camel_case(&model.name)),
        computed_params_interface: has_parameterized_computed_fields(model)
            .then(|| computed_params_interface_name(&model.name)),
        rtk_list_key: rtk_names.list,
        rtk_get_key: rtk_names.get,
        rtk_create_key: rtk_names.create,
        rtk_update_key: rtk_names.update,
        rtk_delete_key: rtk_names.delete,
    }
}

/// The generated `<Model>ComputedParams` interface's own name — always
/// `{ModelName}ComputedParams`, the same convention `Create{Model}Input`/
/// `Update{Model}Input` already use for their own model-derived names.
pub(crate) fn computed_params_interface_name(model_name: &str) -> String {
    format!("{model_name}ComputedParams")
}

/// Builds the `<Model>ComputedParams` interface (`docs/design/computed-fields.md`
/// §"Downstream") — one optional property per parameterized
/// `@computed(params: <Type>?)` field on `model`, wire-keyed by the
/// field's own name and typed as its declared params `type`. `None` when
/// `model` has no such field, which callers treat as "no interface to
/// emit" (mirrors [`ModelApiView::computed_params_interface`] being
/// `None` in that same case).
///
/// Deliberately not built on top of [`build_interface`]: that helper maps
/// a field's own `TypeRef` through `ts_type`, which would append `| null`
/// for the field's arity — but here the *interface property* itself
/// carries the optionality (every entry is `?`, since a caller supplying
/// `computedParams` need not populate every parameterized field), and the
/// *value* type is the params `type`'s bare name, not the computed
/// field's own declared type.
pub(crate) fn build_computed_params_interface(model: &Model) -> Option<InterfaceView> {
    let fields = computed_params_fields(model);
    if fields.is_empty() {
        return None;
    }
    Some(InterfaceView {
        name: computed_params_interface_name(&model.name),
        has_fields: true,
        fields: fields
            .into_iter()
            .map(|(field, params_type)| FieldView {
                property: ts_identifier(&field.name),
                wire_name: field.name.clone(),
                type_name: params_type.to_owned(),
                optional: true,
            })
            .collect(),
    })
}

/// `list_query_key`/`get_query_key`/etc. are each derived from
/// `to_camel_case(&model.name)`, which is a lossy transform: two
/// distinct, parser-guaranteed-unique model names (e.g. `UserGroup` and
/// `User_Group`) can normalize to the same camelCase prefix and
/// therefore the same key. `list_query_key`/`get_query_key` are rendered
/// as sibling property names in the same `cratestackQueryKeys` object
/// literal (`rest-react-query.ts.j2`/`rpc-react-query.ts.j2`), so an
/// undetected collision is a TypeScript compile error
/// (`ts(1117)`), not just a runtime cache-key overlap.
///
/// Call this once per schema, after every model's `ModelApiView` has
/// been built, so each field's collisions can be detected across the
/// *whole* model list rather than per-model in isolation. Colliding
/// entries are suffixed with their own model's raw name — which the
/// parser already guarantees is unique verbatim across the schema
/// (`cratestack-parser`'s `ensure_unique` over the shared type/model/enum
/// namespace) — so the disambiguated key is guaranteed unique too.
pub(crate) fn disambiguate_model_api_keys(models: &mut [ModelApiView]) {
    disambiguate_field(
        models,
        |view| &view.list_query_key,
        |view, key| {
            view.list_query_key = key;
        },
    );
    disambiguate_field(
        models,
        |view| &view.get_query_key,
        |view, key| {
            view.get_query_key = key;
        },
    );
    disambiguate_field(
        models,
        |view| &view.create_mutation_key,
        |view, key| {
            view.create_mutation_key = key;
        },
    );
    disambiguate_field(
        models,
        |view| &view.update_mutation_key,
        |view, key| {
            view.update_mutation_key = key;
        },
    );
    disambiguate_field(
        models,
        |view| &view.delete_mutation_key,
        |view, key| {
            view.delete_mutation_key = key;
        },
    );
}

fn disambiguate_field(
    models: &mut [ModelApiView],
    get: impl Fn(&ModelApiView) -> &String,
    mut set: impl FnMut(&mut ModelApiView, String),
) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for view in models.iter() {
        *counts.entry(get(view).clone()).or_insert(0) += 1;
    }
    for view in models.iter_mut() {
        if counts.get(get(view).as_str()).copied().unwrap_or(0) > 1 {
            let disambiguated = format!("{}_{}", get(view), view.name);
            set(view, disambiguated);
        }
    }
}

/// Renders `items` as a TS array-literal source fragment of single-quoted
/// string literals (`['a', 'b']`, or `[]` for an empty slice) — the same
/// quoting `build_enum_view`'s union-type rendering uses, via the same
/// `escape_ts_string` helper. `pub(crate)` (not just used within this
/// module) since `crate::decimal::build_shape` reuses it for each
/// `WireShapeView`'s key arrays.
pub(crate) fn js_string_array(items: &[String]) -> String {
    let quoted = items
        .iter()
        .map(|item| format!("'{}'", escape_ts_string(item)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{quoted}]")
}
