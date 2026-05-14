import time


class ServiceClient:
    async def execute(self):
        return await self._execute()

    async def _execute(self):
        time.sleep(1)


class Manager:
    async def initialize(self):
        client = ServiceClient()
        return await client.execute()
