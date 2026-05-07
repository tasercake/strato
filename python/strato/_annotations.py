from collections.abc import Callable
from typing import TypeVar

F = TypeVar("F", bound=Callable[..., object])

def blocking(func: F) -> F:
    return func

def non_blocking(func: F) -> F:
    return func

def unblocker(func: F) -> F:
    return func
