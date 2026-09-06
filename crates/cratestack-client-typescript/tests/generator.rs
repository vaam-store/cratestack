use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

#[test]
fn generates_fetch_client_and_tanstack_hooks_for_blog_schema() {
    let schema =
        cratestack_parser::parse_schema_file("../cratestack-pg/tests/fixtures/blog.cstack")
            .expect("fixture schema should parse");

    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "@example/blog-client".to_owned(),
            base_path: "/cstack".to_owned(),
            template_dir: None,
            swr: false,
            full_selection: false,
            refine: false,
            // Issue #617: this test is specifically about the TanStack
            // Query hooks (its name says so), which are gated behind
            // `--tanstack` now — every other test in this file uses
            // `TypeScriptGeneratorConfig::default()` (tanstack off).
            tanstack: true,
            schema_sha256: "blogschemasha256testvalue0000000000000000000000000000000000".to_owned(),
            // `blog.cstack` is REST transport, where `native_cbor` (issue
            // #746) has no effect either way — `true` here matches the
            // real `TypeScriptGeneratorConfig::default()` this test would
            // otherwise get if it used `..Default::default()`.
            native_cbor: true,
            // Issue #906: not this test's concern (its name says
            // `tanstack`), same reasoning as every other field here that
            // isn't `..Default::default()`.
            rtk: false,
        },
    )
    .expect("--tanstack template should render");

    assert_eq!(package.files.len(), 9);

    let package_json = package_file(&package, "package.json");
    let readme = package_file(&package, "README.md");
    let runtime = package_file(&package, "src/runtime.ts");
    let queries = package_file(&package, "src/queries.ts");
    let models = package_file(&package, "src/models.ts");
    let client = package_file(&package, "src/client.ts");
    let react_query = package_file(&package, "src/react-query.ts");
    let index = package_file(&package, "src/index.ts");

    assert!(package_json.contains("\"name\": \"@example/blog-client\""));
    assert!(package_json.contains("\"@tanstack/react-query\": \"^5.0.0\""));
    assert!(readme.contains("Generated CrateStack TypeScript client"));
    assert!(readme.contains("client.procedures.publishPost"));
    assert!(runtime.contains("this.basePath = options.basePath ?? \"/cstack\";"));
    assert!(runtime.contains("class CratestackRuntime"));
    assert!(runtime.contains("class CratestackHttpError"));
    assert!(queries.contains("export interface CratestackFetchQuery"));
    assert!(queries.contains("output[`includeFields[${path}]`] = fields.join(\",\");"));
    assert!(models.contains("export interface Post"));
    assert!(models.contains("title?: string;"));
    assert!(models.contains("subtitle?: string | null;"));
    assert!(models.contains("author?: User;"));
    assert!(models.contains("export interface CreatePostInput"));
    assert!(models.contains("export interface UpdatePostInput"));
    assert!(models.contains("title?: string;"));
    assert!(models.contains("export interface GetFeedArgs"));
    assert!(models.contains("limit?: number | null;"));
    assert!(client.contains("export class ExampleBlogClientClient"));
    assert!(client.contains("readonly posts: PostApi;"));
    assert!(client.contains("list(options: CratestackQueryRequestConfig = {}): Promise<Post[]>"));
    assert!(
        client.contains("list(options: CratestackQueryRequestConfig = {}): Promise<Page<Session>>")
    );
    // cratestack#498/#499 F2: every procedure call site now decodes
    // through `reviveWireFields`, keyed by the return type's own
    // `wireShapes` registry entry name (`Post` here — `blog.cstack`'s
    // `Post` has no `Decimal` field, so the registry entry is a no-op,
    // but the wrapper is unconditional, mirroring the model CRUD methods
    // right above it).
    assert!(client.contains(
        "return this.runtime.post<unknown>(\"/$procs/publishPost\", args, options)\n      .then((value) => reviveWireFields(value, 'Post') as Post);"
    ));
    assert!(react_query.contains("useQuery"));
    assert!(react_query.contains("useMutation"));
    assert!(react_query.contains("usePostListQuery"));
    assert!(react_query.contains("usePublishPostMutation"));
    assert!(index.contains("export * from \"./react-query.js\";"));
}

