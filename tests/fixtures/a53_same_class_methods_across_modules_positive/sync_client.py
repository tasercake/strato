class ServiceClient:
    def execute(self):
        return self._execute()

    def _execute(self):
        return "sync ok"


class Manager:
    def initialize(self):
        client = ServiceClient()
        _ = client.execute()
