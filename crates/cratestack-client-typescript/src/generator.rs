use cratestack_core::Schema;

use crate::config::{
    GeneratedTypeScriptFile, GeneratedTypeScriptPackage, TypeScriptGeneratorConfig,
};
use crate::context::build_template_context;
use crate::error::TypeScriptGeneratorError;
use crate::templates::{OutputPath, build_environment, template_specs_for};

pub fn generate_package(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
) -> Result<GeneratedTypeScriptPackage, TypeScriptGeneratorError> {
    // Composite `@@id([...])` PKs: refuse with the same message the macro
    // path uses, BEFORE any view is built. Every downstream view builder
    // calls `primary_key_field(model).expect(...)`, so without this the
    // command aborts with a panic — and with a message (`validated
    // schemas always have an id field`) that is false, since the parser
    // accepts such a schema happily. First, ahead of the flag checks
    // below, because it is a property of the schema rather than of any
    // flag combination. See `cratestack_core::composite_id`.
    if let Some(model) = ::cratestack_core::composite_id::find_composite_id_model(schema) {
        return Err(TypeScriptGeneratorError::CompositePrimaryKeyUnsupported(
            ::cratestack_core::composite_id::composite_id_unsupported_message(&model.name),
        ));
    }
    // The default layout is unconditional (issue #591: `--swr` used to
    // pick a *replacing* preset via `--preset swr`; it is now an additive
    // flag instead — see `crate::swr`'s module doc for the full
    // rationale). `--swr` appends the `src/swr/` subtree on top of it
    // rather than substituting for it.
    let mut files = generate_default_package(schema, config)?;
    if config.swr {
        files.extend(crate::swr::generate(schema, config)?);
    }
    Ok(GeneratedTypeScriptPackage { files })
}

/// Today's monolithic layout. Deliberately untouched by issue #304 beyond
/// destructuring `OutputPath::Fixed` (every spec here is `Fixed` — see
/// `crate::templates::OutputPath`'s doc comment): same specs, same order,
/// same context, same rendering, so this keeps producing byte-identical
/// output to before `--swr` existed — enforced by the unmodified snapshot
/// tests in `tests/snapshot.rs`. Always runs, regardless of `--swr`
/// (issue #591): the `swr` layout is additive, not a replacement.
fn generate_default_package(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
) -> Result<Vec<GeneratedTypeScriptFile>, TypeScriptGeneratorError> {
    let specs = template_specs_for(schema.transport, config.refine, config.tanstack, config.rtk)?;
    let environment = build_environment(config.template_dir.as_deref(), &specs)?;
    let context = build_template_context(schema, config)?;
    specs
        .iter()
        .map(|spec| {
            let OutputPath::Fixed(output_path) = spec.output_path else {
                unreachable!("default/REST/RPC template specs are always OutputPath::Fixed");
            };
            let template = environment
                .get_template(spec.template_name)
                .map_err(|error| {
                    TypeScriptGeneratorError::TemplateRender(spec.template_name, error)
                })?;
            let contents = template.render(&context).map_err(|error| {
                TypeScriptGeneratorError::TemplateRender(spec.template_name, error)
            })?;
            Ok(GeneratedTypeScriptFile {
                file_name: output_path.to_owned(),
                contents,
            })
        })
        .collect::<Result<Vec<_>, TypeScriptGeneratorError>>()
}