/// Regression test: `Page<T>`/`PageInfo` must match
/// `cratestack_core::page::{Page, PageInfo}` field-for-field — that
/// Rust struct is what every `@@paged` list route actually serializes
/// (`#[serde(rename_all = "camelCase")]`), so the generated TS types
/// are a hardcoded mirror of it, not independently designed. This
/// previously drifted silently (wrong field names, a `nextOffset`
/// field that doesn't exist on the wire, a missing `hasPreviousPage`)
/// because nothing checked it against the real shape.
#[test]
fn page_and_page_info_match_the_core_wire_shape() {
    let schema =
        cratestack_parser::parse_schema_file("../cratestack-pg/tests/fixtures/blog.cstack")
            .expect("fixture schema should parse");

    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("default template should render");
    let models = package_file(&package, "src/models.ts");

    assert!(models.contains(
        "export interface PageInfo {\n  \
         limit: number | null;\n  \
         offset: number | null;\n  \
         hasNextPage: boolean;\n  \
         hasPreviousPage: boolean;\n\
         }"
    ));
    assert!(models.contains(
        "export interface Page<T> {\n  \
         items: T[];\n  \
         totalCount: number | null;\n  \
         pageInfo: PageInfo;\n\
         }"
    ));
}

#[test]
fn page_input_procedure_argument_generates_correctly() {
    let schema = cratestack_parser::parse_schema(
        r#"
type FeedReply {
  limit Int
  offset Int
}

procedure listFeed(page: PageInput): FeedReply
"#,
    )
    .expect("PageInput fixture schema should parse");

    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("default template should render");
    let models = package_file(&package, "src/models.ts");
    let client = package_file(&package, "src/client.ts");

    assert!(models.contains(
        "export interface PageInput {\n  \
         limit: number | null;\n  \
         offset: number | null;\n\
         }"
    ));
    assert!(models.contains("export interface ListFeedArgs {\n  page: PageInput;\n}"));
    assert!(client.contains("listFeed(args: ListFeedArgs"));
}

