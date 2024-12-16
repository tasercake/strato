import os
import pathlib
from collections import deque
from typing import Generator, TypeVar

import tree_sitter_python as tspython
from tree_sitter import Language, Tree, Node, Parser

PY_LANGUAGE = Language(tspython.language())
parser = Parser(PY_LANGUAGE)


# Function to parse a single Python file into a tree-sitter AST
def parse_file(filename) -> Tree:
    with open(filename, "r") as file:
        file_content = file.read()
    return parser.parse(bytes(file_content, "utf8"))


def depth_first_traverse(tree: Tree) -> Generator[Node, None, None]:
    q: deque[Node] = deque([tree.root_node])
    while q:
        node = q.pop()
        yield node
        q.extend(reversed(node.children))


def level_order_traverse(tree: Tree) -> Generator[Node, None, None]:
    q: deque[Node] = deque([tree.root_node])
    while q:
        node = q.popleft()
        yield node
        q.extend(node.children)


PathLike = TypeVar("PathLike", str, pathlib.Path, None)


def analyze(entrypoint: str):
    entry_file_str: str
    match entrypoint:
        case str():
            entry_file_str = os.path.abspath(entrypoint)
        case pathlib.Path():
            entry_file_str = entrypoint.resolve().as_posix()
        case None:
            raise ValueError("Entry file must be provided")

    tree = parse_file(entry_file_str)
    for node in depth_first_traverse(tree):
        if node.type == "import_from_statement":
            print(f"# {node.type}")
            if node.text:
                print(node.text.decode())
            for child in node.children:
                print(child.type, child.text)
                if child.type in "dotted_name":
                    print(child.type, child.text)
                if child.type == "aliased_import":
                    for c in child.children:
                        print(c.type, c.text)
        if node.type == "import_statement":
            print(f"# {node.type}")
            if node.text:
                print(node.text.decode())
            for child in node.children:
                print(child.type, child.text)
                if child.type in "dotted_name":
                    print(child.type, child.text)
                if child.type == "aliased_import":
                    for c in child.children:
                        print(c.type, c.text)
