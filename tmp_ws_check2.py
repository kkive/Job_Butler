import websocket
try:
    ws = websocket.create_connection('ws://127.0.0.1:55002/ws/logs', timeout=2)
    print('connected')
    ws.settimeout(2)
    print('msg', ws.recv())
    ws.close()
except Exception as e:
    print('connect_err', type(e).__name__, e)
