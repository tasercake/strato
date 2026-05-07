import time


class BlockingValue:
    def __add__(self, other):
        time.sleep(1)
        return self

    def __lt__(self, other):
        time.sleep(1)
        return False

    def __format__(self, spec):
        time.sleep(1)
        return "value"

    def __getitem__(self, key):
        time.sleep(1)
        return self

    def __enter__(self):
        time.sleep(1)
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def __iter__(self):
        time.sleep(1)
        return iter(())


async def add_handler():
    value = BlockingValue()
    value + 1


async def compare_handler():
    value = BlockingValue()
    value < 1


async def format_handler():
    value = BlockingValue()
    f"{value}"


async def subscript_handler():
    value = BlockingValue()
    value[0]


async def context_handler():
    value = BlockingValue()
    with value:
        pass


async def iteration_handler():
    value = BlockingValue()
    for _ in value:
        pass
