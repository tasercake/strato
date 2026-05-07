import time


class CallableWorker:
    def __call__(self):
        time.sleep(1)


async def handler():
    worker = CallableWorker()
    worker()
