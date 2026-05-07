import time
from strato import non_blocking

@non_blocking
def actually_safe():
    time.sleep(1)

async def handler():
    actually_safe()
