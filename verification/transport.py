"""Exercise the same bounded request-chunk API as the platform socket binding."""
import base64

def upload(op,e,sock):
    if not e.get('media_upload'):return
    owner=dict(purpose=e['purpose'],ticket=e['media_upload']);count=0
    try:
        while True:
            part=op(dict(owner,op='media_upload_read'));data=base64.b64decode(part['data']);assert len(data)<=8192
            count+=len(data);assert count<=e['upload_bytes'];sock.sendall(data)
            if part['done']:
                assert count==e['upload_bytes'];return
            assert data
    finally:op(dict(owner,op='media_upload_cancel'))

def response(op,e,sock):
    if not e.get('media_download'):
        data=b''
        while chunk:=sock.recv(8192):data+=chunk
        return dict(data=base64.b64encode(data).decode())
    owner=dict(purpose=e['purpose'],ticket=e['media_download']);ready=False
    try:
        while chunk:=sock.recv(8192):
            ready=op(dict(owner,op='media_stream_feed',data=base64.b64encode(chunk).decode()))['ready']
            if ready:break
        return dict(media_receipt=op(dict(owner,op='media_stream_finish',eof=not ready)))
    finally:op(dict(owner,op='media_stream_cancel'))
