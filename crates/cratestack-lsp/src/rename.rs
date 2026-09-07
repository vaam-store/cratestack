//! `textDocument/rename` and `textDocument/prepareRename`.
//!
//! Rename reuses the reference index rather than growing a second notion of
//! "everywhere this symbol appears" — a rename that disagreed with Shift+F12
//! would be a rename that misses call sites.
//!
//! It is held to a stricter standard than navigation, though. A go-to-definition
//! that lands a line off is an annoyance; a rename computed against the wrong
//! byte offsets rewrites the wrong text and is very hard to notice. Everything
//! below that looks like paranoia is there for that reason.

use cratestack_core::{Schema, SourceSpan};
use tower_lsp_server::ls_types::{Position, Range, TextEdit, Uri, WorkspaceEdit};

use crate::references::{declaration_span_of, reference_spans};
use crate::rename_error::RenameError;
use crate::state::DocumentState;
use crate::symbol_target::{SymbolTarget, symbol_target_at};
use crate::text::{position_to_offset, range_from_offsets, span_contains};

/// Words that change how a file parses, rather than just what something is
/// called. Kept alongside the parser's own builtin list, which is queried
/// directly rather than copied so it cannot drift.
const KEYWORDS: &[&str] = &[
    "datasource",
    "auth",
    "mcp",
    "extension",
    "transport",
    "mixin",
    "model",
    "type",
    "enum",
    "view",
    "from",
    "procedure",
    "mutation",
    // cratestack#867. Belongs here for the same reason `view` does: a
    // line starting `query ` parses as a different construct, so offering
    // to rename the keyword itself would silently change what the file
    // means rather than what something is called.
    "query",
];

/// The range of the identifier under the cursor, if it can be renamed.
///
/// Returning `None` is what stops an editor offering a rename box over a
/// keyword, a builtin type, or a comment.
pub(crate) fn prepare_rename(document: &DocumentState, position: Position) -> Option<Range> {
    if document.is_stale() {
        return None;
    }
    let (text, schema) = document.resolved()?;
    let offset = position_to_offset(text, position)?;
    let (_, span) = renameable_at(text, schema, offset)?;
    Some(range_from_offsets(text, span.start, span.end))
}

/// Every range to rewrite, in file order.
pub(crate) fn rename_ranges(
    document: &DocumentState,
    position: Position,
    new_name: &str,
) -> Result<Vec<Range>, RenameError> {
    // Checked before anything else: with a stale schema every span below is
    // measured against text the buffer no longer holds.
    if document.is_stale() {
        return Err(RenameError::StaleSchema);
    }
    let (text, schema) = document.resolved().ok_or(RenameError::NotRenameable)?;

    validate_new_name(new_name)?;

    let offset = position_to_offset(text, position).ok_or(RenameError::NotRenameable)?;
    let (target, _) = renameable_at(text, schema, offset).ok_or(RenameError::NotRenameable)?;

    if conflicts(schema, &target, new_name) {
        return Err(RenameError::Conflict(new_name.to_owned()));
    }

    Ok(reference_spans(text, schema, &target)
        .into_iter()
        .map(|span| range_from_offsets(text, span.start, span.end))
        .collect())
}

/// The symbol under `offset` plus the exact occurrence the cursor sits in.
///
/// Resolving through `reference_spans` rather than trusting `symbol_target_at`
/// alone means a rename can only start from a position the reference index
/// actually collected — so the range the editor highlights is guaranteed to be
/// one of the ranges the rename will rewrite.
fn renameable_at(text: &str, schema: &Schema, offset: usize) -> Option<(SymbolTarget, SourceSpan)> {
    let target = symbol_target_at(text, schema, offset)?;
    // No declaration site means nothing in this schema owns the name.
    declaration_span_of(schema, &target)?;
    let span = reference_spans(text, schema, &target)
        .into_iter()
        .find(|span| span_contains(*span, offset))?;
    Some((target, span))
}

fn validate_new_name(new_name: &str) -> Result<(), RenameError> {
    let mut chars = new_name.chars();
    let valid = match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {
            chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        }
        _ => false,
    };
    if !valid {
        return Err(RenameError::InvalidIdentifier(new_name.to_owned()));
    }
    if KEYWORDS.contains(&new_name)
        || cratestack_parser::builtin_type_names().contains(&new_name)
        || cratestack_parser::reserved_multi_file_keywords().contains(&new_name)
    {
        return Err(RenameError::Reserved(new_name.to_owned()));
    }
    Ok(())
}

/// Whether `new_name` is already used in the scope this symbol lives in — the
/// schema for a declaration, the owning declaration for a field or variant.
/// Probing with a synthetic target reuses the same lookup the rename itself
/// uses, so the two cannot disagree about what "already declared" means.
fn conflicts(schema: &Schema, target: &SymbolTarget, new_name: &str) -> bool {
    let probe = match target {
        SymbolTarget::Declaration(_) => SymbolTarget::Declaration(new_name.to_owned()),
        SymbolTarget::Field { owner, .. } => SymbolTarget::Field {
            owner: owner.clone(),
            field: new_name.to_owned(),
        },
    };
    declaration_span_of(schema, &probe).is_some()
}

/// The full edit for a rename request, ready to return from the handler.
pub(crate) fn workspace_edit(
    uri: Uri,
    document: &DocumentState,
    position: Position,
    new_name: &str,
) -> Result<WorkspaceEdit, RenameError> {
    let edits = rename_ranges(document, position, new_name)?
        .into_iter()
        .map(|range| TextEdit {
            range,
            new_text: new_name.to_owned(),
        })
        .collect::<Vec<_>>();

    Ok(WorkspaceEdit {
        changes: Some([(uri, edits)].into_iter().collect()),
        document_changes: None,
        change_annotations: None,
    })
}
