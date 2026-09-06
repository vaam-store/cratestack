mod builders;
mod builders_model;
mod computed_params_view;
mod config;
mod context;
mod dart_types;
mod data_class_view;
mod enum_filter_view;
mod field_view;
mod find_many_order;
mod find_many_views;
mod generator;
mod idents;
mod naming;
mod package_floors;
mod patch_touch;
mod release_line;
mod riverpod;
mod templates;
mod templates_fragments;
mod views;
mod wire_decode;
mod wire_encode;

pub use config::{
    DEFAULT_NATIVE_CBOR, DartGeneratorConfig, DartGeneratorError, DartPreset, GeneratedDartFile,
    GeneratedDartPackage,
};
pub use generator::generate_package;
