import time

class Worker:
    def slow(self):
        time.sleep(1)

def use_worker(worker: Worker):
    worker.slow()

async def handler(worker: Worker):
    use_worker(worker)
