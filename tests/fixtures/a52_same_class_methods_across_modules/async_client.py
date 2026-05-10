class ServiceClient:
    async def execute(self):
        return await self._execute()

    async def _execute(self):
        return "ok"


class Manager:
    async def initialize(self):
        client = ServiceClient()
        return await client.execute()
