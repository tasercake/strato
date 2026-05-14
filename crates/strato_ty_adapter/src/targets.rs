//! Strato-owned target and callable identity types.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ruff_text_size::TextRange;

/// Stable Strato-owned file identifier used by facade callers.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct FileId(usize);

impl FileId {
    /// Creates a file identifier from a deterministic facade-local ordinal.
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Returns the deterministic facade-local ordinal.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Python file metadata exposed by the adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileInfo {
    id: FileId,
    path: PathBuf,
    is_stub: bool,
}

impl FileInfo {
    /// Creates file metadata.
    #[must_use]
    pub fn new(id: FileId, path: PathBuf, is_stub: bool) -> Self {
        Self { id, path, is_stub }
    }

    /// Returns the facade-local file identifier.
    #[must_use]
    pub const fn id(&self) -> FileId {
        self.id
    }

    /// Returns the normalized system path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns whether the file is a stub file.
    #[must_use]
    pub const fn is_stub(&self) -> bool {
        self.is_stub
    }
}

/// Stable key for a resolved Python definition.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DefinitionKey(String);

impl DefinitionKey {
    /// Creates a definition key from a normalized identifier.
    #[must_use]
    pub fn new(identifier: impl Into<String>) -> Self {
        Self(identifier.into())
    }

    /// Returns the normalized identifier for this definition.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Resolved target for a call expression or callable reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedTarget {
    /// A first-party definition inside one of the facade project files.
    FirstPartyDefinition(DefinitionKey),
    /// One or more external qualified aliases surfaced by ty.
    ExternalQualifiedNames(BTreeSet<String>),
    /// ty could not resolve this fact through the exposed APIs.
    Unknown,
}

impl ResolvedTarget {
    /// Creates a resolved target for a definition key.
    #[must_use]
    pub const fn new(definition: DefinitionKey) -> Self {
        Self::FirstPartyDefinition(definition)
    }

    /// Returns the first-party definition key backing this target, if present.
    #[must_use]
    pub const fn definition(&self) -> Option<&DefinitionKey> {
        match self {
            Self::FirstPartyDefinition(definition) => Some(definition),
            Self::ExternalQualifiedNames(_) | Self::Unknown => None,
        }
    }

    /// Returns true if this target could not be resolved.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

/// Normalized callable metadata exposed by the adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallableInfo {
    definition: DefinitionKey,
    name: String,
    range: TextRange,
}

impl CallableInfo {
    /// Creates callable metadata for a definition key.
    #[must_use]
    pub fn new(definition: DefinitionKey, name: impl Into<String>, range: TextRange) -> Self {
        Self {
            definition,
            name: name.into(),
            range,
        }
    }

    /// Returns the definition key for this callable.
    #[must_use]
    pub const fn definition(&self) -> &DefinitionKey {
        &self.definition
    }

    /// Returns the callable's syntactic name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the callable source range.
    #[must_use]
    pub const fn range(&self) -> TextRange {
        self.range
    }
}

/// Dunder operation query supported by the facade.
#[derive(Clone, Copy, Debug)]
pub enum DunderOperation<'a> {
    /// Binary operator expression.
    Binary(&'a ruff_python_ast::ExprBinOp),
    /// Unary operator expression.
    Unary(&'a ruff_python_ast::ExprUnaryOp),
}

/// Source location represented without exposing parser internals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceLocation {
    /// Zero-based byte offset where this fact starts.
    pub start: u32,
    /// Zero-based byte offset where this fact ends.
    pub end: u32,
}

impl SourceLocation {
    /// Creates a location from a parser text range.
    #[must_use]
    pub fn from_range(range: TextRange) -> Self {
        Self {
            start: range.start().into(),
            end: range.end().into(),
        }
    }
}

/// Syntax facts extracted for one file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterFileSyntax {
    /// File identifier in the facade.
    pub file: FileId,
    /// Normalized file path.
    pub path: PathBuf,
    /// True when this syntax came from a stub file.
    pub is_stub: bool,
    /// Function and method declarations.
    pub functions: Vec<AdapterFunctionSyntax>,
    /// Class declarations.
    pub classes: Vec<AdapterClassSyntax>,
    /// Import declarations.
    pub imports: Vec<AdapterImportSyntax>,
    /// Source call sites. Stub bodies never contribute entries here.
    pub call_sites: Vec<AdapterCallSiteSyntax>,
}

/// Function or method syntax fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterFunctionSyntax {
    /// Local declaration name.
    pub name: String,
    /// Qualified declaration path within the file.
    pub qualified_name: String,
    /// Whether this is an async function.
    pub is_async: bool,
    /// Raw decorator expressions.
    pub decorators: Vec<String>,
    /// Source location.
    pub location: SourceLocation,
}

/// Class syntax fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterClassSyntax {
    /// Local class name.
    pub name: String,
    /// Qualified declaration path within the file.
    pub qualified_name: String,
    /// Raw base-class expressions.
    pub bases: Vec<String>,
    /// Raw decorator expressions.
    pub decorators: Vec<String>,
    /// Source location.
    pub location: SourceLocation,
}

/// Import syntax fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterImportSyntax {
    /// Imported module or top-level imported module for `import` statements.
    pub module: Option<String>,
    /// Imported symbol for `from ... import ...` statements.
    pub name: Option<String>,
    /// Optional alias.
    pub alias: Option<String>,
    /// Relative import level.
    pub level: u32,
    /// Source location.
    pub location: SourceLocation,
}

/// Call-site syntax fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterCallSiteSyntax {
    /// Enclosing declaration qualified name, if any.
    pub enclosing_qualified_name: Option<String>,
    /// Raw call expression.
    pub expression: String,
    /// Source location.
    pub location: SourceLocation,
}

/// Semantic facts for one file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterFileSemantics {
    /// File identifier in the facade.
    pub file: FileId,
    /// Normalized file path.
    pub path: PathBuf,
    /// Resolved call facts. Stub bodies never contribute entries here.
    pub calls: Vec<AdapterCallSemantic>,
}

/// Semantic call fact normalized through the facade.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterCallSemantic {
    /// Enclosing declaration qualified name, if any.
    pub enclosing_qualified_name: Option<String>,
    /// Raw call expression.
    pub expression: String,
    /// Resolved target.
    pub target: ResolvedTarget,
    /// Whether this call is the event-loop executor escape hatch.
    pub is_event_loop_run_in_executor: bool,
    /// Source location.
    pub location: SourceLocation,
}
