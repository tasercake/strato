import time

class Safe:
    def sleep(self, seconds):
        pass

time = Safe()

async def handler():
    time.sleep(1)
