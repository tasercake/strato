import time

class Base:
    @property
    def slow(self):
        time.sleep(1)

class Child(Base):
    pass

async def handler(child: Child):
    child.slow
