import time
from mylib import offload as run_safe

async def handler():
    run_safe(time.sleep, 1)
