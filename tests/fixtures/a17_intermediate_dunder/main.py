import requests

class RemoteObject:
    def __str__(self):
        return load_remote()

def load_remote():
    return requests.get("https://api.example.com/status").text

def helper(obj):
    return str(obj)

async def handler():
    obj = RemoteObject()
    helper(obj)
