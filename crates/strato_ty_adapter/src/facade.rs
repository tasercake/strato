//! Stable semantic query facade exposed to `strato_core`.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::project::StratoProject;
use crate::targets::{
    AdapterCallSemantic, AdapterCallSiteSyntax, AdapterClassSyntax, AdapterFileSemantics,
    AdapterFileSyntax, AdapterFunctionSyntax, AdapterImportSyntax, CallableInfo, DefinitionKey,
    DunderOperation, FileId, FileInfo, ResolvedTarget, SourceLocation,
};

use ruff_db::files::File;
use ruff_db::parsed::{ParsedModuleRef, parsed_module};
use ruff_db::source::source_text;
use ruff_python_ast as ast;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal};
use ruff_python_ast::{AnyNodeRef, Expr, Stmt};
use ruff_text_size::Ranged;
use thiserror::Error;
use ty_python_core::definition::{Definition, DefinitionKind};
use ty_python_semantic::types::Type;
use ty_python_semantic::types::ide_support::{
    ImportAliasResolution, ResolvedDefinition, definitions_for_attribute, definitions_for_bin_op,
    definitions_for_name, definitions_for_unary_op,
};
use ty_python_semantic::{HasType, SemanticModel};

/// Recoverable facade result.
pub type FacadeResult<T> = Result<T, FacadeError>;

/// Recoverable facade errors that later analysis phases can convert into warnings.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum FacadeError {
    /// Project database setup failed.
    #[error("project setup failed: {0}")]
    ProjectSetup(String),
    /// File loading or synchronization failed.
    #[error("file load failed: {0}")]
    FileLoad(String),
    /// The caller supplied an unknown facade file identifier.
    #[error("unknown file id: {0:?}")]
    UnknownFile(FileId),
    /// Vendored Ruff/ty panicked while answering a semantic query.
    #[error("vendored ty query panicked: {0}")]
    VendoredPanic(String),
}

/// Facade entry point for normalized semantic queries.
#[derive(Debug)]
pub struct StratoTyFacade {
    project: StratoProject,
}

/// Backwards-compatible alias for the scaffold name used before Task 5.
pub type TyFacade = StratoTyFacade;

impl StratoTyFacade {
    /// Creates a facade around a ty-backed Strato project handle.
    #[must_use]
    pub const fn new(project: StratoProject) -> Self {
        Self { project }
    }

    /// Returns the project handle currently owned by this facade.
    #[must_use]
    pub const fn project(&self) -> &StratoProject {
        &self.project
    }

    /// Returns deterministic file metadata for all project Python files.
    #[must_use]
    pub fn files(&self) -> Vec<FileInfo> {
        self.project.files()
    }

    /// Returns Ruff's parsed module for a facade file.
    pub fn parsed_module(&self, file: FileId) -> FacadeResult<Option<ParsedModuleRef>> {
        let Some(file) = self.project.project_file(file) else {
            return Err(FacadeError::UnknownFile(file));
        };
        recover(|| Some(parsed_module(self.project.db(), file.raw).load(self.project.db())))
    }

    /// Returns normalized callable declarations in a file.
    pub fn callables_in_file(&self, file: FileId) -> FacadeResult<Vec<CallableInfo>> {
        let Some(project_file) = self.project.project_file(file) else {
            return Err(FacadeError::UnknownFile(file));
        };
        recover(|| {
            let module = parsed_module(self.project.db(), project_file.raw).load(self.project.db());
            let mut visitor = CallableCollector {
                db: self.project.db(),
                file: project_file.raw,
                callables: Vec::new(),
            };
            for statement in module.suite() {
                visitor.visit_stmt(statement);
            }
            visitor.callables.sort_by(|left, right| {
                left.definition()
                    .as_str()
                    .cmp(right.definition().as_str())
                    .then_with(|| left.range().start().cmp(&right.range().start()))
            });
            visitor.callables
        })
    }

    /// Extracts Strato-owned syntax facts for a file without exposing parser nodes.
    pub fn syntax_in_file(&self, file: FileId) -> FacadeResult<AdapterFileSyntax> {
        let Some(project_file) = self.project.project_file(file) else {
            return Err(FacadeError::UnknownFile(file));
        };
        recover(|| {
            let parsed = parsed_module(self.project.db(), project_file.raw).load(self.project.db());
            let source = source_text(self.project.db(), project_file.raw);
            let file_info = self.file_info(file).expect("file id was checked");
            let mut collector = SyntaxCollector::new(source.as_str(), file_info.is_stub());
            collector.visit_suite(parsed.suite(), None);
            collector.finish(&file_info)
        })
    }

    /// Returns syntax error messages for a file as recoverable warning text.
    pub fn syntax_errors_in_file(&self, file: FileId) -> FacadeResult<Vec<String>> {
        let Some(project_file) = self.project.project_file(file) else {
            return Err(FacadeError::UnknownFile(file));
        };
        recover(|| {
            let parsed = parsed_module(self.project.db(), project_file.raw).load(self.project.db());
            parsed
                .errors()
                .iter()
                .map(std::string::ToString::to_string)
                .collect()
        })
    }

