import time


class Worker:
    def instance_slow(self):
        time.sleep(1)

    @staticmethod
    def static_slow():
        time.sleep(1)

    @classmethod
    def class_slow(cls):
        time.sleep(1)


async def instance_handler():
    worker = Worker()
    worker.instance_slow()


async def static_handler():
    Worker.static_slow()


async def class_handler():
    Worker.class_slow()
