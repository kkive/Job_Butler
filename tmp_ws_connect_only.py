import websocket
try:
    ws = websocket.create_connection('ws://127.0.0.1:55002/ws/logs', timeout=3)
    print('connected')
    ws.close()
except Exception as e:
    print('connect_err', type(e).__name__, e)