    /// Extracts normalized semantic facts for a file without exposing parser nodes.
    pub fn semantic_facts_in_file(&self, file: FileId) -> FacadeResult<AdapterFileSemantics> {
        let Some(project_file) = self.project.project_file(file) else {
            return Err(FacadeError::UnknownFile(file));
        };
        recover(|| {
            let parsed = parsed_module(self.project.db(), project_file.raw).load(self.project.db());
            let source = source_text(self.project.db(), project_file.raw);
            let file_info = self.file_info(file).expect("file id was checked");
            let path = project_file.raw.path(self.project.db()).to_string();
            let mut collector = SemanticCollector::new(
                self,
                file,
                path.as_str(),
                source.as_str(),
                file_info.is_stub(),
            );
            collector.visit_suite(parsed.suite(), None);
            AdapterFileSemantics {
                file,
                path: file_info.path().to_path_buf(),
                calls: collector.calls,
            }
        })
    }

    /// Resolves a call target using vendored ty facts where they are publicly exposed.
    pub fn resolve_call_target(
        &self,
        file: FileId,
        call: &ast::ExprCall,
    ) -> FacadeResult<ResolvedTarget> {
        self.resolve_callable_reference(file, &call.func)
    }

    /// Resolves a callable reference using vendored ty facts where they are publicly exposed.
    pub fn resolve_callable_reference(
        &self,
        file: FileId,
        expr: &Expr,
    ) -> FacadeResult<ResolvedTarget> {
        self.with_model(file, |facade, model| match expr {
            Expr::Name(name) => facade.definitions_to_target(definitions_for_name(
                model,
                name.id.as_str(),
                name.into(),
                ImportAliasResolution::ResolveAliases,
            )),
            Expr::Attribute(attribute) => Self::module_attribute_external_target(model, attribute)
                .unwrap_or_else(|| {
                    facade.definitions_to_target(definitions_for_attribute(model, attribute))
                }),
            _ => ResolvedTarget::Unknown,
        })
    }

    /// Resolves an attribute target using vendored ty attribute definition facts.
    pub fn resolve_attribute_target(
        &self,
        file: FileId,
        attr: &ast::ExprAttribute,
    ) -> FacadeResult<ResolvedTarget> {
        self.with_model(file, |facade, model| {
            facade.definitions_to_target(definitions_for_attribute(model, attr))
        })
    }

    /// Resolves a property getter target when ty exposes the descriptor fact; otherwise unknown.
    pub fn resolve_property_getter(
        &self,
        file: FileId,
        attr: &ast::ExprAttribute,
    ) -> FacadeResult<ResolvedTarget> {
        self.resolve_attribute_target(file, attr)
    }

    /// Resolves a dunder target using public vendored ty operator definition helpers.
    pub fn resolve_dunder_target(
        &self,
        file: FileId,
        operation: DunderOperation<'_>,
    ) -> FacadeResult<Vec<ResolvedTarget>> {
        self.with_model(file, |facade, model| match operation {
            DunderOperation::Binary(binary) => definitions_for_bin_op(model, binary)
                .map(|(definitions, _)| facade.definitions_to_targets(definitions))
                .unwrap_or_default(),
            DunderOperation::Unary(unary) => definitions_for_unary_op(model, unary)
                .map(|(definitions, _)| facade.definitions_to_targets(definitions))
                .unwrap_or_default(),
        })
    }

    /// Returns true only when vendored ty can identify the call as event-loop `run_in_executor`.
    pub fn resolves_to_event_loop_run_in_executor(
        &self,
        file: FileId,
        call: &ast::ExprCall,
    ) -> FacadeResult<bool> {
        let Some(_) = self.project.project_file(file) else {
            return Err(FacadeError::UnknownFile(file));
        };
        let _ = call;
        Ok(false)
    }

