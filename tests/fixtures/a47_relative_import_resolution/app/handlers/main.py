from ..services.worker import slow

async def handler():
    slow()
