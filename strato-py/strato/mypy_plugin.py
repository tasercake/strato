from mypy.plugin import Plugin, FunctionHook, CheckerPluginInterface
from mypy.types import Type
from mypy.nodes import FuncDef, CallExpr, SymbolTableNode, SymbolNode

# A simple set of known blocking calls by their full name (module + function).
BLOCKING_CALLS = {
    "time.sleep",
    # Add more known-blocking functions here if you want:
    # "os.read",
    # "time.wait",
}


def is_async_function(func_def: FuncDef) -> bool:
    return func_def.is_coroutine


def get_fullname(sym: SymbolNode) -> str | None:
    # Attempt to get the fully qualified name of a symbol (e.g. "time.sleep")
    if sym.fullname:
        return sym.fullname
    if sym.name and sym.module_name:
        return f"{sym.module_name}.{sym.name}"
    return None


class BlockingCallChecker(FunctionHook):
    def __init__(self, checker: CheckerPluginInterface) -> None:
        self.chk = checker

    def __call__(
        self,
        caller: FuncDef,
        args: list[Type],
        callee: Type,
        arg_kinds: list[int],
        arg_names: list[str | None],
        context: CallExpr,
    ) -> Type:
        # The "caller" is the function currently being type-checked
        # "context" is the call expression: something like "time.sleep(1)"

        # We want to know which function is being called. The call expression's .callee
        # may be a NameExpr or MemberExpr.
        callee_node = None
        if context.callee is not None and hasattr(context.callee, "node"):
            callee_node = (
                context.callee.node
            )  # This should be a SymbolNode if available

        if callee_node and isinstance(callee_node, SymbolTableNode):
            callee_node = callee_node.node

        if callee_node and hasattr(callee_node, "fullname"):
            func_fullname = get_fullname(callee_node)
            if func_fullname in BLOCKING_CALLS:
                # We have identified a call to a known blocking function.
                # Now check if we are inside an async function.
                if is_async_function(caller):
                    # Emit a warning
                    # Note: report is done via "self.chk.msg"
                    self.chk.msg.warn(
                        "Blocking call '{}' inside async function '{}'".format(
                            func_fullname, caller.name
                        ),
                        context,
                    )

        return callee  # Return the original callee type unchanged


class BlockingPlugin(Plugin):
    def get_function_hook(self, fullname: str):
        # This hook is called whenever mypy type checks a function call.
        # If `fullname` is one of our blocking calls, we return a handler
        # Otherwise return None.
        # However, we may prefer to return a handler for all calls and filter
        # inside the handler. This is simpler: always return the handler and let
        # it do filtering.
        return BlockingCallChecker


def plugin(version: str):
    # The entry point used by mypy to load the plugin
    return BlockingPlugin