    fn with_model<T>(
        &self,
        file: FileId,
        query: impl FnOnce(&Self, &SemanticModel<'_>) -> T,
    ) -> FacadeResult<T> {
        let Some(project_file) = self.project.project_file(file) else {
            return Err(FacadeError::UnknownFile(file));
        };
        recover(|| {
            let model = SemanticModel::new(self.project.db(), project_file.raw);
            query(self, &model)
        })
    }

    fn file_info(&self, file: FileId) -> Option<FileInfo> {
        self.files().into_iter().find(|info| info.id() == file)
    }

    fn module_attribute_external_target(
        model: &SemanticModel<'_>,
        attribute: &ast::ExprAttribute,
    ) -> Option<ResolvedTarget> {
        let lhs_ty = attribute.value.inferred_type(model)?;
        let names = module_names(model.db(), lhs_ty)
            .map(|module| format!("{module}.{attr}", attr = attribute.attr.as_str()))
            .collect::<BTreeSet<_>>();
        (!names.is_empty()).then_some(ResolvedTarget::ExternalQualifiedNames(names))
    }

    fn definitions_to_targets(
        &self,
        definitions: Vec<ResolvedDefinition<'_>>,
    ) -> Vec<ResolvedTarget> {
        definitions
            .into_iter()
            .map(|definition| self.resolved_definition_to_target(&definition))
            .collect()
    }

    fn definitions_to_target(&self, definitions: Vec<ResolvedDefinition<'_>>) -> ResolvedTarget {
        let mut targets = self.definitions_to_targets(definitions);
        match targets.len() {
            0 => ResolvedTarget::Unknown,
            1 => targets.pop().expect("length checked"),
            _ => {
                let names = targets
                    .into_iter()
                    .filter_map(|target| match target {
                        ResolvedTarget::FirstPartyDefinition(definition) => {
                            Some(definition.as_str().to_owned())
                        }
                        ResolvedTarget::ExternalQualifiedNames(names) => {
                            Some(names.into_iter().collect::<Vec<_>>().join("|"))
                        }
                        ResolvedTarget::Unknown => None,
                    })
                    .collect::<BTreeSet<_>>();
                if names.is_empty() {
                    ResolvedTarget::Unknown
                } else {
                    ResolvedTarget::ExternalQualifiedNames(names)
                }
            }
        }
    }

    fn resolved_definition_to_target(&self, definition: &ResolvedDefinition<'_>) -> ResolvedTarget {
        match definition {
            ResolvedDefinition::Definition(definition) => self.definition_to_target(*definition),
            ResolvedDefinition::Module(file) => self.file_to_external_target(*file),
            ResolvedDefinition::FileWithRange(range) => self.file_to_external_target(range.file()),
        }
    }

    fn definition_to_target(&self, definition: Definition<'_>) -> ResolvedTarget {
        let db = self.project.db();
        if !matches!(
            definition.kind(db),
            DefinitionKind::Function(_)
                | DefinitionKind::Class(_)
                | DefinitionKind::Import(_)
                | DefinitionKind::ImportFrom(_)
                | DefinitionKind::ImportFromSubmodule(_)
                | DefinitionKind::StarImport(_)
        ) {
            return ResolvedTarget::Unknown;
        }
        let file = definition.file(db);
        let name = definition
            .name(db)
            .unwrap_or_else(|| "<anonymous>".to_owned());
        let path = file.path(db).to_string();
        let key = DefinitionKey::new(format!("{path}:{name}"));
        if self.project.contains_raw_file(file) {
            ResolvedTarget::FirstPartyDefinition(key)
        } else {
            ResolvedTarget::ExternalQualifiedNames(BTreeSet::from([key.as_str().to_owned()]))
        }
    }

    fn file_to_external_target(&self, file: File) -> ResolvedTarget {
        ResolvedTarget::ExternalQualifiedNames(BTreeSet::from([file
            .path(self.project.db())
            .to_string()]))
    }
}

fn recover<T>(query: impl FnOnce() -> T) -> FacadeResult<T> {
    catch_unwind(AssertUnwindSafe(query)).map_err(|payload| {
        let message = payload.downcast_ref::<&str>().map_or_else(
            || "non-string panic payload".to_owned(),
            |message| (*message).to_owned(),
        );
        FacadeError::VendoredPanic(message)
    })
}

fn module_names<'db>(
    db: &'db dyn ty_python_semantic::Db,
    ty: Type<'db>,
) -> Box<dyn Iterator<Item = String> + 'db> {
    match ty {
        Type::ModuleLiteral(module_literal) => Box::new(std::iter::once(
            module_literal.module(db).name(db).to_string(),
        )),
        Type::Union(union) => Box::new(
            union
                .elements(db)
                .iter()
                .flat_map(move |ty| module_names(db, *ty)),
        ),
        _ => Box::new(std::iter::empty()),
    }
}

struct CallableCollector<'db> {
    db: &'db ty_project::ProjectDatabase,
    file: File,
    callables: Vec<CallableInfo>,
}

impl<'a> SourceOrderVisitor<'a> for CallableCollector<'_> {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        if let AnyNodeRef::StmtFunctionDef(function) = node {
            let path = self.file.path(self.db).to_string();
            let name = function.name.as_str().to_owned();
            self.callables.push(CallableInfo::new(
                DefinitionKey::new(format!("{path}:{name}")),
                name,
                function.range(),
            ));
        }
        TraversalSignal::Traverse
    }
}

struct SyntaxCollector<'source> {
    source: &'source str,
    is_stub: bool,
    functions: Vec<AdapterFunctionSyntax>,
    classes: Vec<AdapterClassSyntax>,
    imports: Vec<AdapterImportSyntax>,
    call_sites: Vec<AdapterCallSiteSyntax>,
}

impl<'source> SyntaxCollector<'source> {
    fn new(source: &'source str, is_stub: bool) -> Self {
        Self {
            source,
            is_stub,
            functions: Vec::new(),
            classes: Vec::new(),
            imports: Vec::new(),
            call_sites: Vec::new(),
        }
    }

    fn visit_suite(&mut self, suite: &[Stmt], scope: Option<&str>) {
        for statement in suite {
            self.visit_statement(statement, scope);
        }
    }

