use cratestack_core::TransportStyle;

use crate::error::TypeScriptGeneratorError;

use super::{OutputPath, TemplateSpec};

// Common templates emitted for both REST and RPC schemas.
pub(crate) const COMMON_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "package.json.j2",
        output_path: OutputPath::Fixed("package.json"),
        default_source: include_str!("../../templates/package.json.j2"),
    },
    TemplateSpec {
        template_name: "tsconfig.json.j2",
        output_path: OutputPath::Fixed("tsconfig.json"),
        default_source: include_str!("../../templates/tsconfig.json.j2"),
    },
    TemplateSpec {
        template_name: "README.md.j2",
        output_path: OutputPath::Fixed("README.md"),
        default_source: include_str!("../../templates/README.md.j2"),
    },
    TemplateSpec {
        template_name: "models.ts.j2",
        output_path: OutputPath::Fixed("src/models.ts"),
        default_source: include_str!("../../templates/src/models.ts.j2"),
    },
];

// REST-specific templates. Used when `schema.transport == Rest`.
pub(crate) const REST_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "rest-runtime.ts.j2",
        output_path: OutputPath::Fixed("src/runtime.ts"),
        default_source: include_str!("../../templates/src/rest-runtime.ts.j2"),
    },
    TemplateSpec {
        template_name: "rest-queries.ts.j2",
        output_path: OutputPath::Fixed("src/queries.ts"),
        default_source: include_str!("../../templates/src/rest-queries.ts.j2"),
    },
    TemplateSpec {
        template_name: "rest-client.ts.j2",
        output_path: OutputPath::Fixed("src/client.ts"),
        default_source: include_str!("../../templates/src/rest-client.ts.j2"),
    },
    TemplateSpec {
        template_name: "rest-index.ts.j2",
        output_path: OutputPath::Fixed("src/index.ts"),
        default_source: include_str!("../../templates/src/rest-index.ts.j2"),
    },
];

