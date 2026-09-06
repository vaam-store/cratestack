//! The decisive proof issue #906's acceptance criteria ask for: **a test
//! that proves a mutation's derived `invalidatesTags` match the models it
//! actually touches** — not a formality restating what the code already
//! claims. `crate::rtk::touch::touched_model_names` has its own unit
//! coverage (`src/rtk/touch.rs`'s `#[cfg(test)]`), but this file proves the
//! SAME property end-to-end through the real template-rendering pipeline:
//! generate `src/rtk-api.ts` for real, find the actual rendered
//! `invalidatesTags`/`providesTags` array for a real procedure, and assert
//! it names exactly the models that procedure's own `args`/return type
//! reference — no more (a bystander model never mentioned stays out), no
//! fewer (both an argument-position and a return-position model are
//! found).
//!
//! Both transports: `templates/src/rtk-rest.ts.j2` and `rtk-rpc.ts.j2` are
//! separate hand-written templates (per the transport-parity rule), so a
//! REST-only proof would leave the identical derivation unproven on RPC.

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

/// Three models (`Widget`, `Order`, `Ledger`), and ONE mutation procedure
/// that references exactly two of them: `Order` as an argument type,
/// `Widget` as its return type. `Ledger` is never mentioned anywhere in
/// the procedure's own signature — the bystander this test proves stays
/// OUT of the derived tags.
const SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth Operator {
  id Int
}

model Widget {
  id Int @id
  name String
  @@allow("read", auth() != null)
}

model Order {
  id Int @id
  widgetId Int
  @@allow("read", auth() != null)
}

model Ledger {
  id Int @id
  label String
  @@allow("read", auth() != null)
}

mutation procedure archiveWidgetOrder(order: Order): Widget
  @allow(auth() != null)

procedure summarizeLedger(): Ledger
  @allow(auth() != null)
"#;

#[test]
fn rest_mutation_invalidates_tags_exactly_matches_the_models_it_touches() {
    assert_tags_match_touched_models(SCHEMA, false);
}

#[test]
fn rpc_mutation_invalidates_tags_exactly_matches_the_models_it_touches() {
    let rpc_schema = SCHEMA.replace("datasource db {", "transport rpc\n\ndatasource db {");
    assert_tags_match_touched_models(&rpc_schema, true);
}

fn assert_tags_match_touched_models(schema_source: &str, is_rpc: bool) {
    let package = generate(schema_source);
    let rtk_api = file(&package, "src/rtk-api.ts");

    let mutation_block = endpoint_block(rtk_api, "archiveWidgetOrder");
    assert!(
        mutation_block.contains("invalidatesTags: ["),
        "{}: archiveWidgetOrder's endpoint should carry a literal invalidatesTags array:\n{mutation_block}",
        transport_label(is_rpc)
    );
    // Exactly Order and Widget — the two models this procedure's own
    // args/return type actually reference.
    assert!(
        mutation_block.contains(r#"{ type: "Order" as const }"#),
        "{}: archiveWidgetOrder touches Order (its own arg type) — must invalidate it:\n{mutation_block}",
        transport_label(is_rpc)
    );
    assert!(
        mutation_block.contains(r#"{ type: "Widget" as const }"#),
        "{}: archiveWidgetOrder touches Widget (its own return type) — must invalidate it:\n{mutation_block}",
        transport_label(is_rpc)
    );
    // The decisive negative: Ledger is never mentioned in this
    // procedure's signature, so it must not appear in ITS tags block —
    // checked against the SLICED block, not the whole file (which
    // legitimately mentions "Ledger" elsewhere, e.g. in `tagTypes` and in
    // `summarizeLedger`'s own tags).
    assert!(
        !mutation_block.contains("Ledger"),
        "{}: archiveWidgetOrder never references Ledger — it must not invalidate it just \
         because Ledger exists elsewhere in the schema:\n{mutation_block}",
        transport_label(is_rpc)
    );

    // The query-procedure half of the same proof, `providesTags` instead
    // of `invalidatesTags`: `summarizeLedger` touches only `Ledger`.
    let query_block = endpoint_block(rtk_api, "summarizeLedger");
    assert!(
        query_block.contains("providesTags: ["),
        "{}: summarizeLedger's endpoint should carry a literal providesTags array:\n{query_block}",
        transport_label(is_rpc)
    );
    assert!(
        query_block.contains(r#"{ type: "Ledger" as const, id: "LIST" }"#),
        "{}: summarizeLedger touches Ledger (its own return type) — must provide it:\n{query_block}",
        transport_label(is_rpc)
    );
    assert!(
        !query_block.contains("\"Widget\"") && !query_block.contains("\"Order\""),
        "{}: summarizeLedger never references Widget or Order:\n{query_block}",
        transport_label(is_rpc)
    );
}

fn transport_label(is_rpc: bool) -> &'static str {
    if is_rpc { "RPC" } else { "REST" }
}

/// Slices out exactly one endpoint's own object literal — from its own
/// `{{ key }}: builder.` line up to (and including) the matching closing
/// `}),` at the SAME indentation the endpoints are all rendered at (six
/// spaces, per both templates). Brace-counting would also work but this
/// is enough for these templates' fixed, known indentation and keeps the
/// helper simple.
#[track_caller]
fn endpoint_block<'a>(rtk_api: &'a str, endpoint_key: &str) -> &'a str {
    let start_marker = format!("      {endpoint_key}: builder.");
    let start = rtk_api.find(&start_marker).unwrap_or_else(|| {
        panic!("expected an endpoint named {endpoint_key:?} in src/rtk-api.ts:\n{rtk_api}")
    });
    let end_marker = "\n      }),";
    let end = rtk_api[start..]
        .find(end_marker)
        .unwrap_or_else(|| panic!("expected a closing `}}),` after {endpoint_key:?}'s endpoint"));
    &rtk_api[start..start + end + end_marker.len()]
}

