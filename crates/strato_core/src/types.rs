//! Shared Strato core data types.

use std::collections::{BTreeMap, BTreeSet};

use camino::Utf8PathBuf;

/// File manifest produced by discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileManifest {
    /// Discovered Python files in deterministic path order.
    pub files: Vec<FileEntry>,
    /// Effective first-party source roots.
    pub source_roots: Vec<Utf8PathBuf>,
    /// Effective loaded configuration.
    pub config: StratoConfig,
    /// Effective blocking database after config additions/removals.
    pub blocking_database: BlockingDatabase,
    /// Executor wrapper escape hatch config.
    pub escape_hatch_config: EscapeHatchConfig,
}

/// One discovered Python file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Absolute normalized file path.
    pub path: Utf8PathBuf,
    /// Lowercase SHA-256 content hash.
    pub content_hash: String,
    /// Source or stub classification.
    pub kind: FileKind,
    /// True when the file is under a first-party source root.
    pub is_first_party: bool,
}

/// Python file kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileKind {
    /// `.py` source file.
    Source,
    /// `.pyi` stub file.
    Stub,
}

/// Effective Strato configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StratoConfig {
    /// Directory relative paths in this config resolve against.
    pub root: Utf8PathBuf,
    /// Explicit source roots, before auto-detection.
    pub src_roots: Option<Vec<Utf8PathBuf>>,
    /// Exclude globs from `[tool.strato]`.
    pub exclude: Vec<String>,
    /// Additional third-party stub paths.
    pub stub_paths: Vec<Utf8PathBuf>,
    /// Python version string for later ty initialization.
    pub python_version: String,
    /// Reporting strategy.
    pub intervention_strategy: InterventionStrategy,
    /// Diagnostic severity.
    pub severity: DiagnosticSeverity,
    /// Output format.
    pub output_format: OutputFormat,
    /// Cache directory.
    pub cache_dir: Utf8PathBuf,
    /// Whether cache use is enabled.
    pub cache_enabled: bool,
    /// Blocking database user overrides.
    pub blocking: BlockingConfig,
    /// Configured executor wrappers.
    pub executor_wrappers: BTreeMap<String, ExecutorWrapperConfig>,
}

impl StratoConfig {
    /// Return default config rooted at `root`.
    #[must_use]
    pub fn defaults(root: Utf8PathBuf) -> Self {
        Self {
            cache_dir: root.join(".strato_cache"),
            root,
            src_roots: None,
            exclude: Vec::new(),
            stub_paths: Vec::new(),
            python_version: "3.9".to_string(),
            intervention_strategy: InterventionStrategy::FirstPartyDeepest,
            severity: DiagnosticSeverity::Error,
            output_format: OutputFormat::Text,
            cache_enabled: true,
            blocking: BlockingConfig::default(),
            executor_wrappers: BTreeMap::new(),
        }
    }
}

/// Blocking config additions and removals.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockingConfig {
    /// Custom blocking entries to add.
    pub add: Vec<BlockingEntry>,
    /// Qualified names to remove.
    pub remove: BTreeSet<String>,
    /// Module prefixes that are always blocking.
    pub blocking_modules: BTreeSet<String>,
}

/// Blocking function database.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockingDatabase {
    /// Blocking entries by qualified name.
    pub entries: BTreeMap<String, BlockingEntry>,
    /// Blocking module prefixes.
    pub blocking_modules: BTreeSet<String>,
}

impl BlockingDatabase {
    /// Return the configured canonical blocking name for an alias.
    #[must_use]
    pub fn canonical_name<'a>(&self, qualified_name: &'a str) -> Option<&'a str> {
        if self.entries.contains_key(qualified_name) {
            return Some(qualified_name);
        }
        let canonical = match qualified_name {
            "_socket.socket.connect" => "socket.socket.connect",
            "_socket.socket.recv" => "socket.socket.recv",
            "_socket.socket.send" => "socket.socket.send",
            "_socket.socket.accept" => "socket.socket.accept",
            "_socket.socket.sendall" => "socket.socket.sendall",
            "_socket.socket.recvfrom" => "socket.socket.recvfrom",
            _ => return None,
        };
        self.entries.contains_key(canonical).then_some(canonical)
    }

    /// Return whether a facade-provided qualified target is effectively blocking.
    #[must_use]
    pub fn matches_blocking_target(&self, qualified_name: &str) -> bool {
        self.canonical_name(qualified_name).is_some()
            || self
                .blocking_modules
                .iter()
                .any(|prefix| module_boundary_matches(prefix, qualified_name))
    }
}

fn module_boundary_matches(prefix: &str, qualified_name: &str) -> bool {
    qualified_name == prefix
        || qualified_name
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

/// One blocking callable entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockingEntry {
    /// Qualified callable name.
    pub name: String,
    /// Help text used by later reporting phases.
    pub help: String,
    /// Blocking category.
    pub category: BlockingCategory,
}

