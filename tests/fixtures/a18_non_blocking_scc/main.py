import time
from strato import non_blocking

@non_blocking
def safe_entry(flag):
    if flag:
        unsafe_peer()

def unsafe_peer():
    safe_entry(False)
    time.sleep(1)

async def safe_handler():
    safe_entry(True)

async def unsafe_handler():
    unsafe_peer()
