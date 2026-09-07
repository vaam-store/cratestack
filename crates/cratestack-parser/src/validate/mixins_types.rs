use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{Schema, TypeArity};

use crate::diagnostics::{SchemaError, span_error};
use crate::validate::builder_setter_collisions::{
    validate_no_add_setter_collision, validate_no_build_setter_collision,
};
use crate::validate::collect::record;
use crate::validate::computed_attribute::{
    ComputedFieldSupport, validate_computed_field_attribute,
};
use crate::validate::fields::{
    validate_default_dbgenerated_no_args, validate_field_reserved_identifier,
};
use crate::validate::misspelled_attributes::validate_misspelled_field_attributes;
use crate::validate::removed_attributes::validate_removed_field_attributes;
use crate::validate::reserved_idents::validate_reserved_identifier;
use crate::validate::reserved_keywords::validate_reserved_keyword;
use crate::validate::snake_case_collisions::validate_field_column_collisions;
use crate::validate::type_names::validate_type_ref;

/// Each mixin is checked independently.
pub(super) fn validate_mixins_collecting(
    schema: &Schema,
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
    model_names: &BTreeSet<String>,
    errors: &mut Vec<SchemaError>,
) {
    for mixin in &schema.mixins {
        record(errors, || {
            validate_reserved_identifier(
                &mixin.name,
                mixin.name_span,
                &format!("mixin `{}`", mixin.name),
            )?;
            validate_reserved_keyword(
                &mixin.name,
                mixin.name_span,
                &format!("mixin `{}`", mixin.name),
            )?;
            validate_field_column_collisions(&mixin.fields, "mixin", &mixin.name)?;

            let mut fields = BTreeMap::new();
            for field in &mixin.fields {
                if fields.insert(field.name.clone(), field.span).is_some() {
                    return Err(span_error(
                        format!("duplicate field `{}` on mixin `{}`", field.name, mixin.name),
                        field.span,
                    ));
                }
                if field
                    .attributes
                    .iter()
                    .any(|attribute| attribute.raw.starts_with("@id"))
                {
                    return Err(span_error(
                        format!(
                            "field `{}` on mixin `{}` cannot declare @id",
                            field.name, mixin.name
                        ),
                        field.span,
                    ));
                }
                if field
                    .attributes
                    .iter()
                    .any(|attribute| attribute.raw.starts_with("@@"))
                {
                    return Err(span_error(
                        format!(
                            "field `{}` on mixin `{}` cannot declare model-level attributes",
                            field.name, mixin.name
                        ),
                        field.span,
                    ));
                }
                validate_computed_field_attribute(
                    field,
                    "mixin",
                    &mixin.name,
                    ComputedFieldSupport::Rejected,
                )?;
                validate_field_reserved_identifier(field, "mixin", &mixin.name)?;
                validate_type_ref(
                    type_names,
                    page_item_type_names,
                    model_names,
                    &schema.declared_extensions,
                    &field.ty,
                    field.span,
                    crate::validate::type_names::TypeRefAllow {
                        vector: true,
                        spatial: true,
                        ..Default::default()
                    },
                )?;
                validate_default_dbgenerated_no_args(&mixin.name, field)?;
                validate_removed_field_attributes("mixin", &mixin.name, field)?;
                validate_misspelled_field_attributes("mixin", &mixin.name, field)?;
            }
            Ok(())
        });
    }
}