fn generate(source: &str) -> GeneratedTypeScriptPackage {
    let schema = cratestack_parser::parse_schema(source).expect("fixture schema should parse");
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "rtk-tag-derivation-client".to_owned(),
            rtk: true,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("--rtk should render for this fixture")
}

fn file<'a>(package: &'a GeneratedTypeScriptPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .map(|file| file.contents.as_str())
        .unwrap_or_else(|| panic!("generated package should contain {name}"))
}

/// A mutation procedure must NOT emit the specific `id: "LIST"` tag.
///
/// RTK Query matches a specific tag only against providers of that exact
/// pair, so `{ type, id: "LIST" }` refreshes `listOrder` while leaving every
/// `getOrder(id)` entry stale — a list and a detail view disagreeing, which
/// is among the hardest stale-UI bugs to trace to its cause. The generated
/// code shipped exactly this shape, and this test asserted it, until #906's
/// review caught it. This guard is what stops it coming back.
#[test]
fn mutation_procedures_use_the_general_tag_not_the_specific_list_tag() {
    for is_rpc in [false, true] {
        let source = if is_rpc {
            SCHEMA.replace("datasource db {", "transport rpc\n\ndatasource db {")
        } else {
            SCHEMA.to_owned()
        };
        let package = generate(&source);
        let rtk_api = file(&package, "src/rtk-api.ts");
        let mutation_block = endpoint_block(rtk_api, "archiveWidgetOrder");
        // Slice the tags ARRAY, not the whole endpoint. The surrounding
        // comment deliberately quotes `id: "LIST"` to explain why it is
        // wrong, and a naive block-wide search matches that prose instead
        // of the code — which is exactly how this assertion first failed.
        let tags_array = tags_array(mutation_block, "invalidatesTags");

        assert!(
            !tags_array.contains(r#"id: "LIST""#),
            "{}: a mutation procedure must invalidate the general tag so \
             `get<Model>(id)` entries are reached; the specific LIST tag \
             matches only LIST providers:\n{tags_array}",
            transport_label(is_rpc)
        );
    }
}

/// Slice the literal array that follows `<name>: [`, up to its closing `],`.
///
/// Needed because the generated tags block is preceded by a comment that
/// quotes the very shape under test.
fn tags_array<'a>(endpoint: &'a str, name: &str) -> &'a str {
    let marker = format!("{name}: [");
    let start = endpoint
        .find(&marker)
        .unwrap_or_else(|| panic!("expected `{marker}` in endpoint:\n{endpoint}"));
    let rest = &endpoint[start..];
    let end = rest
        .find("],")
        .unwrap_or_else(|| panic!("expected a closing `],` after `{marker}`"));
    &rest[..end + 2]
}
