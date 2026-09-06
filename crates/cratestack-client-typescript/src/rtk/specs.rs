//! `--rtk`'s (issue #906) template specs — split out of
//! `crate::templates::specs` purely to keep that file under this repo's
//! ~200-LoC convention; the same additive-one-extra-spec shape
//! `--tanstack` established there for `REST_TANSTACK_TEMPLATE_SPECS`/
//! `RPC_TANSTACK_TEMPLATE_SPECS`, just relocated and folded behind one
//! function so the caller is a single line.

use cratestack_core::TransportStyle;

use crate::templates::{OutputPath, TemplateSpec};

const REST_RTK_TEMPLATE_SPECS: &[TemplateSpec] = &[TemplateSpec {
    template_name: "rtk-rest.ts.j2",
    output_path: OutputPath::Fixed("src/rtk-api.ts"),
    default_source: include_str!("../../templates/src/rtk-rest.ts.j2"),
}];
const RPC_RTK_TEMPLATE_SPECS: &[TemplateSpec] = &[TemplateSpec {
    template_name: "rtk-rpc.ts.j2",
    output_path: OutputPath::Fixed("src/rtk-api.ts"),
    default_source: include_str!("../../templates/src/rtk-rpc.ts.j2"),
}];

/// `crate::templates::specs::template_specs_for`'s `--rtk` contribution:
/// empty unless `rtk`, then transport-resolved. REST and RPC dispatch
/// through genuinely different mechanisms (`crate::rtk`'s module doc), so
/// this is two hand-written source templates rather than one spec whose
/// content branches on transport.
pub(crate) fn rtk_specs_for(transport: TransportStyle, rtk: bool) -> &'static [TemplateSpec] {
    if !rtk {
        return &[];
    }
    match transport {
        TransportStyle::Rest => REST_RTK_TEMPLATE_SPECS,
        TransportStyle::Rpc => RPC_RTK_TEMPLATE_SPECS,
    }
}
