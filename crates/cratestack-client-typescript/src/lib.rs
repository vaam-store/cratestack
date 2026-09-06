mod computed_params;
mod config;
mod context;
mod error;
mod find_many_views;
mod generator;
mod naming;
mod package_deps;
mod package_floors;
mod procedure_views;
mod refine;
mod release_line;
mod rtk;
mod swr;
mod tanstack_collisions;
mod templates;
mod types;
mod views;
mod wire_shapes;

pub use config::{
    DEFAULT_NATIVE_CBOR, DEFAULT_RTK, DEFAULT_TANSTACK, GeneratedTypeScriptFile,
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig,
};
pub use error::TypeScriptGeneratorError;
pub use generator::generate_package;
