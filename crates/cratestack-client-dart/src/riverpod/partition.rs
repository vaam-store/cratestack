//! Owned-vs-shared type partition for the `riverpod` preset (issue #301).
//!
//! The `default` preset renders every schema `type`/`enum` into one flat
//! `models.dart` — no ownership question ever arises. The `riverpod`
//! preset splits generation per model, so every nested `type`/`enum` needs
//! a single file to live in. This module computes that assignment.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cratestack_core::{Schema, TypeDecl, TypeRef, computed_params_type_name};

use crate::naming::{is_relation_field, model_name_set};

/// Which generated file a nested `type`/`enum` belongs to.
///
/// **The ownership rule:** a `type`/`enum` is reachable from a "locus" —
/// a model (via its own non-relation fields) or `Procedures` (via every
/// procedure's args/return type) — by following field references
/// transitively through other nested `type`s. Exactly one locus reaching
/// it means that locus owns it (`Owner::Model` inlines into that model's
/// own file; `Owner::Procedures` into `procedures.dart`). Zero loci (an
/// orphan declared but unused — the `default` preset still renders these,
/// so this preset must not drop them) or two-or-more loci means it's
/// genuinely shared: `lib/src/models/shared_types.dart`, imported rather
/// than duplicated. Model-to-model relation fields never contribute to
/// this graph — a relation just makes one model's file import another's
/// directly (see `crate::riverpod::imports::model_relation_targets`).
///
/// **Narrower asymmetry than it looks:** an `enum` can be any model
/// field's type, so two models genuinely sharing one is common
/// (`partition_tests.rs`'s `enum_used_by_two_models_is_shared`). A
/// nested `type` block can only be a model field's type when that field
/// carries `@computed` — `cratestack-parser/src/validate/
/// type_names.rs`'s `reject_type_decl_as_model_field_type` still rejects
/// a *stored* model field typed as a `type` block at parse time, but
/// exempts a `@computed` one, since a computed field is never a column
/// (see that function's doc for the full rationale). The seed loop below
/// doesn't distinguish stored from computed fields, so a `type` reached
/// only through `@computed` fields on two different models is genuinely
/// multi-model-owned too, the same as an `enum` — a `type` is only
/// guaranteed single-owner (or an orphan) when its sole reachers are
/// procedures.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Owner {
    Model(String),
    Procedures,
    Shared,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TypePartition {
    type_owner: BTreeMap<String, Owner>,
    enum_owner: BTreeMap<String, Owner>,
    /// Per-locus reachable name set, retained so callers can split each
    /// locus's reachable names into "owned here" vs "defined in
    /// `shared_types.dart`, needs an import" without recomputing BFS.
    reachable: BTreeMap<Owner, BTreeSet<String>>,
}

impl TypePartition {
    pub(crate) fn type_owner(&self, name: &str) -> &Owner {
        self.type_owner.get(name).unwrap_or(&Owner::Shared)
    }

    pub(crate) fn enum_owner(&self, name: &str) -> &Owner {
        self.enum_owner.get(name).unwrap_or(&Owner::Shared)
    }

    /// Names (type or enum) reachable from `locus` and owned by it —
    /// belongs inlined directly into `locus`'s own generated file.
    pub(crate) fn owned_names(&self, locus: &Owner) -> BTreeSet<&str> {
        self.reachable
            .get(locus)
            .into_iter()
            .flatten()
            .filter(|name| self.owner_by_name(name) == locus)
            .map(String::as_str)
            .collect()
    }

    /// Names (type or enum) reachable from `locus` but owned by
    /// `Owner::Shared` — `locus`'s generated file needs to import
    /// `shared_types.dart` for these rather than define them.
    pub(crate) fn shared_refs(&self, locus: &Owner) -> BTreeSet<&str> {
        self.reachable
            .get(locus)
            .into_iter()
            .flatten()
            .filter(|name| *self.owner_by_name(name) == Owner::Shared)
            .map(String::as_str)
            .collect()
    }

    fn owner_by_name(&self, name: &str) -> &Owner {
        self.type_owner
            .get(name)
            .or_else(|| self.enum_owner.get(name))
            .unwrap_or(&Owner::Shared)
    }
}