/// Blocking categories from the documented schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlockingCategory {
    /// Sleep APIs.
    Sleep,
    /// Network I/O.
    NetworkIo,
    /// File I/O.
    FileIo,
    /// Subprocess APIs.
    Subprocess,
    /// Database I/O.
    DatabaseIo,
    /// User input.
    UserInput,
    /// Other blocking work.
    Other,
}

impl BlockingCategory {
    /// Parse a documented category string.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "sleep" => Self::Sleep,
            "network-io" => Self::NetworkIo,
            "file-io" => Self::FileIo,
            "subprocess" => Self::Subprocess,
            "database-io" => Self::DatabaseIo,
            "user-input" => Self::UserInput,
            "other" => Self::Other,
            _ => return None,
        })
    }

    /// Return the documented category string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sleep => "sleep",
            Self::NetworkIo => "network-io",
            Self::FileIo => "file-io",
            Self::Subprocess => "subprocess",
            Self::DatabaseIo => "database-io",
            Self::UserInput => "user-input",
            Self::Other => "other",
        }
    }
}

/// Executor wrapper escape hatch configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorWrapperConfig {
    /// Which wrapper parameter receives the callable.
    pub callable_param: CallableParam,
}

/// Callable parameter selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallableParam {
    /// Positional parameter index.
    Position(u64),
    /// Keyword parameter name.
    Keyword(String),
}

/// Escape hatch config used by later annotation phases.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EscapeHatchConfig {
    /// Executor wrappers keyed by qualified name.
    pub executor_wrappers: BTreeMap<String, ExecutorWrapperConfig>,
}

/// Reporting intervention strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterventionStrategy {
    /// Report deepest first-party call.
    FirstPartyDeepest,
    /// Report async boundary.
    AsyncBoundary,
}

/// Diagnostic severity setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// Error severity.
    Error,
    /// Warning severity.
    Warning,
}

/// Output format setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Text output.
    Text,
    /// JSON output.
    Json,
    /// SARIF output.
    Sarif,
}

/// Recoverable analysis warning collected without halting later phases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisWarning {
    /// A Python syntax error reported by the parser of record.
    SyntaxError {
        /// File containing the syntax error.
        path: Utf8PathBuf,
        /// Human-readable parser message.
        error: String,
    },
    /// A recoverable adapter-boundary failure.
    Adapter {
        /// File being queried, when known.
        path: Option<Utf8PathBuf>,
        /// Human-readable adapter message.
        error: String,
    },
    /// General recoverable analysis warning.
    General {
        /// File containing the warning.
        path: Option<Utf8PathBuf>,
        /// Human-readable warning message.
        message: String,
    },
}

/// Source location represented as byte offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceLocation {
    /// Zero-based byte offset where this fact starts.
    pub start: u32,
    /// Zero-based byte offset where this fact ends.
    pub end: u32,
}

/// Strato-owned syntax facts for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSyntax {
    /// Source or stub path.
    pub path: Utf8PathBuf,
    /// Source or stub classification from discovery.
    pub kind: FileKind,
    /// Function and method declarations.
    pub functions: Vec<FunctionSyntax>,
    /// Class declarations.
    pub classes: Vec<ClassSyntax>,
    /// Import declarations.
    pub imports: Vec<ImportSyntax>,
    /// Source call sites. Stub bodies never contribute entries here.
    pub call_sites: Vec<CallSiteSyntax>,
}

/// Function or method declaration syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub location: SourceLocation,
}

/// Class declaration syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub location: SourceLocation,
}

/// Import declaration syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub location: SourceLocation,
}

/// Call-site syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSiteSyntax {
    /// Enclosing declaration qualified path, if any.
    pub enclosing_qualified_name: Option<String>,
    /// Raw call expression.
    pub expression: String,
    /// Source location.
    pub location: SourceLocation,
}

/// In-memory semantic facts for one analysis run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticFacts {
    /// Resolved call facts by path.
    pub calls_by_path: BTreeMap<Utf8PathBuf, Vec<SemanticCall>>,
    /// Recoverable warnings encountered while querying semantics.
    pub warnings: Vec<AnalysisWarning>,
}

/// Semantic call fact normalized through the adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCall {
    /// Enclosing declaration qualified path, if any.
    pub enclosing_qualified_name: Option<String>,
    /// Raw call expression.
    pub expression: String,
    /// Resolved call target.
    pub target: SemanticTarget,
    /// Whether this call is the event-loop executor escape hatch.
    pub is_event_loop_run_in_executor: bool,
    /// Source location.
    pub location: SourceLocation,
}

/// Adapter-normalized semantic target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticTarget {
    /// A first-party definition key.
    FirstPartyDefinition(String),
    /// One or more external qualified names.
    ExternalQualifiedNames(BTreeSet<String>),
    /// Unknown or unresolved target.
    Unknown,
}
