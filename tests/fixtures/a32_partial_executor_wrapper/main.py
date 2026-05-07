import asyncio
import time
from functools import partial

async def handler():
    loop = asyncio.get_running_loop()
    await loop.run_in_executor(None, partial(time.sleep, 1))