pub(crate) fn partition_types(schema: &Schema) -> TypePartition {
    let model_names = model_name_set(&schema.models);
    let type_by_name: BTreeMap<&str, &TypeDecl> = schema
        .types
        .iter()
        .map(|ty| (ty.name.as_str(), ty))
        .collect();
    let enum_names: BTreeSet<&str> = schema.enums.iter().map(|e| e.name.as_str()).collect();

    let mut reachable: BTreeMap<Owner, BTreeSet<String>> = BTreeMap::new();

    for model in &schema.models {
        // The typed client computedParams surface — see
        // `docs/design/computed-fields.md`'s "Downstream" section: a
        // `@computed(params: <Type>?)` field's `<Type>` is
        // never a field's own declared type (`field.ty` stays the
        // computed field's *return* type, e.g. `String`), so it would
        // otherwise never enter this model's reachable set — leaving the
        // generated `{Model}ComputedParams` class's own `<Type>?` field
        // referencing a `type` this model's own file never imports.
        // Chained in as an extra seed per field, same as any other
        // directly-referenced name.
        let seeds = model
            .fields
            .iter()
            .filter(|field| !is_relation_field(&model_names, field))
            .flat_map(|field| {
                std::iter::once(referenced_name(&field.ty))
                    .chain(computed_params_type_name(field).map(str::to_owned))
            });
        reachable.insert(
            Owner::Model(model.name.clone()),
            reachable_closure(seeds, &type_by_name, &enum_names),
        );
    }

    let procedure_seeds = schema.procedures.iter().flat_map(|procedure| {
        procedure
            .args
            .iter()
            .map(|arg| referenced_name(&arg.ty))
            .chain(std::iter::once(referenced_name(&procedure.return_type)))
    });
    reachable.insert(
        Owner::Procedures,
        reachable_closure(procedure_seeds, &type_by_name, &enum_names),
    );

    let type_owner = schema
        .types
        .iter()
        .map(|ty| (ty.name.clone(), owner_of(&ty.name, &reachable)))
        .collect();
    let enum_owner = schema
        .enums
        .iter()
        .map(|decl| (decl.name.clone(), owner_of(&decl.name, &reachable)))
        .collect();

    TypePartition {
        type_owner,
        enum_owner,
        reachable,
    }
}

fn owner_of(name: &str, reachable: &BTreeMap<Owner, BTreeSet<String>>) -> Owner {
    let mut owners = reachable
        .iter()
        .filter(|(_, names)| names.contains(name))
        .map(|(owner, _)| owner.clone());
    match (owners.next(), owners.next()) {
        (Some(only), None) => only,
        _ => Owner::Shared,
    }
}

/// Unwraps `Page<T>`/`FindMany<T>` to `T`'s name; every other arity/shape
/// just carries its own `TypeRef.name` (built-in scalar, model, nested
/// `type`, or `enum` — the caller sorts out which). The `FindMany<T>`
/// branch matters for `build_procedures.rs`'s `referenced_models`: a
/// `FindMany<Post>` argument resolves to a `PostFindMany` Dart class
/// defined in `models/post.dart` (see `build_model.rs`), so
/// `procedures.dart` needs to detect "Post" as the referenced model the
/// same way it already does for a plain `Post`/`Page<Post>` arg —
/// otherwise the import is missing and `PostFindMany` fails to resolve.
pub(crate) fn referenced_name(ty: &TypeRef) -> String {
    if ty.is_page() {
        let item = ty
            .page_item()
            .expect("validated Page<T> should include an item type");
        return referenced_name(item);
    }
    if ty.is_find_many() {
        let item = ty
            .find_many_item()
            .expect("validated FindMany<T> should include an item type");
        return referenced_name(item);
    }
    ty.name.clone()
}

fn reachable_closure(
    seeds: impl Iterator<Item = String>,
    type_by_name: &BTreeMap<&str, &TypeDecl>,
    enum_names: &BTreeSet<&str>,
) -> BTreeSet<String> {
    let mut visited_types: BTreeSet<String> = BTreeSet::new();
    let mut result = BTreeSet::new();
    let mut queue: VecDeque<String> = seeds.collect();

    while let Some(name) = queue.pop_front() {
        if enum_names.contains(name.as_str()) {
            result.insert(name);
            continue;
        }
        if let Some(type_decl) = type_by_name.get(name.as_str()) {
            if !visited_types.insert(name.clone()) {
                continue;
            }
            result.insert(name.clone());
            for field in &type_decl.fields {
                queue.push_back(referenced_name(&field.ty));
            }
        }
        // Otherwise `name` is a built-in scalar or a model name — models
        // are handled by `build_model::relation_targets`, not this
        // partition.
    }

    result
}

#[cfg(test)]
#[path = "partition_tests.rs"]
mod tests;
