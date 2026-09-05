//! The `swr` preset's owned-vs-shared type partition (issue #304).
//!
//! ## The ownership rule
//!
//! Every enum and `type` block ("eligible" names below) is placed in
//! exactly one of three places:
//!
//! - **A single model's file** (`src/models/<model>.ts`), inline, if that
//!   model is the *only* consumer that reaches the type (directly, or
//!   transitively through another eligible type's own fields — e.g. a
//!   `type Address` referencing an `enum Country`).
//! - **`src/procedures.ts`**, inline, if only procedures reach it.
//! - **`src/models/shared.ts`**, if two or more consumers reach it (or —
//!   the safe default — if *no* model or procedure reaches it at all, e.g.
//!   a declared-but-unused type; the default preset still emits every
//!   declared type regardless of use, and `swr` preserves that).
//!
//! A model's own generated types (the model interface, `Create<X>Input`,
//! `Update<X>Input`) and a procedure's own `<Name>Args` wrapper are never
//! subject to this rule — they're already named after their one owner
//! (`crate::naming::procedure_wrapper_name`), so there's no duplication
//! risk to guard against.
//!
//! Relations (a model field typed as another model, e.g. `author User`)
//! are also out of scope here: they're never duplicated or shared in the
//! way an enum/type can be — they always mean "import the other model's
//! own type from its own file" — so `crate::swr::context` handles them
//! directly rather than through this ownership computation.
//!
//! ## A `type` block can reach a model only through `@computed`
//!
//! Verified against `cratestack-parser`'s semantic checker
//! (`validate/type_names.rs`): a *stored* model field cannot be typed as
//! a `type` block — it's a hard parse error ("`type` blocks are not
//! backed by a database column ... use a scalar, an `enum`, or a
//! `@relation`") — but `reject_type_decl_as_model_field_type` exempts a
//! `@computed` field from that check, since a computed field is never a
//! column. So a `type` block's entry points are procedure args/return
//! types, another `type` block's own fields (transitively), *and* now a
//! model's own `@computed` field. Two *models* genuinely sharing a
//! `type` block this way is therefore just as real as two models sharing
//! an `enum` — `model_referenced_eligible_names` (`ownership_graph.rs`)
//! walks every visible field, computed or not, so this falls out without
//! any extra code. Two *procedures* can also share a `type` block, and
//! since `src/procedures.ts` is one file regardless of how many
//! procedures it holds, this module tracks each procedure as its own
//! consumer (not one lumped "Procedures" bucket) specifically so that
//! case still exercises real `Shared` classification instead of
//! accidentally always winning by single-consumer default.
//! `TypeOwner::Procedures` still means exactly one file either way
//! (`src/procedures.ts`) — the per-procedure tracking only changes
//! *whether* a multiply-referenced type counts as shared, not which file
//! a procedures-only type lands in.
//!
//! ## Why this can't under-share
//!
//! If eligible type `A`'s fields reference eligible type `B`, every
//! consumer that reaches `A` also reaches `B` (BFS follows that edge
//! regardless of which consumer is walking). So `consumers(A) ⊆
//! consumers(B)` for every direct edge `A → B`, which means: whenever `A`
//! is shared (2+ consumers), `B` has at least as many consumers and is
//! therefore shared too. A model-owned or procedures-owned type can only
//! be reached by its one owner, transitively — so inlining a whole
//! single-owner closure into one file (see `crate::swr::context`) never
//! misses a cross-file reference to another eligible type. It can still
//! reference a *model* type directly (not just another eligible type) —
//! `crate::swr::context` scans owned/shared type bodies for that
//! separately, since it's a different reference kind (see module doc
//! above).

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::Schema;

use super::ownership_graph::{
    Consumer, model_referenced_eligible_names, procedure_referenced_eligible_names, reachable_set,
    type_decl_adjacency,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TypeOwner {
    Model(String),
    Procedures,
    Shared,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TypeOwnership {
    owners: BTreeMap<String, TypeOwner>,
    model_reachable: BTreeMap<String, BTreeSet<String>>,
    procedures_reachable: BTreeSet<String>,
}

impl TypeOwnership {
    pub(crate) fn owner_of(&self, name: &str) -> Option<&TypeOwner> {
        self.owners.get(name)
    }

    /// Eligible (enum/type) names this model needs to `import type` from
    /// `./shared` — the subset of what it transitively reaches that
    /// isn't inlined into its own file.
    pub(crate) fn shared_imports_for_model(&self, model_name: &str) -> Vec<String> {
        self.model_reachable
            .get(model_name)
            .into_iter()
            .flatten()
            .filter(|name| matches!(self.owner_of(name), Some(TypeOwner::Shared)))
            .cloned()
            .collect()
    }

    /// Same as [`Self::shared_imports_for_model`], for `src/procedures.ts`.
    pub(crate) fn shared_imports_for_procedures(&self) -> Vec<String> {
        self.procedures_reachable
            .iter()
            .filter(|name| matches!(self.owner_of(name), Some(TypeOwner::Shared)))
            .cloned()
            .collect()
    }
}

pub(crate) fn compute_type_ownership(schema: &Schema) -> TypeOwnership {
    let eligible: BTreeSet<&str> = schema
        .types
        .iter()
        .map(|ty| ty.name.as_str())
        .chain(schema.enums.iter().map(|e| e.name.as_str()))
        .collect();

    let adjacency: BTreeMap<&str, BTreeSet<&str>> = schema
        .types
        .iter()
        .map(|ty| {
            (
                ty.name.as_str(),
                type_decl_adjacency(ty.fields.iter(), &eligible),
            )
        })
        .collect();

    let mut consumers_of: BTreeMap<&str, BTreeSet<Consumer>> = eligible
        .iter()
        .map(|name| (*name, BTreeSet::new()))
        .collect();
    let mut model_reachable = BTreeMap::new();
    let mut procedures_reachable = BTreeSet::new();

    for model in &schema.models {
        let roots = model_referenced_eligible_names(model, &eligible);
        let reachable = reachable_set(&roots, &adjacency);
        for &name in &reachable {
            consumers_of
                .entry(name)
                .or_default()
                .insert(Consumer::Model(model.name.clone()));
        }
        model_reachable.insert(
            model.name.clone(),
            reachable.into_iter().map(str::to_owned).collect(),
        );
    }

    for procedure in &schema.procedures {
        let roots = procedure_referenced_eligible_names(procedure, &eligible);
        let reachable = reachable_set(&roots, &adjacency);
        for &name in &reachable {
            consumers_of
                .entry(name)
                .or_default()
                .insert(Consumer::Procedure(procedure.name.clone()));
        }
        procedures_reachable.extend(reachable.into_iter().map(str::to_owned));
    }

    let owners = consumers_of
        .into_iter()
        .map(|(name, consumers)| {
            let owner = match consumers.len() {
                1 => match consumers.into_iter().next().expect("len checked above") {
                    Consumer::Model(model_name) => TypeOwner::Model(model_name),
                    Consumer::Procedure(_) => TypeOwner::Procedures,
                },
                _ => TypeOwner::Shared,
            };
            (name.to_owned(), owner)
        })
        .collect();

    TypeOwnership {
        owners,
        model_reachable,
        procedures_reachable,
    }
}

#[cfg(test)]
#[path = "ownership_tests.rs"]
mod tests;
