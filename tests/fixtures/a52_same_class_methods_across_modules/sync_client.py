import time


class ServiceClient:
    def execute(self):
        return self._execute()

    def _execute(self):
        time.sleep(1)


class Manager:
    def initialize(self):
        client = ServiceClient()
        _ = client.execute()