/// Each `type` block is checked independently.
pub(super) fn validate_types_collecting(
    schema: &Schema,
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
    model_names: &BTreeSet<String>,
    errors: &mut Vec<SchemaError>,
) {
    for ty in &schema.types {
        record(errors, || {
            validate_reserved_identifier(&ty.name, ty.name_span, &format!("type `{}`", ty.name))?;
            validate_reserved_keyword(&ty.name, ty.name_span, &format!("type `{}`", ty.name))?;
            validate_field_column_collisions(&ty.fields, "type", &ty.name)?;
            validate_no_build_setter_collision(
                ty.fields
                    .iter()
                    .map(|field| (field.name.as_str(), field.span)),
                "type",
                &ty.name,
            )?;
            validate_no_add_setter_collision(
                ty.fields.iter().map(|field| {
                    (
                        field.name.as_str(),
                        field.span,
                        matches!(field.ty.arity, TypeArity::List),
                    )
                }),
                "type",
                &ty.name,
            )?;

            let mut fields = BTreeSet::new();
            for field in &ty.fields {
                if !fields.insert(field.name.clone()) {
                    return Err(span_error(
                        format!("duplicate field `{}` on type `{}`", field.name, ty.name),
                        field.span,
                    ));
                }
                validate_computed_field_attribute(
                    field,
                    "type",
                    &ty.name,
                    ComputedFieldSupport::Supported,
                )?;
                validate_field_reserved_identifier(field, "type", &ty.name)?;
                validate_type_ref(
                    type_names,
                    page_item_type_names,
                    model_names,
                    &schema.declared_extensions,
                    &field.ty,
                    field.span,
                    crate::validate::type_names::TypeRefAllow {
                        vector: true,
                        spatial: true,
                        ..Default::default()
                    },
                )?;
                validate_removed_field_attributes("type", &ty.name, field)?;
                validate_misspelled_field_attributes("type", &ty.name, field)?;
            }
            Ok(())
        });
    }
}

/// Each enum is checked independently so one bad enum does not hide the next.
pub(super) fn validate_enums_collecting(schema: &Schema, errors: &mut Vec<SchemaError>) {
    for enum_decl in &schema.enums {
        record(errors, || {
            validate_reserved_identifier(
                &enum_decl.name,
                enum_decl.name_span,
                &format!("enum `{}`", enum_decl.name),
            )?;
            validate_reserved_keyword(
                &enum_decl.name,
                enum_decl.name_span,
                &format!("enum `{}`", enum_decl.name),
            )?;

            let mut variants = BTreeSet::new();
            for variant in &enum_decl.variants {
                if !variants.insert(variant.name.clone()) {
                    return Err(span_error(
                        format!(
                            "duplicate variant `{}` on enum `{}`",
                            variant.name, enum_decl.name
                        ),
                        variant.span,
                    ));
                }
                validate_reserved_identifier(
                    &variant.name,
                    variant.span,
                    &format!("variant `{}` on enum `{}`", variant.name, enum_decl.name),
                )?;
                validate_reserved_keyword(
                    &variant.name,
                    variant.span,
                    &format!("variant `{}` on enum `{}`", variant.name, enum_decl.name),
                )?;
            }
            Ok(())
        });
    }
}

pub(super) fn validate_auth(
    schema: &Schema,
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
    model_names: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    if let Some(auth) = &schema.auth {
        validate_field_column_collisions(&auth.fields, "auth block", &auth.name)?;

        let mut fields = BTreeSet::new();
        for field in &auth.fields {
            if !fields.insert(field.name.clone()) {
                return Err(span_error(
                    format!(
                        "duplicate field `{}` on auth block `{}`",
                        field.name, auth.name
                    ),
                    field.span,
                ));
            }
            validate_computed_field_attribute(
                field,
                "auth block",
                &auth.name,
                ComputedFieldSupport::Rejected,
            )?;
            validate_field_reserved_identifier(field, "auth block", &auth.name)?;
            validate_type_ref(
                type_names,
                page_item_type_names,
                model_names,
                &schema.declared_extensions,
                &field.ty,
                field.span,
                crate::validate::type_names::TypeRefAllow {
                    vector: true,
                    spatial: true,
                    ..Default::default()
                },
            )?;
            validate_removed_field_attributes("auth block", &auth.name, field)?;
            validate_misspelled_field_attributes("auth block", &auth.name, field)?;
        }
    }
    Ok(())
}
