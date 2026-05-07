import requests

class RemoteObject:
    def __str__(self):
        return requests.get("https://api.example.com/status").text

async def handler():
    obj = RemoteObject()
    print(str(obj))
