import asyncio
import time

async def handler():
    await asyncio.to_thread(time.sleep, 1)