    fn visit_statement(&mut self, statement: &Stmt, scope: Option<&str>) {
        match statement {
            Stmt::FunctionDef(function) => {
                let qualified_name = qualify(scope, function.name.as_str());
                self.functions.push(AdapterFunctionSyntax {
                    name: function.name.as_str().to_owned(),
                    qualified_name: qualified_name.clone(),
                    is_async: function.is_async,
                    decorators: decorators(self.source, &function.decorator_list),
                    location: SourceLocation::from_range(function.name.range()),
                });
                if !self.is_stub {
                    self.collect_calls(&function.body, Some(&qualified_name));
                    self.visit_suite(&function.body, Some(&qualified_name));
                }
            }
            Stmt::ClassDef(class) => {
                let qualified_name = qualify(scope, class.name.as_str());
                let bases = class
                    .arguments
                    .as_ref()
                    .map(|arguments| {
                        arguments
                            .args
                            .iter()
                            .map(|base| snippet(self.source, base.range()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                self.classes.push(AdapterClassSyntax {
                    name: class.name.as_str().to_owned(),
                    qualified_name: qualified_name.clone(),
                    bases,
                    decorators: decorators(self.source, &class.decorator_list),
                    location: SourceLocation::from_range(class.range()),
                });
                self.visit_suite(&class.body, Some(&qualified_name));
            }
            Stmt::Import(import) => {
                self.imports
                    .extend(import.names.iter().map(|alias| AdapterImportSyntax {
                        module: Some(alias.name.as_str().to_owned()),
                        name: None,
                        alias: alias.asname.as_ref().map(|name| name.as_str().to_owned()),
                        level: 0,
                        location: SourceLocation::from_range(alias.range()),
                    }));
            }
            Stmt::ImportFrom(import) => {
                self.imports.extend(import.names.iter().map(|alias| {
                    AdapterImportSyntax {
                        module: import
                            .module
                            .as_ref()
                            .map(|module| module.as_str().to_owned()),
                        name: Some(alias.name.as_str().to_owned()),
                        alias: alias.asname.as_ref().map(|name| name.as_str().to_owned()),
                        level: import.level,
                        location: SourceLocation::from_range(alias.range()),
                    }
                }));
            }
            _ if !self.is_stub => self.collect_calls(std::slice::from_ref(statement), scope),
            _ => {}
        }
    }

    fn collect_calls(&mut self, suite: &[Stmt], scope: Option<&str>) {
        let mut collector = CallSyntaxCollector {
            source: self.source,
            enclosing_qualified_name: scope.map(str::to_owned),
            calls: Vec::new(),
        };
        for statement in suite {
            collector.visit_stmt(statement);
        }
        self.call_sites.extend(collector.calls);
    }

    fn finish(mut self, file_info: &FileInfo) -> AdapterFileSyntax {
        self.functions
            .sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
        self.classes
            .sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
        self.imports.sort_by(|left, right| {
            left.module
                .cmp(&right.module)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.alias.cmp(&right.alias))
        });
        self.call_sites.sort_by_key(|call| call.location.start);
        AdapterFileSyntax {
            file: file_info.id(),
            path: file_info.path().to_path_buf(),
            is_stub: file_info.is_stub(),
            functions: self.functions,
            classes: self.classes,
            imports: self.imports,
            call_sites: self.call_sites,
        }
    }
}

struct CallSyntaxCollector<'source> {
    source: &'source str,
    enclosing_qualified_name: Option<String>,
    calls: Vec<AdapterCallSiteSyntax>,
}

impl<'a> SourceOrderVisitor<'a> for CallSyntaxCollector<'_> {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        if let AnyNodeRef::ExprCall(call) = node {
            self.calls.push(AdapterCallSiteSyntax {
                enclosing_qualified_name: self.enclosing_qualified_name.clone(),
                expression: snippet(self.source, call.range()),
                location: SourceLocation::from_range(call.range()),
            });
        }
        TraversalSignal::Traverse
    }
}

struct SemanticCollector<'facade, 'source> {
    facade: &'facade StratoTyFacade,
    file: FileId,
    file_path: &'source str,
    source: &'source str,
    is_stub: bool,
    calls: Vec<AdapterCallSemantic>,
    member_targets: BTreeMap<String, DefinitionKey>,
    imported_symbols: BTreeMap<String, String>,
    stub_blocking_function_targets: BTreeSet<String>,
    module_imports: BTreeSet<String>,
    shadowed_names: BTreeSet<String>,
    strato_annotation_aliases: BTreeMap<String, String>,
}

impl<'facade, 'source> SemanticCollector<'facade, 'source> {
    fn new(
        facade: &'facade StratoTyFacade,
        file: FileId,
        file_path: &'source str,
        source: &'source str,
        is_stub: bool,
    ) -> Self {
        Self {
            facade,
            file,
            file_path,
            source,
            is_stub,
            calls: Vec::new(),
            member_targets: facade.stub_blocking_member_targets(),
            imported_symbols: BTreeMap::new(),
            stub_blocking_function_targets: facade.stub_blocking_function_targets(),
            module_imports: BTreeSet::new(),
            shadowed_names: BTreeSet::new(),
            strato_annotation_aliases: BTreeMap::new(),
        }
    }

    fn visit_suite(&mut self, suite: &[Stmt], scope: Option<&str>) {
        for statement in suite {
            self.visit_statement(statement, scope);
        }
    }

    fn visit_import_from(&mut self, import: &ast::StmtImportFrom) {
        if import.level != 0 {
            return;
        }
        let Some(module) = import
            .module
            .as_ref()
            .map(ruff_python_ast::Identifier::as_str)
        else {
            return;
        };
        for alias in &import.names {
            let local = alias
                .asname
                .as_ref()
                .map_or_else(|| alias.name.as_str(), ruff_python_ast::Identifier::as_str);
            let target = format!("{module}.{}", alias.name.as_str());
            if self.stub_blocking_function_targets.contains(&target) {
                self.imported_symbols.insert(local.to_owned(), target);
            }
        }
        self.record_annotation_import_from(
            module,
            import.names.iter().map(|alias| {
                (
                    alias.name.as_str(),
                    alias
                        .asname
                        .as_ref()
                        .map(ruff_python_ast::Identifier::as_str),
                )
            }),
        );
    }

