from typing import Annotated
import typer

from enum import StrEnum, auto

from strato.analyzers.ast_analyzer import analyze as analyze_ast
from strato.analyzers.treesitter_analyzer import analyze as analyze_treesitter


app = typer.Typer()


class AnalyzerOption(StrEnum):
    ast = auto()
    treesitter = auto()


def method_callback(value: AnalyzerOption) -> AnalyzerOption:
    if value not in AnalyzerOption:
        raise typer.BadParameter(f"Method must be one of {', '.join(AnalyzerOption)}")
    return value


@app.command()
def main(
    root: str,
    method: Annotated[
        AnalyzerOption, typer.Option(callback=method_callback)
    ] = AnalyzerOption.ast,
):
    if method == "ast":
        analyze_ast(root)
    elif method == "treesitter":
        analyze_treesitter(root)
    else:
        raise typer.BadParameter("Method must be 'ast' or 'treesitter'")
