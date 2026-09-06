//! `touched_model_names` — the schema-derived signal `--rtk`'s
//! `providesTags`/`invalidatesTags` for custom procedures are built from
//! (issue #906). See `crate::rtk`'s module doc, "Tag derivation" section,
//! for why this is the ticket's actual point rather than a formality.
//!
//! # Precedent, and the one thing this adds beyond it
//!
//! `crate::swr::context_imports::procedure_model_refs` already walks a
//! procedure's `args`/`return_type` for model names, for a DIFFERENT
//! reason (which `models/<model>.js` files `swr/procedures.ts` needs to
//! `import`) and aggregated schema-wide rather than per-procedure. This
//! module needs the SAME signal, but keyed per-procedure (each procedure
//! gets its own tag list) — different enough in shape that reusing the
//! swr function directly would mean unpicking its aggregation, so this is
//! a sibling walk over the same two positions, not a fork of behavior.
//!
//! It additionally recurses into `generic_args` generically — `Page<Model>`
//! AND `FindMany<Model>` — rather than calling `crate::types::base_type_name`
//! alone (that helper only unwraps `Page<T>`; a `FindMany<Model>` argument
//! would otherwise go undetected here).
//!
//! # Known gap, shared with `procedure_model_refs`
//!
//! Only a procedure's OWN `args`/`return_type` are walked — not the fields
//! of a plain `type` a procedure returns/accepts (the
//! `type_references_model.cstack` shape cratestack#626 fixed for
//! `cratestack-client-dart`'s IMPORT computation, a different concern).
//! A procedure that returns a `type` whose own field references a `model`
//! therefore under-derives its tags here exactly as `procedure_model_refs`
//! under-imports there. Closing it needs a `type_decls`-aware recursive
//! walk (Dart's post-#626 `owned_type_decl_model_refs`); deliberately out
//! of this ticket's scope — matching this crate's existing posture at
//! `procedure_model_refs` rather than a new gap introduced here — not
//! guessed at half-tested. Flagged rather than silently narrowed.

use std::collections::BTreeSet;

use cratestack_core::{Procedure, TypeRef};

/// Model names `procedure`'s own `args`/`return_type` reference, sorted
/// (`BTreeSet`) so a generated tag array renders in a stable order across
/// runs regardless of the schema's own declaration order.
pub(crate) fn touched_model_names(
    procedure: &Procedure,
    model_names: &BTreeSet<&str>,
) -> BTreeSet<String> {
    let mut touched = BTreeSet::new();
    for arg in &procedure.args {
        collect(&arg.ty, model_names, &mut touched);
    }
    collect(&procedure.return_type, model_names, &mut touched);
    touched
}

fn collect(type_ref: &TypeRef, model_names: &BTreeSet<&str>, touched: &mut BTreeSet<String>) {
    if model_names.contains(type_ref.name.as_str()) {
        touched.insert(type_ref.name.clone());
    }
    for generic_arg in &type_ref.generic_args {
        collect(generic_arg, model_names, touched);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cratestack_core::{ProcedureArg, ProcedureKind, SourceSpan, TypeArity};

    fn span() -> SourceSpan {
        SourceSpan {
            start: 0,
            end: 0,
            line: 1,
        }
    }

    fn ty(name: &str, arity: TypeArity, generic_args: Vec<TypeRef>) -> TypeRef {
        TypeRef {
            name: name.to_owned(),
            name_span: span(),
            arity,
            generic_args,
            int_args: Vec::new(),
            ident_args: Vec::new(),
        }
    }

    fn arg(name: &str, ty: TypeRef) -> ProcedureArg {
        ProcedureArg {
            docs: Vec::new(),
            name: name.to_owned(),
            name_span: span(),
            ty,
            span: span(),
        }
    }

    fn procedure(args: Vec<ProcedureArg>, return_type: TypeRef) -> Procedure {
        Procedure {
            docs: Vec::new(),
            name: "testProcedure".to_owned(),
            name_span: span(),
            kind: ProcedureKind::Mutation,
            args,
            return_type,
            attributes: Vec::new(),
            span: span(),
        }
    }

    /// The decisive proof issue #906's acceptance criteria ask for: a
    /// mutation procedure's derived tags match the models it ACTUALLY
    /// touches — no more (a bystander model the procedure never mentions
    /// stays out), no fewer (both an argument-position and a
    /// return-position model are found).
    #[test]
    fn derives_exactly_the_models_the_procedure_actually_references() {
        let widget = ty("Widget", TypeArity::Required, Vec::new());
        let order = ty("Order", TypeArity::Required, Vec::new());
        let returns_order = procedure(
            vec![arg("widgetId", ty("Int", TypeArity::Required, Vec::new()))],
            order.clone(),
        );
        // `Ledger` never appears in this procedure's args/return type —
        // proving absence matters exactly as much as proving presence.
        let model_names: BTreeSet<&str> = ["Widget", "Order", "Ledger"].into_iter().collect();
        let touched = touched_model_names(&returns_order, &model_names);
        assert_eq!(
            touched,
            BTreeSet::from(["Order".to_owned()]),
            "only the return type's model should be touched — Widget is an Int arg name, \
             not a Widget-typed one, and Ledger is never referenced at all"
        );

        // Now touch Widget for real, via an argument whose type IS Widget.
        let procedure_with_widget_arg = procedure(
            vec![arg("widget", widget)],
            ty("Int", TypeArity::Required, Vec::new()),
        );
        let touched = touched_model_names(&procedure_with_widget_arg, &model_names);
        assert_eq!(touched, BTreeSet::from(["Widget".to_owned()]));
    }

    #[test]
    fn unwraps_page_and_find_many_generic_wrappers() {
        let model_names: BTreeSet<&str> = ["Widget"].into_iter().collect();

        let paged = procedure(
            Vec::new(),
            ty(
                "Page",
                TypeArity::Required,
                vec![ty("Widget", TypeArity::Required, Vec::new())],
            ),
        );
        assert_eq!(
            touched_model_names(&paged, &model_names),
            BTreeSet::from(["Widget".to_owned()]),
            "Page<Widget> should still be detected as touching Widget"
        );

        let find_many = procedure(
            vec![arg(
                "filter",
                ty(
                    "FindMany",
                    TypeArity::Required,
                    vec![ty("Widget", TypeArity::Required, Vec::new())],
                ),
            )],
            ty("Int", TypeArity::Required, Vec::new()),
        );
        assert_eq!(
            touched_model_names(&find_many, &model_names),
            BTreeSet::from(["Widget".to_owned()]),
            "FindMany<Widget> should still be detected as touching Widget"
        );
    }

    #[test]
    fn a_procedure_touching_nothing_derives_an_empty_set() {
        let model_names: BTreeSet<&str> = ["Widget"].into_iter().collect();
        let procedure = procedure(
            vec![arg("name", ty("String", TypeArity::Required, Vec::new()))],
            ty("String", TypeArity::Required, Vec::new()),
        );
        assert!(touched_model_names(&procedure, &model_names).is_empty());
    }
}