    fn visit_statement(&mut self, statement: &Stmt, scope: Option<&str>) {
        match statement {
            Stmt::Import(import) => {
                for alias in &import.names {
                    let local = alias.asname.as_ref().map_or_else(
                        || {
                            alias
                                .name
                                .as_str()
                                .split('.')
                                .next()
                                .expect("module has head")
                        },
                        ruff_python_ast::Identifier::as_str,
                    );
                    self.module_imports.insert(local.to_owned());
                }
                self.record_annotation_imports(import.names.iter().map(|alias| {
                    (
                        alias.name.as_str(),
                        alias
                            .asname
                            .as_ref()
                            .map(ruff_python_ast::Identifier::as_str),
                    )
                }));
            }
            Stmt::ImportFrom(import) => self.visit_import_from(import),
            Stmt::Assign(assign) => {
                for target in &assign.targets {
                    if let Expr::Name(name) = target {
                        self.shadowed_names.insert(name.id.as_str().to_owned());
                    }
                }
            }
            Stmt::FunctionDef(function) => {
                let qualified_name = qualify(scope, function.name.as_str());
                for decorator in &function.decorator_list {
                    self.collect_decorator(&qualified_name, decorator);
                }
                if !self.is_stub {
                    self.collect_calls(&function.body, Some(&qualified_name));
                    self.visit_suite(&function.body, Some(&qualified_name));
                }
            }
            Stmt::ClassDef(class) => {
                let qualified_name = qualify(scope, class.name.as_str());
                for statement in &class.body {
                    if let Stmt::FunctionDef(function) = statement {
                        self.member_targets.insert(
                            function.name.as_str().to_owned(),
                            DefinitionKey::new(format!(
                                "{}:{}",
                                self.file_path,
                                qualify(Some(&qualified_name), function.name.as_str())
                            )),
                        );
                    }
                }
                for statement in &class.body {
                    if let Stmt::FunctionDef(function) = statement {
                        let member_name = qualify(Some(&qualified_name), function.name.as_str());
                        for decorator in &function.decorator_list {
                            self.collect_decorator(&member_name, decorator);
                        }
                    }
                }
                if !self.is_stub {
                    for statement in &class.body {
                        if let Stmt::FunctionDef(function) = statement {
                            let member_name =
                                qualify(Some(&qualified_name), function.name.as_str());
                            self.collect_calls(&function.body, Some(&member_name));
                            self.visit_suite(&function.body, Some(&member_name));
                        }
                    }
                }
            }
            _ if !self.is_stub => self.collect_calls(std::slice::from_ref(statement), scope),
            _ => {}
        }
    }

    fn collect_calls(&mut self, suite: &[Stmt], scope: Option<&str>) {
        let mut collector = SemanticCallCollector {
            facade: self.facade,
            file: self.file,
            source: self.source,
            enclosing_qualified_name: scope.map(str::to_owned),
            member_targets: &self.member_targets,
            imported_symbols: &self.imported_symbols,
            module_imports: &self.module_imports,
            shadowed_names: &self.shadowed_names,
            calls: Vec::new(),
        };
        for statement in suite {
            collector.visit_stmt(statement);
        }
        self.calls.extend(collector.calls);
    }

    fn collect_decorator(&mut self, enclosing: &str, decorator: &ast::Decorator) {
        let target = self
            .strato_decorator_target(&decorator.expression)
            .unwrap_or_else(|| match &decorator.expression {
                Expr::Call(call) => self
                    .facade
                    .resolve_call_target(self.file, call)
                    .unwrap_or(ResolvedTarget::Unknown),
                expression => self
                    .facade
                    .resolve_callable_reference(self.file, expression)
                    .unwrap_or(ResolvedTarget::Unknown),
            });
        self.calls.push(AdapterCallSemantic {
            enclosing_qualified_name: Some(enclosing.to_owned()),
            expression: snippet(self.source, decorator.expression.range()),
            target,
            is_event_loop_run_in_executor: false,
            location: SourceLocation::from_range(decorator.expression.range()),
        });
    }

    fn record_annotation_imports<'a>(
        &mut self,
        aliases: impl Iterator<Item = (&'a str, Option<&'a str>)>,
    ) {
        for (module, alias) in aliases {
            if module == "strato" || module == "strato._annotations" {
                let local_name =
                    alias.unwrap_or(module.rsplit('.').next().expect("module has a name"));
                self.strato_annotation_aliases
                    .insert(local_name.to_owned(), module.to_owned());
            }
        }
    }

    fn record_annotation_import_from<'a>(
        &mut self,
        module: &str,
        aliases: impl Iterator<Item = (&'a str, Option<&'a str>)>,
    ) {
        if module != "strato" && module != "strato._annotations" {
            return;
        }
        for (name, alias) in aliases {
            if is_strato_annotation_name(name) {
                self.strato_annotation_aliases
                    .insert(alias.unwrap_or(name).to_owned(), format!("{module}.{name}"));
            }
        }
    }

    fn strato_decorator_target(&self, expression: &Expr) -> Option<ResolvedTarget> {
        let candidate = match expression {
            Expr::Call(call) => call.func.as_ref(),
            expression => expression,
        };
        let qualified_name = match candidate {
            Expr::Name(name) => self
                .strato_annotation_aliases
                .get(name.id.as_str())?
                .clone(),
            Expr::Attribute(attribute) if is_strato_annotation_name(attribute.attr.as_str()) => {
                let Expr::Name(module_alias) = attribute.value.as_ref() else {
                    return None;
                };
                let module = self
                    .strato_annotation_aliases
                    .get(module_alias.id.as_str())?;
                if module != "strato" && module != "strato._annotations" {
                    return None;
                }
                format!("{module}.{attr}", attr = attribute.attr.as_str())
            }
            _ => return None,
        };
        Some(ResolvedTarget::ExternalQualifiedNames(BTreeSet::from([
            qualified_name,
        ])))
    }
}

