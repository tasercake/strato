import os
import ast
from collections import defaultdict
from typing import Generator, Iterable


def discover_python_files(root_dir) -> Generator[str, None, None]:
    for dirpath, _, filenames in os.walk(root_dir):
        for filename in filenames:
            if filename.endswith(".py"):
                yield os.path.join(dirpath, filename)


# Function to parse a single Python file into an AST
def parse_file(filename):
    with open(filename, "r") as file:
        file_content = file.read()
    return ast.parse(file_content, filename=filename)


# Visitor class to collect function definitions
class FunctionVisitor(ast.NodeVisitor):
    def __init__(self):
        self.functions = []

    def visit_FunctionDef(self, node):
        self.functions.append(node.name)
        self.generic_visit(node)

    def visit_AsyncFunctionDef(self, node):
        self.functions.append(node.name)
        self.generic_visit(node)


class FunctionCallVisitor(ast.NodeVisitor):
    def __init__(self):
        self.function_calls = []

    def visit_Call(self, node: ast.Call) -> None:
        if isinstance(node.func, ast.Name):
            # Direct call like `foo()`
            func_name = node.func.id
        elif isinstance(node.func, ast.Attribute):
            # Attribute call like `time.sleep()`
            func_name = node.func.attr
        else:
            # This might occur if there is some unusual construct
            func_name = "<unknown>"

        self.function_calls.append(func_name)
        self.generic_visit(node)


# Function to get function names from an AST
def get_function_defs_from_ast(tree: ast.AST):
    visitor = FunctionVisitor()
    visitor.visit(tree)
    return visitor.functions


# Function to get function names from an AST
def get_function_calls_from_ast(tree: ast.AST):
    visitor = FunctionCallVisitor()
    visitor.visit(tree)
    return visitor.function_calls


# Visitor class to collect imports
class ImportVisitor(ast.NodeVisitor):
    def __init__(self):
        self.imports = []

    def visit_Import(self, node):
        for alias in node.names:
            self.imports.append(alias.name)
        self.generic_visit(node)

    def visit_ImportFrom(self, node):
        module = node.module
        for alias in node.names:
            self.imports.append(f"{module}.{alias.name}")
        self.generic_visit(node)


# Function to get import names from an AST
def get_imports_from_ast(tree: ast.AST):
    visitor = ImportVisitor()
    visitor.visit(tree)
    return visitor.imports


# Function to gather functions and imports from multiple files
def gather_info_from_files(filenames: Iterable[str]):
    function_map = defaultdict(list)
    import_map = defaultdict(list)
    for filename in filenames:
        tree = parse_file(filename)
        function_defs = get_function_defs_from_ast(tree)
        function_calls = get_function_calls_from_ast(tree)
        imports = get_imports_from_ast(tree)
        function_map[filename] = function_defs
        import_map[filename] = imports
    return function_map, import_map


# Function to analyze the gathered information
def analyze(root_dir: str):
    filenames = discover_python_files(root_dir)
    function_map, import_map = gather_info_from_files(filenames)

    # Example check: Find unused imports
    for filename, imports in import_map.items():
        defined_functions = function_map[filename]
        for imp in imports:
            if imp not in defined_functions:
                print(f"Unused import in {filename}: {imp}")
