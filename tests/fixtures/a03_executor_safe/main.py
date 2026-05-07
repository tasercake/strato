import asyncio
import time

async def handler():
    loop = asyncio.get_event_loop()
    await loop.run_in_executor(None, time.sleep, 1)
