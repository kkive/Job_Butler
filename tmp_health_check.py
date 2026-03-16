import requests
try:
    r=requests.get('http://127.0.0.1:55002/health',timeout=2)
    print('status',r.status_code,'body',r.text)
except Exception as e:
    print('err',e)
