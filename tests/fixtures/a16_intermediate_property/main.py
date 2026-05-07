import requests

class DataFetcher:
    @property
    def data(self):
        return load_remote()

def load_remote():
    return requests.get("https://api.example.com/data").json()

def helper(fetcher):
    return fetcher.data

async def handler():
    fetcher = DataFetcher()
    helper(fetcher)
