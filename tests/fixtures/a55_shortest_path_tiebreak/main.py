import io
import os
import time

async def handler():
    helper()
    time.sleep(1)
    os.listdir(".")
    os.listdir("/")

def helper():
    io.open("example.txt")
