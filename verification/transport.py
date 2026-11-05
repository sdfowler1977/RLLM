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
