//! "Is this schema-authored name one of the words the multi-file-schemas
//! feature (epic #910) will claim as command words" check, shared by every
//! ident site a `.cstack` schema feeds into codegen: field names
//! (models/mixins/types/auth/views — see
//! [`super::fields::validate_field_reserved_identifier`]),
//! model/mixin/type/view/procedure names, enum names and variants, and
//! procedure argument names.
//!
//! `part` and `import` are ordinary Rust identifiers — they parse fine as
//! names today, and the existing Rust-keyword rule
//! ([`super::reserved_idents::validate_reserved_identifier`]) rightly lets
//! them through. Ticket #922 reserves them NOW so nobody writes a schema
//! that will break once `part "file.cstack"` / `import "file.cstack"`
//! land as real declarations (#918/#920). The check is exact-match only:
//! `partition`, `important`, `imports` and `importer` all keep parsing.

use cratestack_core::SourceSpan;

use crate::diagnostics::{SchemaError, span_error};
use crate::validate::reserved_idents::validate_reserved_identifier;

/// The words the multi-file-schemas feature (epic #910) will use as
/// command words. Reserved as identifiers in every `.cstack` ident site so
/// no existing schema collides with the future grammar.
const RESERVED_KEYWORDS: [&str; 2] = ["part", "import"];

/// Reject `name` if it is a word reserved for multi-file schemas.
/// `subject` should read naturally as "$subject is reserved for
/// multi-file schemas (`part`/`import`) and cannot be used as an
/// identifier" — e.g. `` model `part` ``, `` field `import` on
/// model `Foo` ``, `` procedure argument `part` on procedure `create` ``.
pub(super) fn validate_reserved_keyword(
    name: &str,
    span: SourceSpan,
    subject: &str,
) -> Result<(), SchemaError> {
    if !RESERVED_KEYWORDS.contains(&name) {
        return Ok(());
    }
    Err(span_error(
        format!(
            "{subject} is reserved for multi-file schemas (`part`/`import`) and cannot be used as \
             an identifier. Rename it.",
        ),
        span,
    ))
}

/// Run both reservation checks for one ident site: the Rust-identifier
/// check ([`validate_reserved_identifier`], `reserved_idents.rs`) and the
/// multi-file-schema keyword check ([`validate_reserved_keyword`]). Called
/// once per ident site so a name that is neither a reserved Rust keyword
/// nor `part`/`import` parses cleanly, and so each site adds only one call
/// (which keeps every source file under the 200-LoC ceiling).
pub(super) fn validate_reserved_ident_site(
    name: &str,
    span: SourceSpan,
    subject: &str,
) -> Result<(), SchemaError> {
    validate_reserved_identifier(name, span, subject)?;
    validate_reserved_keyword(name, span, subject)
}
