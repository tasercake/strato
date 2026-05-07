import asyncio
import time

def helper():
    time.sleep(1)

async def safe_caller():
    await asyncio.to_thread(helper)

async def unsafe_caller():
    helper()
