//! Generation-time collision check for `--rtk` (cratestack#906) — the
//! `--rtk` analogue of `crate::tanstack_collisions`, following the same
//! stance (#344/#777): this generator derives identifiers the schema
//! author never wrote, so it refuses a schema whose derived names collide
//! instead of pushing the failure downstream.
//!
//! Model-vs-model collisions are structurally impossible under
//! `crate::rtk::naming`'s scheme (see that module's doc) — the ONE
//! collision left reachable is procedure-vs-model, exactly the shape
//! `tanstack_collisions`/`crate::swr::collisions` already guard for their
//! own naming schemes. `--rtk`'s failure mode if this went unchecked is
//! sharper than either: RTK Query's endpoint map is a single object
//! literal, so a colliding key is `ts(1117)` ("An object literal cannot
//! have multiple properties with the same name") at best, or silent
//! last-write-wins clobbering at worst if a future TypeScript relaxed
//! that check — either way, a real defect this check exists to refuse
//! before it is rendered at all.

use std::collections::HashMap;

use crate::error::TypeScriptGeneratorError;
use crate::procedure_views::ProcedureView;
use crate::views::ModelApiView;

/// Refuses a schema whose `--rtk` procedure endpoint key equals one of the
/// five derived model endpoint keys.
pub(crate) fn reject_rtk_endpoint_name_collisions(
    models: &[ModelApiView],
    procedures: &[ProcedureView],
) -> Result<(), TypeScriptGeneratorError> {
    let mut derived: HashMap<String, (&str, &'static str)> = HashMap::new();
    for model in models {
        for (identifier, operation) in emitted_model_endpoint_names(model) {
            // First writer wins, matching `tanstack_collisions`'s posture:
            // a model-vs-model collision is structurally impossible here
            // (see `crate::rtk::naming`'s doc), so this branch only ever
            // reconciles the same model appearing once — nothing is lost.
            derived
                .entry(identifier)
                .or_insert((model.name.as_str(), operation));
        }
    }

    for procedure in procedures {
        if let Some(&(model, operation)) = derived.get(procedure.method_name.as_str()) {
            return Err(TypeScriptGeneratorError::RtkEndpointNameCollision {
                procedure: procedure.name.clone(),
                identifier: procedure.method_name.clone(),
                model: model.to_owned(),
                operation,
            });
        }
    }
    Ok(())
}

/// The subset of a model's five derived endpoint keys that this model
/// actually emits — gated exactly as `templates/src/rtk-{rest,rpc}.ts.j2`
/// gate them, mirroring `tanstack_collisions::emitted_model_hook_names`'s
/// same reasoning: a key suppressed by `@@internal` or a missing `create`
/// rule is never emitted, so it cannot collide.
///
/// Reads the SAME `ModelApiView::rtk_*_key` fields the templates
/// interpolate — never re-derives them by string concatenation — so this
/// check can never drift from what actually renders (see that field's own
/// doc comment for the underscore-in-model-name case that would otherwise
/// diverge).
fn emitted_model_endpoint_names(model: &ModelApiView) -> Vec<(String, &'static str)> {
    let mut result = Vec::new();
    if model.allows_list {
        result.push((model.rtk_list_key.clone(), "list"));
    }
    if model.allows_get {
        result.push((model.rtk_get_key.clone(), "get"));
    }
    if model.allows_create {
        result.push((model.rtk_create_key.clone(), "create"));
    }
    if model.allows_update {
        result.push((model.rtk_update_key.clone(), "update"));
    }
    if model.allows_delete {
        result.push((model.rtk_delete_key.clone(), "delete"));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::TypeScriptGeneratorError;

    fn model(name: &str) -> ModelApiView {
        let rtk_names = crate::rtk::naming::rtk_endpoint_names(name);
        ModelApiView {
            name: name.to_owned(),
            api_name: format!("{name}Api"),
            accessor: name.to_lowercase(),
            file_stem: name.to_lowercase(),
            route: format!("/{}", name.to_lowercase()),
            primary_key_type: "number".to_owned(),
            primary_key_name: "id".to_owned(),
            allows_list: true,
            allows_get: true,
            allows_create: true,
            allows_update: true,
            allows_delete: true,
            create_input_name: format!("Create{name}Input"),
            update_input_name: format!("Update{name}Input"),
            list_return_type: format!("{name}[]"),
            is_paged: false,
            revival_shape_name: name.to_owned(),
            list_query_key: format!("{name}List"),
            get_query_key: format!("{name}Detail"),
            create_mutation_key: format!("{name}Create"),
            update_mutation_key: format!("{name}Update"),
            delete_mutation_key: format!("{name}Delete"),
            computed_params_interface: None,
            rtk_list_key: rtk_names.list,
            rtk_get_key: rtk_names.get,
            rtk_create_key: rtk_names.create,
            rtk_update_key: rtk_names.update,
            rtk_delete_key: rtk_names.delete,
        }
    }

    /// The decisive proof: `procedure list_widget` derives the exact same
    /// endpoint key `Widget`'s own `list` operation does (`listWidget`),
    /// and this must be REFUSED, not silently rendered as one of the two
    /// colliding object keys.
    #[test]
    fn refuses_a_procedure_that_collides_with_a_derived_model_endpoint_key() {
        let models = vec![model("Widget")];
        let procedure = crate::procedure_views::ProcedureView {
            name: "list_widget".to_owned(),
            method_name: "listWidget".to_owned(),
            hook_name: "ListWidget".to_owned(),
            args_name: "ListWidgetArgs".to_owned(),
            return_type: "unknown".to_owned(),
            route: "/$procs/list_widget".to_owned(),
            kind: "query",
            query_key: "listWidgetProcedure".to_owned(),
            mutation_key: "listWidgetProcedure".to_owned(),
            revival_kind: "shape",
            revival_scalar_kind: "",
            revival_shape_name: String::new(),
            revival_paged: false,
            touched_models: Vec::new(),
        };

        let error = reject_rtk_endpoint_name_collisions(&models, &[procedure])
            .expect_err("a colliding procedure/model endpoint key must be refused");
        assert!(matches!(
            error,
            TypeScriptGeneratorError::RtkEndpointNameCollision { .. }
        ));
    }

    #[test]
    fn distinct_names_never_collide() {
        let models = vec![model("Widget")];
        let procedure = crate::procedure_views::ProcedureView {
            name: "sync_inventory".to_owned(),
            method_name: "syncInventory".to_owned(),
            hook_name: "SyncInventory".to_owned(),
            args_name: "SyncInventoryArgs".to_owned(),
            return_type: "unknown".to_owned(),
            route: "/$procs/sync_inventory".to_owned(),
            kind: "mutation",
            query_key: "syncInventoryProcedure".to_owned(),
            mutation_key: "syncInventoryProcedure".to_owned(),
            revival_kind: "shape",
            revival_scalar_kind: "",
            revival_shape_name: String::new(),
            revival_paged: false,
            touched_models: Vec::new(),
        };
        assert!(reject_rtk_endpoint_name_collisions(&models, &[procedure]).is_ok());
    }
}
