import time
import mylib

def slow():
    time.sleep(1)

async def handler():
    await mylib.offload(func=slow)
