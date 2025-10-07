import asyncio
import websockets

async def hello():
    async with websockets.connect("wss://6946-85-65-157-57.eu.ngrok.io") as websocket:
        # await websocket.send(input("client id: "))
        # await websocket.send(input("your message: "))
        async for message in websocket:
            print(message)

asyncio.run(hello())