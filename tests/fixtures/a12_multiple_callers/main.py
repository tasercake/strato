import time

def helper():
    time.sleep(1)

async def handler_a():
    helper()

async def handler_b():
    helper()
