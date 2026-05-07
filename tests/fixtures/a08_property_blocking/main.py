import requests

class DataFetcher:
    @property
    def data(self):
        return requests.get("https://api.example.com/data").json()

async def handler():
    fetcher = DataFetcher()
    result = fetcher.data