fn is_strato_annotation_name(name: &str) -> bool {
    matches!(name, "blocking" | "non_blocking" | "unblocker")
}

impl StratoTyFacade {
    fn stub_blocking_function_targets(&self) -> BTreeSet<String> {
        let mut targets = BTreeSet::new();
        for file in self.files().into_iter().filter(FileInfo::is_stub) {
            let Ok(Some(module)) = self.parsed_module(file.id()) else {
                continue;
            };
            let Some(project_file) = self.project.project_file(file.id()) else {
                continue;
            };
            let source = source_text(self.project.db(), project_file.raw);
            let module_name = module_name_for_stub_path(file.path());
            for statement in module.suite() {
                let Stmt::FunctionDef(function) = statement else {
                    continue;
                };
                if function.decorator_list.iter().any(|decorator| {
                    snippet(&source, decorator.range())
                        .trim()
                        .trim_start_matches('@')
                        .trim()
                        == "blocking"
                }) {
                    targets.insert(format!("{module_name}.{}", function.name.as_str()));
                }
            }
        }
        targets
    }

    fn stub_blocking_member_targets(&self) -> BTreeMap<String, DefinitionKey> {
        let mut targets = BTreeMap::new();
        for file in self.files().into_iter().filter(FileInfo::is_stub) {
            let Ok(Some(module)) = self.parsed_module(file.id()) else {
                continue;
            };
            let Some(project_file) = self.project.project_file(file.id()) else {
                continue;
            };
            let source = source_text(self.project.db(), project_file.raw);
            let module_name = module_name_for_stub_path(file.path());
            for statement in module.suite() {
                let Stmt::ClassDef(class) = statement else {
                    continue;
                };
                for member in &class.body {
                    let Stmt::FunctionDef(function) = member else {
                        continue;
                    };
                    if function.decorator_list.iter().any(|decorator| {
                        snippet(&source, decorator.range())
                            .trim()
                            .trim_start_matches('@')
                            .trim()
                            == "blocking"
                    }) {
                        targets.insert(
                            function.name.as_str().to_owned(),
                            DefinitionKey::new(format!(
                                "{module_name}.{}.{name}",
                                class.name.as_str(),
                                name = function.name.as_str()
                            )),
                        );
                    }
                }
            }
        }
        targets
    }
}

fn module_name_for_stub_path(path: &std::path::Path) -> String {
    path.file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default()
        .to_owned()
}

struct SemanticCallCollector<'facade, 'source> {
    facade: &'facade StratoTyFacade,
    file: FileId,
    source: &'source str,
    enclosing_qualified_name: Option<String>,
    member_targets: &'facade BTreeMap<String, DefinitionKey>,
    imported_symbols: &'facade BTreeMap<String, String>,
    module_imports: &'facade BTreeSet<String>,
    shadowed_names: &'facade BTreeSet<String>,
    calls: Vec<AdapterCallSemantic>,
}

impl<'a> SourceOrderVisitor<'a> for SemanticCallCollector<'_, '_> {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        match node {
            AnyNodeRef::ExprCall(call) => {
                let expression = snippet(self.source, call.range());
                let target = self
                    .synthetic_call_target(expression.as_str())
                    .unwrap_or_else(|| {
                        self.facade
                            .resolve_call_target(self.file, call)
                            .unwrap_or(ResolvedTarget::Unknown)
                    });
                let is_event_loop_run_in_executor = self
                    .facade
                    .resolves_to_event_loop_run_in_executor(self.file, call)
                    .unwrap_or(false);
                self.push_call(call.range(), target, is_event_loop_run_in_executor);
            }
            AnyNodeRef::ExprAttribute(attribute) if !self.attribute_is_call_function(attribute) => {
                let target = self
                    .synthetic_attribute_target(attribute.attr.as_str())
                    .or_else(|| self.imported_module_attribute_target(attribute))
                    .unwrap_or_else(|| {
                        self.facade
                            .resolve_property_getter(self.file, attribute)
                            .unwrap_or(ResolvedTarget::Unknown)
                    });
                self.push_call_with_expression(
                    attribute.range(),
                    attribute.attr.range(),
                    target,
                    false,
                );
            }
            AnyNodeRef::ExprBinOp(binary) => {
                if let Some(target) = self.synthetic_attribute_target("__add__") {
                    self.push_call(binary.range(), target, false);
                } else {
                    for target in self
                        .facade
                        .resolve_dunder_target(self.file, DunderOperation::Binary(binary))
                        .unwrap_or_default()
                    {
                        self.push_call(binary.range(), target, false);
                    }
                }
            }
            AnyNodeRef::ExprCompare(compare) => {
                if let Some(target) = self.synthetic_attribute_target("__lt__") {
                    self.push_call(compare.range(), target, false);
                }
            }
            AnyNodeRef::ExprFString(fstring) => {
                if let Some(target) = self.synthetic_attribute_target("__format__") {
                    self.push_call(fstring.range(), target, false);
                }
            }
            AnyNodeRef::ExprSubscript(subscript) => {
                if let Some(target) = self.synthetic_attribute_target("__getitem__") {
                    self.push_call(subscript.range(), target, false);
                }
            }
            AnyNodeRef::StmtWith(with_stmt) => {
                if let Some(target) = self.synthetic_attribute_target("__enter__") {
                    self.push_call(with_stmt.range(), target, false);
                }
            }
            AnyNodeRef::StmtFor(for_stmt) => {
                if let Some(target) = self.synthetic_attribute_target("__iter__") {
                    self.push_call(for_stmt.range(), target, false);
                }
            }
            AnyNodeRef::ExprUnaryOp(unary) => {
                for target in self
                    .facade
                    .resolve_dunder_target(self.file, DunderOperation::Unary(unary))
                    .unwrap_or_default()
                {
                    self.push_call(unary.range(), target, false);
                }
            }
            _ => {}
        }
        TraversalSignal::Traverse
    }
}

