//! Phase 2 parse integration through the adapter boundary.

use std::collections::BTreeMap;

use camino::Utf8PathBuf;
use strato_cache::{
    CacheArtifact, CacheArtifactKind, CacheManifest, CacheStorage, CachedFileKind,
    CachedFileResult, StorageKey,
};
use strato_ty_adapter::{
    facade::{FacadeError, StratoTyFacade},
    project::StratoProject,
    targets as adapter,
};
use thiserror::Error;

use crate::{
    STRATO_VERSION,
    types::{
        AnalysisWarning, CallSiteSyntax, ClassSyntax, FileKind, FileManifest, FileSyntax,
        FunctionSyntax, ImportSyntax, SourceLocation,
    },
};

const CACHE_FORMAT_VERSION: u32 = 1;

/// Output from Phase 2 parsing.
pub struct ParsedProject {
    /// Facade initialized from discovery output and reused by semantics.
    pub facade: StratoTyFacade,
    /// Deterministic syntax facts by normalized path.
    pub syntax_by_path: BTreeMap<Utf8PathBuf, FileSyntax>,
    /// Recoverable parse warnings.
    pub warnings: Vec<AnalysisWarning>,
}

/// Fatal parse setup errors.
#[derive(Debug, Error)]
pub enum ParseError {
    /// Adapter project setup failed before file-level recovery was possible.
    #[error(transparent)]
    Adapter(#[from] FacadeError),
    /// Cache storage failed.
    #[error(transparent)]
    Cache(#[from] strato_cache::StorageError),
}

/// Initialize the adapter from discovery output and extract Strato-owned syntax facts.
pub fn parse_project(manifest: &FileManifest) -> Result<ParsedProject, ParseError> {
    parse_project_inner(manifest, None)
}

/// Initialize the adapter and reuse cached Strato-owned syntax facts when valid.
pub fn parse_project_with_cache(manifest: &FileManifest) -> Result<ParsedProject, ParseError> {
    let storage = CacheStorage::new(manifest.config.cache_dir.clone());
    parse_project_inner(manifest, Some(&storage))
}

fn parse_project_inner(
    manifest: &FileManifest,
    storage: Option<&CacheStorage>,
) -> Result<ParsedProject, ParseError> {
    let project = StratoProject::from_paths(
        manifest.config.root.as_std_path(),
        manifest.files.iter().map(|file| file.path.as_std_path()),
    )?;
    let facade = StratoTyFacade::new(project);
    let kinds_by_path = manifest
        .files
        .iter()
        .map(|file| (file.path.clone(), file.kind))
        .collect::<BTreeMap<_, _>>();
    let hashes_by_path = manifest
        .files
        .iter()
        .map(|file| (file.path.clone(), file.content_hash.clone()))
        .collect::<BTreeMap<_, _>>();
    let config_hash = cache_config_hash(manifest);
    let previous_manifest = match &storage {
        Some(storage) => storage.read_manifest()?,
        None => None,
    };
    let previous_manifest = previous_manifest.filter(|manifest| {
        manifest.is_compatible(CACHE_FORMAT_VERSION, STRATO_VERSION, &config_hash)
    });
    let mut next_manifest =
        CacheManifest::with_metadata(CACHE_FORMAT_VERSION, STRATO_VERSION, config_hash.clone());
    let mut syntax_by_path = BTreeMap::new();
    let mut warnings = Vec::new();

    for file in facade.files() {
        let path = utf8_path(file.path());
        let content_hash = hashes_by_path.get(&path).cloned().unwrap_or_default();
        let key = StorageKey::new(CacheArtifactKind::Syntax, path.to_string());
        next_manifest.record(key.clone(), content_hash.clone());
        match facade.syntax_errors_in_file(file.id()) {
            Ok(errors) => {
                warnings.extend(
                    errors
                        .into_iter()
                        .map(|error| AnalysisWarning::SyntaxError {
                            path: path.clone(),
                            error,
                        }),
                );
            }
            Err(error) => warnings.push(AnalysisWarning::Adapter {
                path: Some(path.clone()),
                error: error.to_string(),
            }),
        }
        if let Some(syntax) =
            read_cached_syntax(storage, previous_manifest.as_ref(), &key, &content_hash)?
        {
            syntax_by_path.insert(syntax.path.clone(), syntax);
            continue;
        }
        match facade.syntax_in_file(file.id()) {
            Ok(syntax) => {
                let syntax = convert_file_syntax(syntax, &kinds_by_path);
                write_cached_syntax(storage, &key, &content_hash, &syntax)?;
                syntax_by_path.insert(syntax.path.clone(), syntax);
            }
            Err(error) => warnings.push(AnalysisWarning::Adapter {
                path: Some(path),
                error: error.to_string(),
            }),
        }
    }
    if let Some(storage) = &storage {
        storage.write_manifest(&next_manifest)?;
    }

    Ok(ParsedProject {
        facade,
        syntax_by_path,
        warnings,
    })
}

fn read_cached_syntax(
    storage: Option<&CacheStorage>,
    manifest: Option<&CacheManifest>,
    key: &StorageKey,
    content_hash: &str,
) -> Result<Option<FileSyntax>, ParseError> {
    let Some(storage) = storage else {
        return Ok(None);
    };
    if manifest.and_then(|manifest| manifest.entries.get(key)) != Some(&content_hash.to_string()) {
        return Ok(None);
    }
    let Some(artifact) = storage.read(key)? else {
        return Ok(None);
    };
    let CacheArtifact::Syntax(cached) = artifact else {
        return Ok(None);
    };
    if cached.content_hash != content_hash {
        return Ok(None);
    }
    Ok(Some(convert_cached_file_syntax(cached.syntax)))
}

fn write_cached_syntax(
    storage: Option<&CacheStorage>,
    key: &StorageKey,
    content_hash: &str,
    syntax: &FileSyntax,
) -> Result<(), ParseError> {
    if let Some(storage) = storage {
        let cached = CachedFileResult {
            content_hash: content_hash.to_string(),
            raw_decorators: raw_decorators(syntax),
            syntax: convert_to_cached_file_syntax(syntax),
        };
        storage.write(key, &CacheArtifact::Syntax(cached))?;
    }
    Ok(())
}

fn cache_config_hash(manifest: &FileManifest) -> String {
    strato_cache::sha256_hex(format!("{:?}", manifest.config).as_bytes())
}

fn convert_file_syntax(
    syntax: adapter::AdapterFileSyntax,
    kinds_by_path: &BTreeMap<Utf8PathBuf, FileKind>,
) -> FileSyntax {
    let path = utf8_path(&syntax.path);
    let kind = kinds_by_path
        .get(&path)
        .copied()
        .unwrap_or(if syntax.is_stub {
            FileKind::Stub
        } else {
            FileKind::Source
        });
    FileSyntax {
        path,
        kind,
        functions: syntax.functions.into_iter().map(convert_function).collect(),
        classes: syntax.classes.into_iter().map(convert_class).collect(),
        imports: syntax.imports.into_iter().map(convert_import).collect(),
        call_sites: syntax
            .call_sites
            .into_iter()
            .map(convert_call_site)
            .collect(),
    }
}

fn convert_to_cached_file_syntax(syntax: &FileSyntax) -> strato_cache::FileSyntax {
    strato_cache::FileSyntax {
        path: syntax.path.clone(),
        kind: match syntax.kind {
            FileKind::Source => CachedFileKind::Source,
            FileKind::Stub => CachedFileKind::Stub,
        },
        functions: syntax
            .functions
            .iter()
            .map(|function| strato_cache::FunctionSyntax {
                name: function.name.clone(),
                qualified_name: function.qualified_name.clone(),
                is_async: function.is_async,
                decorators: function.decorators.clone(),
                location: convert_to_cached_location(function.location),
            })
            .collect(),
        classes: syntax
            .classes
            .iter()
            .map(|class| strato_cache::ClassSyntax {
                name: class.name.clone(),
                qualified_name: class.qualified_name.clone(),
                bases: class.bases.clone(),
                decorators: class.decorators.clone(),
                location: convert_to_cached_location(class.location),
            })
            .collect(),
        imports: syntax
            .imports
            .iter()
            .map(|import| strato_cache::ImportSyntax {
                module: import.module.clone(),
                name: import.name.clone(),
                alias: import.alias.clone(),
                level: import.level,
                location: convert_to_cached_location(import.location),
            })
            .collect(),
        call_sites: syntax
            .call_sites
            .iter()
            .map(|call| strato_cache::CallSiteSyntax {
                enclosing_qualified_name: call.enclosing_qualified_name.clone(),
                expression: call.expression.clone(),
                location: convert_to_cached_location(call.location),
            })
            .collect(),
    }
}

fn convert_cached_file_syntax(syntax: strato_cache::FileSyntax) -> FileSyntax {
    FileSyntax {
        path: syntax.path,
        kind: match syntax.kind {
            CachedFileKind::Source => FileKind::Source,
            CachedFileKind::Stub => FileKind::Stub,
        },
        functions: syntax
            .functions
            .into_iter()
            .map(|function| FunctionSyntax {
                name: function.name,
                qualified_name: function.qualified_name,
                is_async: function.is_async,
                decorators: function.decorators,
                location: convert_cached_location(function.location),
            })
            .collect(),
        classes: syntax
            .classes
            .into_iter()
            .map(|class| ClassSyntax {
                name: class.name,
                qualified_name: class.qualified_name,
                bases: class.bases,
                decorators: class.decorators,
                location: convert_cached_location(class.location),
            })
            .collect(),
        imports: syntax
            .imports
            .into_iter()
            .map(|import| ImportSyntax {
                module: import.module,
                name: import.name,
                alias: import.alias,
                level: import.level,
                location: convert_cached_location(import.location),
            })
            .collect(),
        call_sites: syntax
            .call_sites
            .into_iter()
            .map(|call| CallSiteSyntax {
                enclosing_qualified_name: call.enclosing_qualified_name,
                expression: call.expression,
                location: convert_cached_location(call.location),
            })
            .collect(),
    }
}

fn convert_to_cached_location(location: SourceLocation) -> strato_cache::SyntaxLocation {
    strato_cache::SyntaxLocation {
        start: location.start,
        end: location.end,
    }
}

fn convert_cached_location(location: strato_cache::SyntaxLocation) -> SourceLocation {
    SourceLocation {
        start: location.start,
        end: location.end,
    }
}

fn raw_decorators(syntax: &FileSyntax) -> Vec<strato_cache::DecoratorSyntax> {
    let mut decorators = Vec::new();
    decorators.extend(syntax.functions.iter().flat_map(|function| {
        function
            .decorators
            .iter()
            .map(|decorator| strato_cache::DecoratorSyntax {
                target: function.qualified_name.clone(),
                expression: decorator.clone(),
            })
    }));
    decorators.extend(syntax.classes.iter().flat_map(|class| {
        class
            .decorators
            .iter()
            .map(|decorator| strato_cache::DecoratorSyntax {
                target: class.qualified_name.clone(),
                expression: decorator.clone(),
            })
    }));
    decorators
}

fn convert_function(function: adapter::AdapterFunctionSyntax) -> FunctionSyntax {
    FunctionSyntax {
        name: function.name,
        qualified_name: function.qualified_name,
        is_async: function.is_async,
        decorators: function.decorators,
        location: convert_location(function.location),
    }
}

fn convert_class(class: adapter::AdapterClassSyntax) -> ClassSyntax {
    ClassSyntax {
        name: class.name,
        qualified_name: class.qualified_name,
        bases: class.bases,
        decorators: class.decorators,
        location: convert_location(class.location),
    }
}

fn convert_import(import: adapter::AdapterImportSyntax) -> ImportSyntax {
    ImportSyntax {
        module: import.module,
        name: import.name,
        alias: import.alias,
        level: import.level,
        location: convert_location(import.location),
    }
}

fn convert_call_site(call: adapter::AdapterCallSiteSyntax) -> CallSiteSyntax {
    CallSiteSyntax {
        enclosing_qualified_name: call.enclosing_qualified_name,
        expression: call.expression,
        location: convert_location(call.location),
    }
}

pub(crate) fn convert_location(location: adapter::SourceLocation) -> SourceLocation {
    SourceLocation {
        start: location.start,
        end: location.end,
    }
}

pub(crate) fn utf8_path(path: &std::path::Path) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(path.to_path_buf()).expect("adapter paths are valid UTF-8")
}
