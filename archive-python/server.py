import asyncio
import websockets

clients = {}

async def echo(websocket):
    # type: (websockets.websocket) -> None
    print("accepted")

    conv_id = await websocket.recv()
    print("conversation id: %s" % conv_id)

    conv_clients = clients.setdefault(conv_id, [])

    # means there would be three clients, which doesn't make sense
    if len(conv_clients) > 1:
        websocket.close()
        return

    conv_clients.append(websocket)
    print("conversation has %d members" % len(conv_clients))

    async for message in websocket:
        tasks = []
        print(f"received {message}")
        conv_clients_to_remove = []
        for target_sock in conv_clients:
            if target_sock is not websocket:
                try:
                    await target_sock.send(message)
                except:
                    print("removing a client")
                    conv_clients_to_remove.append(target_sock)

        for to_remove in conv_clients_to_remove:
            conv_clients.remove(to_remove)

async def main():
    async with websockets.serve(echo, "0.0.0.0", 8765):
        await asyncio.Future()  # run forever

asyncio.run(main())