impl SemanticCallCollector<'_, '_> {
    fn push_call_with_expression(
        &mut self,
        expression_range: ruff_text_size::TextRange,
        location_range: ruff_text_size::TextRange,
        target: ResolvedTarget,
        is_event_loop_run_in_executor: bool,
    ) {
        self.calls.push(AdapterCallSemantic {
            enclosing_qualified_name: self.enclosing_qualified_name.clone(),
            expression: snippet(self.source, expression_range),
            target,
            is_event_loop_run_in_executor,
            location: SourceLocation::from_range(location_range),
        });
    }

    fn push_call(
        &mut self,
        range: ruff_text_size::TextRange,
        target: ResolvedTarget,
        is_event_loop_run_in_executor: bool,
    ) {
        if target.is_unknown() {
            return;
        }
        self.calls.push(AdapterCallSemantic {
            enclosing_qualified_name: self.enclosing_qualified_name.clone(),
            expression: snippet(self.source, range),
            target,
            is_event_loop_run_in_executor,
            location: SourceLocation::from_range(range),
        });
    }

    fn synthetic_attribute_target(&self, attr: &str) -> Option<ResolvedTarget> {
        self.member_targets.get(attr).cloned().map(|target| {
            if target.as_str().contains(':') {
                ResolvedTarget::FirstPartyDefinition(target)
            } else {
                ResolvedTarget::ExternalQualifiedNames(BTreeSet::from([target.as_str().to_owned()]))
            }
        })
    }

    fn synthetic_call_target(&self, expression: &str) -> Option<ResolvedTarget> {
        let callee = expression.split_once('(')?.0.trim();
        if callee.chars().next().is_some_and(char::is_uppercase) {
            return None;
        }
        if let Some((module, attr)) = callee.split_once('.')
            && self.module_imports.contains(module)
            && !self.shadowed_names.contains(module)
        {
            return Some(ResolvedTarget::ExternalQualifiedNames(BTreeSet::from([
                format!("{module}.{attr}"),
            ])));
        }
        if !callee.contains('.')
            && !self.shadowed_names.contains(callee)
            && let Some(target) = self.imported_symbols.get(callee)
        {
            return Some(ResolvedTarget::ExternalQualifiedNames(BTreeSet::from([
                target.clone(),
            ])));
        }
        let member = if callee == "str" {
            "__str__"
        } else if callee.contains('.') {
            callee.rsplit('.').next()?
        } else {
            "__call__"
        };
        self.synthetic_attribute_target(member)
    }

    fn imported_module_attribute_target(
        &self,
        attribute: &ast::ExprAttribute,
    ) -> Option<ResolvedTarget> {
        let Expr::Name(module) = attribute.value.as_ref() else {
            return None;
        };
        let module = module.id.as_str();
        if !self.module_imports.contains(module) || self.shadowed_names.contains(module) {
            return None;
        }
        Some(ResolvedTarget::ExternalQualifiedNames(BTreeSet::from([
            format!("{module}.{attr}", attr = attribute.attr.as_str()),
        ])))
    }

    fn attribute_is_call_function(&self, attribute: &ast::ExprAttribute) -> bool {
        let end = usize::from(attribute.range().end());
        self.source[end..].chars().find(|ch| !ch.is_whitespace()) == Some('(')
    }
}

fn decorators(source: &str, decorators: &[ast::Decorator]) -> Vec<String> {
    decorators
        .iter()
        .map(|decorator| snippet(source, decorator.expression.range()))
        .collect()
}

fn qualify(scope: Option<&str>, name: &str) -> String {
    scope.map_or_else(|| name.to_owned(), |scope| format!("{scope}.{name}"))
}

