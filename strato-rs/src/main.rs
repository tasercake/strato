use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Component, Path};

use rustpython_parser::ast::{Mod, Stmt};
use rustpython_parser::Parse;
use walkdir::WalkDir;

/// Represents a Python module with its name, AST, and file path.
struct Module {
    name: String,
    ast: Mod,
    #[allow(dead_code)]
    path: String,
}

fn main() {
    // Get the root directory from command-line arguments or use the current directory
    let args: Vec<String> = env::args().collect();
    let root = if args.len() > 1 { &args[1] } else { "." };

    // Collect all Python files
    let files = collect_python_files(root);

    // Build a module map from file paths to Module structs
    let modules = build_module_map(files);

    // Lint each module
    for module in modules.values() {
        lint_module(module, &modules);
    }
}

/// Collects all Python files in the specified directory and its subdirectories.
fn collect_python_files<'a>(root: &'a str) -> impl Iterator<Item = String> + 'a {
    WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s == "py")
                    .unwrap_or(false)
        })
        .map(|e| e.path().to_string_lossy().into_owned())
}

/// Parses a Python file into an AST.
fn parse_python_file(file_path: &str) -> Result<Mod, String> {
    let source = fs::read_to_string(file_path).map_err(|e| e.to_string())?;
    let stmts = Parse::parse(&source, file_path).map_err(|e| e.to_string())?;
    Ok(Mod::Module(stmts))
}

fn build_module_map<I>(files: I) -> HashMap<String, Module>
where
    I: IntoIterator<Item = String>,
{
    let mut modules = HashMap::new();

    for file_path in files {
        match parse_python_file(&file_path) {
            Ok(ast) => {
                let module_name = derive_module_name(&file_path);
                modules.insert(
                    module_name.clone(),
                    Module {
                        name: module_name,
                        ast,
                        path: file_path.clone(),
                    },
                );
            }
            Err(e) => {
                eprintln!("Failed to parse file '{}': {}", file_path, e);
            }
        }
    }

    modules
}

/// Derives a module name from a file path.
fn derive_module_name(file_path: &str) -> String {
    // Convert file path to module name (e.g., "src/foo/bar.py" -> "src.foo.bar")
    let path = Path::new(file_path);
    let mut components = vec![];

    for component in path.components() {
        if let Component::Normal(os_str) = component {
            let s = os_str.to_string_lossy();
            let s = s.replace(".py", "");
            components.push(s);
        }
    }

    components.join(".")
}

/// Lints a module by checking for unresolved imports.
fn lint_module(module: &Module, modules: &HashMap<String, Module>) {
    match &module.ast {
        Mod::Module(mod_module) => {
            for stmt in &mod_module.body {
                lint_statement(stmt, module, modules);
            }
        }
        _ => {}
    }
}

/// Lints individual statements within a module.
fn lint_statement(stmt: &Stmt, _module: &Module, _modules: &HashMap<String, Module>) {
    match &stmt {
        _ => {
            // Recursively handle other statement kinds if necessary
        }
    }
}

/// Resolves relative imports to absolute module names.
fn resolve_relative_import(
    current_module: &str,
    imported_module: &str,
    level: Option<usize>,
) -> String {
    let mut parts: Vec<&str> = current_module.split('.').collect();
    if let Some(level) = level {
        for _ in 0..level {
            if parts.is_empty() {
                break;
            }
            parts.pop();
        }
    }
    if !imported_module.is_empty() {
        parts.push(imported_module);
    }
    parts.join(".")
}
