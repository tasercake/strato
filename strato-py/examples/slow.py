import asyncio
import time


async def foo():
    # An async function that calls a blocking stdlib function
    time.sleep(1)


async def main():
    await foo()


if __name__ == "__main__":
    asyncio.run(main(), debug=True)