// RPC-specific templates. Used when `schema.transport == Rpc`.
pub(crate) const RPC_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "rpc-runtime.ts.j2",
        output_path: OutputPath::Fixed("src/runtime.ts"),
        default_source: include_str!("../../templates/src/rpc-runtime.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-links.ts.j2",
        output_path: OutputPath::Fixed("src/links.ts"),
        default_source: include_str!("../../templates/src/rpc-links.ts.j2"),
    },
    // Issue #277's `application/cbor-seq` boundary scanner, split across
    // two files by concern (see each file's own header comment): the
    // low-level single-item structural walk, and the stateful
    // chunk-buffering scanner + error-sentinel classification built on
    // it. Both stay under this repo's ~200-LoC convention individually;
    // a single merged file wouldn't have.
    TemplateSpec {
        template_name: "rpc-cbor-item.ts.j2",
        output_path: OutputPath::Fixed("src/cbor-item.ts"),
        default_source: include_str!("../../templates/src/rpc-cbor-item.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-cbor-seq.ts.j2",
        output_path: OutputPath::Fixed("src/cbor-seq.ts"),
        default_source: include_str!("../../templates/src/rpc-cbor-seq.ts.j2"),
    },
    // The `streamLinks` chain's terminal link (issue #277) — split out
    // of `rpc-runtime.ts.j2` to avoid growing that already-over-budget
    // file further; see its own header comment.
    TemplateSpec {
        template_name: "rpc-stream-terminal.ts.j2",
        output_path: OutputPath::Fixed("src/stream-terminal.ts"),
        default_source: include_str!("../../templates/src/rpc-stream-terminal.ts.j2"),
    },
    // Typed `model.<X>.list` query builder (issue #333) — mirrors
    // `rest-queries.ts.j2`'s position ahead of `rest-client.ts.j2` above:
    // the client template imports `toRpcListInput`/`CratestackRpcListQuery`
    // from here.
    TemplateSpec {
        template_name: "rpc-queries.ts.j2",
        output_path: OutputPath::Fixed("src/queries.ts"),
        default_source: include_str!("../../templates/src/rpc-queries.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-client.ts.j2",
        output_path: OutputPath::Fixed("src/client.ts"),
        default_source: include_str!("../../templates/src/rpc-client.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-index.ts.j2",
        output_path: OutputPath::Fixed("src/index.ts"),
        default_source: include_str!("../../templates/src/rpc-index.ts.j2"),
    },
];

// Issue #617's `--tanstack` opt-in: one extra file, appended to whichever
// mode's spec list only when `TypeScriptGeneratorConfig::tanstack` is set.
// Unlike `REFINE_TEMPLATE_SPECS` below (one template whose *content*
// branches on transport via the rendering context), TanStack Query has
// two separately hand-written source templates — `rest-react-query.ts.j2`,
// `rpc-react-query.ts.j2` — because the generated hooks call different
// runtime APIs per transport (`CratestackFetchQuery` helpers vs
// `CratestackRpcRuntime`), so `template_specs_for` below picks the
// transport-appropriate one of these rather than a single const. Both used
// to live unconditionally inside `REST_TEMPLATE_SPECS`/`RPC_TEMPLATE_SPECS`
// above — moved out here, mirroring how `REFINE_TEMPLATE_SPECS` was kept
// out of the unconditional lists, so `react-query.ts` no longer appears in
// a default run (`tests/snapshot.rs` pins that).
const REST_TANSTACK_TEMPLATE_SPECS: &[TemplateSpec] = &[TemplateSpec {
    template_name: "rest-react-query.ts.j2",
    output_path: OutputPath::Fixed("src/react-query.ts"),
    default_source: include_str!("../../templates/src/rest-react-query.ts.j2"),
}];
const RPC_TANSTACK_TEMPLATE_SPECS: &[TemplateSpec] = &[TemplateSpec {
    template_name: "rpc-react-query.ts.j2",
    output_path: OutputPath::Fixed("src/react-query.ts"),
    default_source: include_str!("../../templates/src/rpc-react-query.ts.j2"),
}];

// Issue #571's `--refine` opt-in: one extra file, appended to the REST or
// RPC spec list only when `TypeScriptGeneratorConfig::refine` is set (the
// template itself picks `ResourceMap` vs `RpcResourceMap` per transport —
// see `crate::context::build_template_context`'s `refine_resource_map_type`
// and `crate::refine`'s module doc). Kept out of
// `COMMON_TEMPLATE_SPECS`/`REST_TEMPLATE_SPECS`/`RPC_TEMPLATE_SPECS`
// deliberately — those are unconditional, and `refine.ts` must not appear
// in a default run (`tests/snapshot.rs` pins that output byte-for-byte).
pub(crate) const REFINE_TEMPLATE_SPECS: &[TemplateSpec] = &[TemplateSpec {
    template_name: "refine.ts.j2",
    output_path: OutputPath::Fixed("src/refine.ts"),
    default_source: include_str!("../../templates/src/refine.ts.j2"),
}];

/// Pick the right template specs for the schema's declared transport.
/// REST schemas get the historical fetch-based client + the
/// `CratestackFetchQuery` helpers; RPC schemas get a CratestackRpcRuntime
/// that speaks the `/rpc/{op_id}` URL space, plus their own `queries.ts`
/// (issue #333) — `CratestackRpcListQuery`/`toRpcListInput`, the RPC
/// counterpart of `CratestackFetchQuery`/`toSearchQuery`. RPC's version
/// builds a plain object for the codec-encoded POST body rather than a
/// URL query string (no URL-query shaping needed when every call is a
/// POST with a typed body), but both transports now have a real typed
/// `list` input.
///
/// `tanstack` (issue #617) appends one extra spec, transport-resolved from
/// `REST_TANSTACK_TEMPLATE_SPECS`/`RPC_TANSTACK_TEMPLATE_SPECS` above.
/// Unlike `refine`, this composes with EVERY transport — `--tanstack`
/// gates the same `src/react-query.ts` that used to be unconditional, it
/// does not add support for a transport that lacked it before.
/// `refine` (issue #571) appends one extra spec for REST or RPC schemas.
/// It is a parameter rather than folded into `mode_specs`
/// because it is additive to an otherwise unchanged run — `refine: false`
/// returns exactly the list this function returned before the flag
/// existed. `tanstack`/`rtk` (issue #906, `crate::rtk::specs`) follow the
/// same additive-parameter shape.
pub(crate) fn template_specs_for(
    transport: TransportStyle,
    refine: bool,
    tanstack: bool,
    rtk: bool,
) -> Result<Vec<TemplateSpec>, TypeScriptGeneratorError> {
    let mode_specs = match transport {
        TransportStyle::Rest => REST_TEMPLATE_SPECS,
        TransportStyle::Rpc => RPC_TEMPLATE_SPECS,
    };
    let tanstack_specs: &[TemplateSpec] = if tanstack {
        match transport {
            TransportStyle::Rest => REST_TANSTACK_TEMPLATE_SPECS,
            TransportStyle::Rpc => RPC_TANSTACK_TEMPLATE_SPECS,
        }
    } else {
        &[]
    };
    let rtk_specs = crate::rtk::specs::rtk_specs_for(transport, rtk);
    let refine_specs = if refine { REFINE_TEMPLATE_SPECS } else { &[] };
    Ok([
        COMMON_TEMPLATE_SPECS,
        mode_specs,
        tanstack_specs,
        rtk_specs,
        refine_specs,
    ]
    .concat())
}
