import time
from mylib import offload

async def handler():
    offload(time.sleep, 1)
