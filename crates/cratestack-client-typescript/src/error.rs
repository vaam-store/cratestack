/// Split out of `templates.rs` (issue #304) to keep that file under this
/// repo's ~200-LoC convention as it grew a fan-out mechanism for the `swr`
/// preset — this enum is pure error data with no rendering logic, so it
/// moves cleanly on its own.
#[derive(Debug, thiserror::Error)]
pub enum TypeScriptGeneratorError {
    #[error("failed to read template '{template_name}' from {path}: {source}")]
    TemplateRead {
        path: String,
        template_name: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to register template '{0}': {1}")]
    TemplateRegistration(&'static str, #[source] minijinja::Error),
    #[error("failed to render template '{0}': {1}")]
    TemplateRender(&'static str, #[source] minijinja::Error),
    /// Issue #344: `--swr`'s per-model file name
    /// (`src/swr/models/{{ file_stem }}.ts`) is derived from
    /// `crate::naming::to_kebab_case`, which — like `to_camel_case`/
    /// `to_pascal_case`/`to_snake_case` — tokenizes through the same
    /// lossy `split_words` (splits on `_`/`-`/` ` *and* case boundaries).
    /// Two distinct, parser-valid model names (e.g. `UserGroup` and
    /// `User_Group`) can collapse to the same word sequence and therefore
    /// the same file path. Decision spike #317 ruled out a single
    /// parser-level check (each collision-prone call site normalizes
    /// differently, so no shared check can cover all of them); this call
    /// site fails loudly rather than disambiguating (contrast
    /// `crate::views::disambiguate_model_api_keys`, which suffixes a
    /// colliding *display* key) because a clobbered generated file is
    /// silent data loss a schema author has no way to notice short of
    /// diffing generator output on disk.
    #[error(
        "--swr: models `{first}` and `{second}` both normalize to the file name \
         `src/swr/models/{file_stem}.ts` — rename one of them so their kebab-case forms differ"
    )]
    SwrModelFileNameCollision {
        first: String,
        second: String,
        file_stem: String,
    },
    /// Issue #777: `--swr` exports a model's five CRUD operations as plain
    /// free functions (`list{Models}`/`get{Model}`/…, `crate::naming::
    /// model_fn_names`) and a procedure as `to_camel_case(&procedure.name)`
    /// (`crate::procedure_views::build_procedure`), then barrel-`export *`s
    /// `./models/<model>.js` *and* `./procedures.js` from
    /// `src/swr/index.ts`. When the two derive the same identifier the
    /// generated package does not compile — `tsc` reports TS2308 on the
    /// barrel — so this fails generation instead, the way
    /// [`Self::SwrModelFileNameCollision`] already does for the analogous
    /// #344 file-name case. The default (non-`--swr`) layout is structurally
    /// immune: its model operations are methods on per-model client classes,
    /// with no top-level binding for a procedure to collide with.
    ///
    /// Naming the *procedure* as the thing to rename is a hint, not a rule:
    /// renaming the model works equally well. Neither is picked for the
    /// schema author, since a silently disambiguated public function name is
    /// exactly what the #344 precedent refuses to do.
    #[error(
        "--swr: procedure `{procedure}` and model `{model}`'s generated `{operation}` function \
         are both exported as `{identifier}` from `src/swr/index.ts` (TypeScript TS2308) — \
         rename one of them so their camelCase forms differ"
    )]
    SwrProcedureNameCollision {
        procedure: String,
        identifier: String,
        model: String,
        operation: &'static str,
    },
    /// cratestack#802: the `--tanstack` analogue of
    /// [`Self::SwrProcedureNameCollision`], and a sharper failure than it.
    /// `--swr` splits model and procedure functions across files that a
    /// barrel `export *`s, so its collision is TS2308 at the barrel;
    /// `--tanstack` emits both hook families into the same
    /// `src/react-query.ts`, so this is a same-file duplicate declaration
    /// that no `export *` de-duplication can mask. See
    /// `crate::tanstack_collisions`.
    ///
    /// The codes are TS2393 + TS2323, measured by running `tsc` on a
    /// generated package from `tests/fixtures/
    /// tanstack_mutation_hook_collision.cstack` with the check disabled.
    /// cratestack#802 predicted TS2300; that is the *other* duplicate-
    /// identifier code and is not what this path actually emits. The
    /// message names the real ones so a user who greps their build output
    /// finds this error.
    #[error(
        "--tanstack: procedure `{procedure}` and model `{model}`'s generated `{operation}` hook \
         are both declared as `{identifier}` in `src/react-query.ts` (TypeScript TS2393 \
         duplicate function implementation, plus TS2323 cannot redeclare exported variable) — \
         rename one of them so their PascalCase forms differ"
    )]
    TanstackHookNameCollision {
        procedure: String,
        identifier: String,
        model: String,
        operation: &'static str,
    },
    /// cratestack#906: the `--rtk` analogue of
    /// [`Self::TanstackHookNameCollision`]. RTK Query's endpoint map
    /// (`src/rtk-api.ts`'s `createApi({ endpoints: (builder) => ({ ... }) })`)
    /// is a single object literal, so a colliding key is a duplicate
    /// property name (`ts(1117)`) — see `crate::rtk::collisions`'s module
    /// doc for the full reasoning and why a model-vs-model collision is
    /// structurally impossible under `crate::rtk::naming`'s scheme,
    /// leaving only this procedure-vs-model case reachable.
    #[error(
        "--rtk: procedure `{procedure}` and model `{model}`'s generated `{operation}` endpoint \
         are both declared as `{identifier}` in `src/rtk-api.ts`'s `createApi({{ endpoints }})` \
         object (TypeScript ts(1117): an object literal cannot have multiple properties with \
         the same name) — rename one of them so their camelCase forms differ"
    )]
    RtkEndpointNameCollision {
        procedure: String,
        identifier: String,
        model: String,
        operation: &'static str,
    },
    /// The schema declares a composite primary key (`@@id([...])`) on at
    /// least one model. `include_*_schema!` has rejected these since the
    /// gap was found (see `cratestack_core::composite_id`), but this
    /// generator had no equivalent guard and instead panicked inside
    /// `views.rs`'s `primary_key_field(model).expect(...)` — a panic
    /// rather than an error, carrying a message (`validated schemas
    /// always have an id field`) that is simply false: the parser accepts
    /// such a schema. Same rejection, same wording, as the macro path.
    #[error("{0}")]
    CompositePrimaryKeyUnsupported(String),
}
