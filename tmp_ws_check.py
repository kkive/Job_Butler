import asyncio
import websockets

async def main():
    try:
        async with websockets.connect('ws://127.0.0.1:55002/ws/logs') as ws:
            print('connected')
            try:
                msg = await asyncio.wait_for(ws.recv(), timeout=2)
                print('msg', msg)
            except Exception as e:
                print('recv_err', type(e).__name__, e)
    except Exception as e:
        print('connect_err', type(e).__name__, e)

asyncio.run(main())
