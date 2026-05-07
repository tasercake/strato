from strato import blocking

@blocking
def custom_slow():
    pass

async def handler():
    custom_slow()