#[test]
fn find_many_procedure_argument_generates_correctly() {
    let schema = cratestack_parser::parse_schema(
        r#"
model Post {
  id Int @id
  title String
}

procedure searchPosts(query: FindMany<Post>): Post[]
"#,
    )
    .expect("FindMany fixture schema should parse");

    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("default template should render");
    let models = package_file(&package, "src/models.ts");
    let client = package_file(&package, "src/client.ts");

    // Shared filter-operator primitives (once per package, not per model).
    assert!(models.contains("export interface EqualityFilter<V> {"));
    assert!(models.contains("export interface ComparableFilter<V> extends EqualityFilter<V> {"));
    assert!(models.contains("export interface StringFilter extends ComparableFilter<string> {"));
    assert!(models.contains("export type NumberFilter = ComparableFilter<number>;"));
    assert!(models.contains(r#"export type SortDirection = "asc" | "desc";"#));

    // Per-model `PostWhere`/`PostSortField`/`PostOrderByClause`/`PostFindMany`.
    assert!(models.contains("export type PostSortField = 'id' | 'title';"));
    assert!(models.contains(
        "export interface PostWhere {\n  \
         id?: NumberFilter;\n  \
         title?: StringFilter;\n\
         }"
    ));
    assert!(models.contains(
        "export interface PostOrderByClause {\n  \
         field: PostSortField;\n  \
         direction: SortDirection;\n\
         }"
    ));
    assert!(models.contains(
        "export interface PostFindMany {\n  \
         where?: PostWhere;\n  \
         orderBy?: PostOrderByClause[];\n\
         }"
    ));
    assert!(models.contains("export interface SearchPostsArgs {\n  query: PostFindMany;\n}"));
    assert!(client.contains("searchPosts(args: SearchPostsArgs"));
}

#[test]
fn preserves_enums_and_scalar_mappings() {
    let schema =
        cratestack_parser::parse_schema_file("../cratestack-pg/tests/fixtures/enums.cstack")
            .expect("fixture schema should parse");

    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("default template should render");
    let models = package_file(&package, "src/models.ts");
    let client = package_file(&package, "src/client.ts");

    assert!(models.contains("export type Role = 'admin' | 'member';"));
    assert!(models.contains("export const RoleValues = ["));
    assert!(models.contains("role?: Role;"));
    assert!(client.contains("resolveUser(args: ResolveUserArgs"));
}

/// Regression test for issue #137 — a `type` block field referencing a
/// `model` type. TypeScript emits every model/type/enum interface into one
/// flat `models.ts` file, so there's no module-qualification concern like
/// the Rust macro output has, but this locks the shape in regardless.
#[test]
fn type_block_field_referencing_a_model_generates_correctly() {
    let schema = cratestack_parser::parse_schema_file(
        "../cratestack-pg/tests/fixtures/type_references_model.cstack",
    )
    .expect("fixture schema should parse");

    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("default template should render");
    let models = package_file(&package, "src/models.ts");

    assert!(models.contains("export interface SomeModel"));
    assert!(models.contains("export interface ApiKeySecret"));
    assert!(models.contains("model: SomeModel;"));
}

/// Regression test for #118: `@server_only` fields must not leak into
/// the generated model, Create<X>Input, or Update<X>Input interfaces.
#[test]
fn server_only_fields_are_excluded_from_generated_interfaces() {
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/server_only_field.cstack")
        .expect("fixture schema should parse");

    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("default template should render");
    let models = package_file(&package, "src/models.ts");

    assert!(
        !models.contains("secretHash"),
        "server_only field `secretHash` leaked into models.ts:\n{models}"
    );
    assert!(models.contains("export interface Widget"));
    assert!(models.contains("export interface CreateWidgetInput"));
    assert!(models.contains("export interface UpdateWidgetInput"));
}

/// Regression test for #119: a model with no `@@allow("create", ...)`
/// must not get a generated `.create()` or an auto-derived
/// `Create<X>Input` that collides with a hand-declared `type` of the
/// same name used by a procedure.
#[test]
fn create_is_gated_on_allow_create_policy() {
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/create_policy_gate.cstack")
        .expect("fixture schema should parse");

    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("default template should render");
    let models = package_file(&package, "src/models.ts");
    let client = package_file(&package, "src/client.ts");

    // Exactly one CreateWidgetInput — the hand-declared one, matching
    // the procedure's own input shape (no `id`).
    assert_eq!(
        models.matches("export interface CreateWidgetInput").count(),
        1,
        "CreateWidgetInput should not be duplicated:\n{models}"
    );
    assert!(models.contains("export interface CreateWidgetInput {\n  name: string;\n}"));

    // No generated `.create()` on the model API — the verb fail-closes
    // by policy, so it would only ever 403.
    assert!(
        !client.contains("create(input: CreateWidgetInput"),
        "WidgetApi should not expose a dead .create() method:\n{client}"
    );

    // The real, reachable procedure keeps working, wrapping the
    // hand-declared (and now unpolluted) CreateWidgetInput inside its
    // own CreateWidgetArgs { args: CreateWidgetInput } — same pattern
    // as blog.cstack's publishPost(args: PublishPostInput).
    assert!(client.contains("createWidget(args: CreateWidgetArgs"));
    assert!(models.contains("export interface CreateWidgetArgs {\n  args: CreateWidgetInput;\n}"));
}

/// A `model` computed field (`docs/design/computed-fields.md`) is part of
/// the response interface but is never a create/update input, filter, or
/// sort key — and `get`/`list` accept a typed, per-model-gated
/// `computedParams` query parameter (`CratestackFetchQuery<TComputedParams>`,
/// gated to the generated `<Model>ComputedParams` interface — the field is
/// `@computed(params: ProxyParams?)`, a *parameterized* computed field, so
/// `Image` is gated).
#[test]
fn model_computed_field_is_response_only_and_computed_params_is_available_on_reads() {
    let schema = cratestack_parser::parse_schema(
        r#"
model Image {
  id Int @id
  storageKey String
  proxyUrl String @computed(params: ProxyParams?)

  @@allow("create", true)
}

type ProxyParams {
  width Int?
  height Int?
}
"#,
    )
    .expect("computed-field model schema should parse");

    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("default template should render");

    let models = package_file(&package, "src/models.ts");
    let queries = package_file(&package, "src/queries.ts");
    let client = package_file(&package, "src/client.ts");

    // Response interface: computed field present exactly like any other
    // field.
    let image_start = models.find("export interface Image ").unwrap();
    let image_end = models[image_start..]
        .find("\nexport interface")
        .map(|offset| image_start + offset)
        .unwrap_or(models.len());
    let image_interface = &models[image_start..image_end];
    assert!(
        image_interface.contains("proxyUrl"),
        "Image response interface must carry proxyUrl: {image_interface}"
    );

    // Create input: computed field excluded entirely.
    let create_start = models.find("export interface CreateImageInput ").unwrap();
    let create_end = models[create_start..]
        .find("\nexport interface")
        .map(|offset| create_start + offset)
        .unwrap_or(models.len());
    let create_interface = &models[create_start..create_end];
    assert!(
        create_interface.contains("storageKey"),
        "CreateImageInput must keep the ordinary field: {create_interface}"
    );
    assert!(
        !create_interface.contains("proxyUrl"),
        "CreateImageInput must never carry a computed field: {create_interface}"
    );

    // Update input: same exclusion.
    let update_start = models.find("export interface UpdateImageInput ").unwrap();
    let update_end = models[update_start..]
        .find("\nexport interface")
        .map(|offset| update_start + offset)
        .unwrap_or(models.len());
    let update_interface = &models[update_start..update_end];
    assert!(
        !update_interface.contains("proxyUrl"),
        "UpdateImageInput must never carry a computed field: {update_interface}"
    );

    // Where/sort: `ImageWhere` still exists (storageKey is filterable),
    // but never mentions proxyUrl; the sort field union never carries a
    // proxyUrl variant either.
    let where_start = models.find("export interface ImageWhere ").unwrap();
    let where_end = models[where_start..]
        .find("\nexport interface")
        .map(|offset| where_start + offset)
        .unwrap_or(models.len());
    assert!(
        !models[where_start..where_end].contains("proxyUrl"),
        "ImageWhere must never carry a computed field: {}",
        &models[where_start..where_end]
    );
    assert!(
        !models.contains("'proxyUrl'"),
        "ImageSortField union must never carry a computed field variant: {models}"
    );

    // The shared query type carries the typed computedParams field,
    // folded into the query-parameter object by `toSearchQuery`.
    assert!(
        queries.contains("export interface CratestackFetchQuery<TComputedParams = never>"),
        "CratestackFetchQuery must be generic over TComputedParams (default never): {queries}"
    );
    assert!(
        queries.contains("computedParams?: TComputedParams;"),
        "CratestackFetchQuery must carry a typed computedParams: {queries}"
    );
    assert!(
        queries.contains("output.computedParams = query.computedParams;"),
        "toSearchQuery must fold computedParams into the request's query parameters: {queries}"
    );

    // `Image` declares a *parameterized* computed field, so it's gated:
    // `models.ts` gets a generated `ImageComputedParams` interface (one
    // optional prop per parameterized computed field, typed as its
    // declared params interface, wire-keyed by field name), and
    // `client.ts`'s `list`/`get` instantiate `CratestackQueryRequestConfig`
    // with it instead of relying on the `never` default.
    assert!(
        models.contains("export interface ImageComputedParams {\n  proxyUrl?: ProxyParams;\n}"),
        "models.ts must carry the generated ImageComputedParams interface: {models}"
    );
    assert!(
        client.contains("list(options: CratestackQueryRequestConfig<ImageComputedParams> = {})"),
        "client.ts's list() must instantiate CratestackQueryRequestConfig<ImageComputedParams>: {client}"
    );
    assert!(
        client.contains(
            "get(id: number, options: CratestackQueryRequestConfig<ImageComputedParams> = {})"
        ),
        "client.ts's get() must instantiate CratestackQueryRequestConfig<ImageComputedParams>: {client}"
    );
}

fn package_file<'a>(
    package: &'a cratestack_client_typescript::GeneratedTypeScriptPackage,
    file_name: &str,
) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .unwrap_or_else(|| panic!("missing generated file {file_name}"))
        .contents
        .as_str()
}

