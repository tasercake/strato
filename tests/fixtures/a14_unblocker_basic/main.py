import asyncio
import time
from strato import unblocker

@unblocker
def my_offload(func):
    return asyncio.to_thread(func)

async def safe_handler():
    await my_offload(lambda: time.sleep(1))

async def unsafe_handler():
    time.sleep(1)
