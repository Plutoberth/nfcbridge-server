import asyncio
import websockets

clients = []

async def echo(websocket):
    print("accepted")

    clients.append(websocket)

    async for message in websocket:
        tasks = []
        print(f"received from {message}")
        for target_sock in clients:
            print(f"broadcasting to {target_sock}")
            if target_sock is not websocket:
                await target_sock.send(message)

async def main():
    async with websockets.serve(echo, "localhost", 8765):
        await asyncio.Future()  # run forever

asyncio.run(main())