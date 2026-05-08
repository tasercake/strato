from strato import blocking

class Client:
    @blocking
    def fetch(self) -> None: ...