#[test]
fn decimal_scalar_maps_to_a_real_declared_decimal_type() {
    // Historical regression (pre-cratestack#498): `ts_type()` had no
    // `Decimal` arm at all, so `Decimal` fell through to the catch-all and
    // was emitted verbatim as a TS type name that nothing declares —
    // generation still succeeded, and the breakage only surfaced at `tsc`
    // with `TS2304: Cannot find name 'Decimal'`, which a generation-only
    // assertion wouldn't catch.
    //
    // cratestack#498 replaced the interim `string` mapping (which merely
    // fixed the `tsc` failure without giving the SDK a decimal type) with
    // a real one: `Decimal` is now `models.ts`'s own exported
    // `decimal.js`-backed class (`DecimalJs.clone({...})`, see
    // `models.ts.j2`'s doc comment), not the bare wire-format string. This
    // is the "give the SDKs a real decimal type" approach, not "canonicalize
    // the wire" — the maintainer's recorded decision on cratestack#498.
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/decimal_scalar.cstack")
        .expect("fixture schema should parse");

    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("default template should render");
    let models = package_file(&package, "src/models.ts");

    assert!(
        models.contains("amountXaf?: Decimal;"),
        "a required Decimal field must be typed `Decimal`, got:\n{models}"
    );
    assert!(
        models.contains("discountXaf?: Decimal | null;"),
        "an optional Decimal field must be typed `Decimal | null`, got:\n{models}"
    );
    assert!(
        models.contains("export const Decimal = DecimalJs.clone("),
        "models.ts must export a real, plain-notation-configured `Decimal` value, got:\n{models}"
    );
    assert!(
        models.contains("export type Decimal = DecimalJs;"),
        "models.ts must export the `Decimal` instance type, got:\n{models}"
    );
    assert!(
        models.contains("export function reviveWireFields("),
        "models.ts must export the decode-side revival helper, got:\n{models}"
    );
    assert!(
        models.contains("export type DecimalFilter = ComparableFilter<Decimal>;"),
        "DecimalFilter's comparison operands must be typed `Decimal`, got:\n{models}"
    );

    let client = package_file(&package, "src/client.ts");
    assert!(
        client.contains("reviveWireFields(value, 'Invoice')"),
        "the REST client's Invoice CRUD methods must revive via Invoice's own \
         wireShapes entry on decode, got:\n{client}"
    );

    let package_json = package_file(&package, "package.json");
    assert!(
        package_json.contains("\"decimal.js\""),
        "package.json must declare the decimal.js runtime dependency, got:\n{package_json}"
    );
}

#[test]
fn decimal_scalar_revives_on_decode_over_rpc_transport_too() {
    // Requirement #6 (cratestack#498): both REST and RPC transports.
    // `decimal_scalar_maps_to_a_real_declared_decimal_type` proves the
    // REST-transport `rest-client.ts.j2`; this proves the RPC-transport
    // `rpc-client.ts.j2` gets the identical `reviveWireFields` wiring.
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/decimal_scalar_rpc.cstack")
        .expect("fixture schema should parse");

    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("default template should render");
    let client = package_file(&package, "src/client.ts");

    for op in ["model.Invoice.list", "model.Invoice.get"] {
        assert!(
            client.contains(op),
            "expected RPC op `{op}` to still be generated, got:\n{client}"
        );
    }
    assert!(
        client.contains("reviveWireFields(value, 'Invoice')"),
        "the RPC client's Invoice CRUD methods must revive via Invoice's own \
         wireShapes entry on decode, got:\n{client}"
    );
}