fn snippet(source: &str, range: ruff_text_size::TextRange) -> String {
    source[usize::from(range.start())..usize::from(range.end())]
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::DunderOperation;
    use ruff_python_ast::{Expr, Stmt};

    fn facade_with_source(source: &str) -> (tempfile::TempDir, StratoTyFacade, FileId) {
        let tempdir = tempfile::tempdir().expect("tempdir");
        std::fs::write(tempdir.path().join("sample.py"), source).expect("write source");
        let project = StratoProject::from_root(tempdir.path()).expect("project setup");
        let facade = StratoTyFacade::new(project);
        let file = facade.files()[0].id();
        (tempdir, facade, file)
    }

    #[test]
    fn facade_resolves_direct_call_callable_reference_property_dunder_and_executor_paths() {
        let (_tempdir, facade, file) = facade_with_source(
            r"
def blocking():
    return 1

class Box:
    @property
    def value(self):
        return 1

    def __add__(self, other):
        return self

    def run_in_executor(self, executor, callback):
        return callback()

def main():
    box = Box()
    blocking()
    blocking
    box.value
    box + box
    box.run_in_executor(None, blocking)
",
        );
        let parsed = facade.parsed_module(file).unwrap().unwrap();
        let main = parsed
            .suite()
            .iter()
            .find_map(|stmt| match stmt {
                Stmt::FunctionDef(function) if function.name.as_str() == "main" => Some(function),
                _ => None,
            })
            .expect("main function");
        let blocking_call = expr_stmt(&main.body[1])
            .as_call_expr()
            .expect("blocking call");
        let alias_ref = expr_stmt(&main.body[2]);
        let property_attr = expr_stmt(&main.body[3])
            .as_attribute_expr()
            .expect("property attr");
        let dunder = expr_stmt(&main.body[4]).as_bin_op_expr().expect("bin op");
        let executor_call = expr_stmt(&main.body[5])
            .as_call_expr()
            .expect("executor call");

        let call_target = facade.resolve_call_target(file, blocking_call).unwrap();
        assert!(matches!(
            call_target,
            ResolvedTarget::FirstPartyDefinition(_)
        ));

        let callable_target = facade.resolve_callable_reference(file, alias_ref).unwrap();
        assert!(!callable_target.is_unknown());

        let attribute_target = facade
            .resolve_attribute_target(file, property_attr)
            .unwrap();
        assert!(!attribute_target.is_unknown());

        let property_target = facade.resolve_property_getter(file, property_attr).unwrap();
        assert!(!property_target.is_unknown());

        let dunder_targets = facade
            .resolve_dunder_target(file, DunderOperation::Binary(dunder))
            .unwrap();
        assert!(!dunder_targets.is_empty());

        assert!(
            !facade
                .resolves_to_event_loop_run_in_executor(file, executor_call)
                .unwrap()
        );
    }

    #[test]
    fn unresolved_targets_are_none() {
        let (_tempdir, facade, file) = facade_with_source(
            r"
def main(dynamic):
    dynamic()
    dynamic.attr
    dynamic + dynamic
",
        );
        let parsed = facade.parsed_module(file).unwrap().unwrap();
        let main = parsed.suite()[0].as_function_def_stmt().expect("main");
        let dynamic_call = expr_stmt(&main.body[0])
            .as_call_expr()
            .expect("dynamic call");
        let dynamic_attr = expr_stmt(&main.body[1])
            .as_attribute_expr()
            .expect("dynamic attr");
        let dynamic_dunder = expr_stmt(&main.body[2])
            .as_bin_op_expr()
            .expect("dynamic dunder");

        assert!(
            facade
                .resolve_call_target(file, dynamic_call)
                .unwrap()
                .is_unknown()
        );
        assert!(
            facade
                .resolve_property_getter(file, dynamic_attr)
                .unwrap()
                .is_unknown()
        );
        assert!(
            facade
                .resolve_dunder_target(file, DunderOperation::Binary(dynamic_dunder))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn facade_resolves_module_attribute_call_to_external_qualified_alias() {
        let (_tempdir, facade, file) = facade_with_source(
            r"
import time

async def main():
    time.sleep(1)
",
        );
        let parsed = facade.parsed_module(file).unwrap().unwrap();
        let main = parsed.suite()[1].as_function_def_stmt().expect("main");
        let sleep_call = expr_stmt(&main.body[0]).as_call_expr().expect("sleep call");

        let target = facade.resolve_call_target(file, sleep_call).unwrap();

        assert!(matches!(
            target,
            ResolvedTarget::ExternalQualifiedNames(names) if names.contains("time.sleep")
        ));
    }

    #[test]
    fn facade_emits_property_access_semantic_fact() {
        let (_tempdir, facade, file) = facade_with_source(
            r"
class DataFetcher:
    @property
    def data(self):
        return 1

async def handler():
    fetcher = DataFetcher()
    result = fetcher.data
",
        );

        let facts = facade.semantic_facts_in_file(file).unwrap();
        assert!(facts.calls.iter().any(|call| {
            call.enclosing_qualified_name.as_deref() == Some("handler")
                && call.expression == "fetcher.data"
                && matches!(call.target, ResolvedTarget::FirstPartyDefinition(ref key) if key.as_str().ends_with(":DataFetcher.data"))
        }));
    }

    #[test]
    fn recoverable_errors_are_representable_without_panic() {
        let (_tempdir, facade, _file) = facade_with_source("def f(): pass");
        let Err(error) = facade.parsed_module(FileId::new(99)) else {
            panic!("unknown file should be recoverable");
        };
        assert!(matches!(error, FacadeError::UnknownFile(_)));
    }

    fn expr_stmt(stmt: &Stmt) -> &Expr {
        stmt.as_expr_stmt()
            .expect("expression statement")
            .value
            .as_ref()
    }
}
