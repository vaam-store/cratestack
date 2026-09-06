use std::ops::Range;
use std::sync::Arc;

use ariadne::{Color, Label, Report, ReportKind, Source};
use cratestack_core::SourceSpan;

/// A schema error, identified by which file it came from (cratestack#916).
///
/// `file`/`source_text` start out empty: every constructor below (`new`,
/// `span_error`) fires deep inside parsing/validation, dozens of call sites
/// that have no path in scope and share one property — every error a single
/// parse produces always belongs to the one file that parse was given.
/// Rather than thread a path through every one of those call sites for no
/// behavioral gain, [`SchemaError::with_file`] is applied exactly once, at
/// the boundary where a path *is* known (`parse_schema_named`,
/// `parse_schema_diagnostics`, `parse_schema_file` in `lib.rs`) — so a
/// `SchemaError` a caller can actually observe always carries real file
/// identity, even though the type technically allows an empty one internally.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct SchemaError {
    message: String,
    span: Range<usize>,
    line: usize,
    file: Arc<str>,
    source_text: Arc<str>,
}

impl SchemaError {
    pub(crate) fn new(message: impl Into<String>, span: Range<usize>, line: usize) -> Self {
        Self {
            message: message.into(),
            span,
            line,
            file: Arc::from(""),
            source_text: Arc::from(""),
        }
    }

    /// Attach this error's file identity and that file's source text.
    ///
    /// `source` is an `Arc<str>` the caller already holds (not a fresh
    /// `String`) so tagging every error collected from one parse — several,
    /// with [`crate::parse_schema_diagnostics`] — is a refcount bump each,
    /// not a copy of the whole schema per error.
    pub(crate) fn with_file(mut self, file: &Arc<str>, source: &Arc<str>) -> Self {
        self.file = Arc::clone(file);
        self.source_text = Arc::clone(source);
        self
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    pub fn line(&self) -> usize {
        self.line
    }

    /// The file this error belongs to — empty only for an error that was
    /// never passed through [`Self::with_file`] (not reachable through any
    /// public constructor; see the type's doc comment).
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Render this error as a human-readable diagnostic.
    ///
    /// Takes no arguments: the error already knows both its file (`self.file`)
    /// and that file's source (`self.source_text`), attached once at the
    /// parse/diagnostics boundary. Before cratestack#916 this took a `(path,
    /// source)` pair supplied by the caller, which had to happen to match the
    /// file the error actually came from — nothing enforced that once more
    /// than one file was involved.
    pub fn render(&self) -> String {
        let mut output = Vec::new();
        let file = self.file.to_string();
        Report::build(ReportKind::Error, (file.clone(), self.span.clone()))
            .with_message(&self.message)
            .with_label(
                Label::new((file.clone(), self.span.clone()))
                    .with_message(&self.message)
                    .with_color(Color::Red),
            )
            .finish()
            .write((file, Source::from(self.source_text.as_ref())), &mut output)
            .expect("diagnostic rendering should succeed");

        String::from_utf8(output).expect("ariadne should emit utf-8")
    }
}

pub(crate) fn span_error(message: impl Into<String>, span: SourceSpan) -> SchemaError {
    SchemaError::new(message, span.start..span.end, span.line)
}
