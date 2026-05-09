//! Cache manifest and artifact boundary types.

use std::collections::BTreeMap;

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use sha2::{Digest, Sha256};

/// Returns the lowercase SHA-256 digest for `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Allowed cache artifact categories owned by Strato.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CacheArtifactKind {
    /// Discovery/file manifest artifacts.
    Discovery,
    /// Strato-owned syntax extraction artifacts.
    Syntax,
    /// Raw decorator syntax artifacts.
    Decorators,
}

impl CacheArtifactKind {
    /// Return every allowed cache artifact kind.
    #[must_use]
    pub const fn all() -> [Self; 3] {
        [Self::Discovery, Self::Syntax, Self::Decorators]
    }

    /// Stable string form used in storage paths and manifests.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::Syntax => "syntax",
            Self::Decorators => "decorators",
        }
    }
}

/// Stable cache storage key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StorageKey {
    /// Artifact kind.
    pub kind: CacheArtifactKind,
    /// Content-addressed or logical key.
    pub key: String,
}

impl StorageKey {
    /// Create a storage key.
    #[must_use]
    pub fn new(kind: CacheArtifactKind, key: impl Into<String>) -> Self {
        Self {
            kind,
            key: key.into(),
        }
    }
}

/// Cache manifest with deterministic entry ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheManifest {
    /// Manifest schema version.
    pub version: u32,
    /// Strato version that wrote this manifest.
    pub strato_version: String,
    /// Effective analysis configuration hash.
    pub config_hash: String,
    /// Content hashes by storage key.
    pub entries: BTreeMap<StorageKey, String>,
}

impl CacheManifest {
    /// Create an empty cache manifest.
    #[must_use]
    pub fn new(version: u32) -> Self {
        Self {
            version,
            strato_version: String::new(),
            config_hash: String::new(),
            entries: BTreeMap::new(),
        }
    }

    /// Create an empty cache manifest with compatibility metadata.
    #[must_use]
    pub fn with_metadata(
        version: u32,
        strato_version: impl Into<String>,
        config_hash: impl Into<String>,
    ) -> Self {
        Self {
            version,
            strato_version: strato_version.into(),
            config_hash: config_hash.into(),
            entries: BTreeMap::new(),
        }
    }

    /// Return whether this manifest is compatible with the requested run.
    #[must_use]
    pub fn is_compatible(&self, version: u32, strato_version: &str, config_hash: &str) -> bool {
        self.version == version
            && self.strato_version == strato_version
            && self.config_hash == config_hash
    }

    /// Record a cache entry content hash.
    pub fn record(&mut self, key: StorageKey, content_hash: impl Into<String>) {
        self.entries.insert(key, content_hash.into());
    }
}

/// Strato-owned per-file cached result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedFileResult {
    /// SHA-256 content hash of the source file.
    pub content_hash: String,
    /// Syntax declarations and imports extracted from Ruff parsed modules.
    pub syntax: FileSyntax,
    /// Raw decorator expressions before semantic classification.
    pub raw_decorators: Vec<DecoratorSyntax>,
}

/// Strato-owned syntax facts safe to serialize.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSyntax {
    /// Source or stub path.
    pub path: Utf8PathBuf,
    /// Source or stub classification.
    pub kind: CachedFileKind,
    /// Function and method declarations.
    pub functions: Vec<FunctionSyntax>,
    /// Class declarations.
    pub classes: Vec<ClassSyntax>,
    /// Import declarations.
    pub imports: Vec<ImportSyntax>,
    /// Source call sites.
    pub call_sites: Vec<CallSiteSyntax>,
}

/// Cached source file kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CachedFileKind {
    /// `.py` source file.
    Source,
    /// `.pyi` stub file.
    Stub,
}

/// Source location represented as byte offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyntaxLocation {
    /// Zero-based byte offset where this fact starts.
    pub start: u32,
    /// Zero-based byte offset where this fact ends.
    pub end: u32,
}

/// Function or method declaration syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionSyntax {
    /// Local function name.
    pub name: String,
    /// Qualified path within the module.
    pub qualified_name: String,
    /// Whether the function is async.
    pub is_async: bool,
    /// Raw decorator expressions.
    pub decorators: Vec<String>,
    /// Source location.
    pub location: SyntaxLocation,
}

/// Class declaration syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassSyntax {
    /// Local class name.
    pub name: String,
    /// Qualified path within the module.
    pub qualified_name: String,
    /// Raw base expressions.
    pub bases: Vec<String>,
    /// Raw decorator expressions.
    pub decorators: Vec<String>,
    /// Source location.
    pub location: SyntaxLocation,
}

/// Import declaration syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportSyntax {
    /// Imported module.
    pub module: Option<String>,
    /// Imported symbol for from-imports.
    pub name: Option<String>,
    /// Optional alias.
    pub alias: Option<String>,
    /// Relative import level.
    pub level: u32,
    /// Source location.
    pub location: SyntaxLocation,
}

/// Call-site syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallSiteSyntax {
    /// Enclosing declaration qualified path, if any.
    pub enclosing_qualified_name: Option<String>,
    /// Raw call expression.
    pub expression: String,
    /// Source location.
    pub location: SyntaxLocation,
}

/// Raw decorator syntax safe to serialize.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoratorSyntax {
    /// Decorated target name.
    pub target: String,
    /// Raw decorator expression.
    pub expression: String,
}

/// Serializable cache artifacts. No catch-all variant is provided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CacheArtifact {
    /// Discovery manifest bytes owned by Strato discovery.
    Discovery(Vec<u8>),
    /// Per-file syntax extraction result.
    Syntax(CachedFileResult),
    /// Raw decorator syntax list.
    Decorators(Vec<DecoratorSyntax>),
}

impl CacheArtifact {
    /// Return this artifact's allowed kind.
    #[must_use]
    pub const fn kind(&self) -> CacheArtifactKind {
        match self {
            Self::Discovery(_) => CacheArtifactKind::Discovery,
            Self::Syntax(_) => CacheArtifactKind::Syntax,
            Self::Decorators(_) => CacheArtifactKind::Decorators,
        }
    }
}
