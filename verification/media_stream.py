"""Real HTTP artifact URLs, bounded streaming, ownership and output integrity."""
import base64,copy,hashlib,io,json,pathlib,socket,subprocess,tempfile,threading,wave
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[];calls=[];target='';payload=io.BytesIO()
with wave.open(payload,'wb') as w:w.setnchannels(1);w.setsampwidth(2);w.setframerate(16000);w.writeframes(b'\0\0'*4096)
audio=payload.getvalue()
def check(name,value):assert value,name;passed.append(name)
class Peer(BaseHTTPRequestHandler):
    def log_message(self,*a):pass
    def do_GET(self):
        calls.append((self.server.server_port,self.path,self.headers.get('X-Key')))
        body=audio if self.path=='/file'else b'x'*(1024*1024+1)
        self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers()
        try:self.wfile.write(body)
        except (BrokenPipeError,ConnectionResetError):pass
    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length','0')));calls.append((self.server.server_port,self.path,self.headers.get('X-Key')))
        body=json.dumps({'url':target}).encode();self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
servers=[ThreadingHTTPServer(('127.0.0.1',0),Peer) for _ in range(2)]
for server in servers:threading.Thread(target=server.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='stream-',dir=PROJECT/'output') as temp:
    root=pathlib.Path(temp);p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def raw(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
    def op(v):r=raw(v);assert 'value'in r,r;return r['value']
    def event(st,a,i=None):return op(dict(op='event',state=st,action=a,inputs=i or {},root=temp))
    def start(response='artifact_url',path='/generate',kind='audio',limit=1):
        cfg=dict(version=1,hosts=[dict(id=f'h{i}',url=f'http://127.0.0.1:{s.server_port}',enabled=True,plain=True,auth_header='X-Key',key=f'key{i}') for i,s in enumerate(servers)],routes=[dict(id='generate',capability='speech_synthesis',host='h0',path=path,method='POST'if response=='artifact_url'else'GET',response=response,response_path='/url'if response=='artifact_url'else'',artifact_kind=kind,artifact_host='h1',artifact_limit_mb=limit,enabled=True)],defaults={})
        d=event(None,'resume');d=event(d['state'],'service_connections');d=event(d['state'],'service_save',dict(service_config=json.dumps(cfg)));assert not d['state'].get('notice_error'),d['state'].get('notice')
        d=event(d['state'],'service_request');d=event(d['state'],'service_review',dict(service_route='generate',service_input='{}'));return event(d['state'],'approve')
    def owner(e):return dict(purpose=e['purpose'],ticket=e['media_download'])
    def wire(e):return base64.b64decode(e['wire_base64'])if'wire_base64'in e else e['wire'].encode()
    def exchange(d):
        e=d['effect'];out=b''
        with socket.create_connection((e['host'],e['port']),timeout=5)as sock:
            sock.sendall(wire(e))
            while chunk:=sock.recv(8192):out+=chunk
        return event(d['state'],'network_result',dict(purpose=e['purpose'],data=base64.b64encode(out).decode()))
    def feed(d,wire_bytes=None):
        e=d['effect'];o=owner(e);ready=False
        def part(data):return op(dict(o,op='media_stream_feed',data=base64.b64encode(data).decode()))['ready']
        if wire_bytes is not None:
            for i in range(0,len(wire_bytes),8192):ready=part(wire_bytes[i:i+8192])
        else:
            with socket.create_connection((e['host'],e['port']),timeout=5)as sock:
                sock.sendall(wire(e))
                while data:=sock.recv(8192):
                    ready=part(data)
                    if ready:break
        return op(dict(o,op='media_stream_finish',eof=not ready))
    def settle(d,receipt):return event(d['state'],'network_result',dict(purpose=d['effect']['purpose'],media_receipt=receipt))
    def cancel(d):op(dict(owner(d['effect']),op='media_stream_cancel'));return event(d['state'],'cancel')
    try:
        target=f'http://127.0.0.1:{servers[1].server_port}/file';d=exchange(start());check('artifact URL creates a separate bounded GET',d['effect'].get('media_download')and d['effect']['port']==servers[1].server_port and wire(d['effect']).startswith(b'GET /file '))
        check('download credentials belong only to artifact host',b'key1'in wire(d['effect'])and b'key0'not in wire(d['effect']))
        o=owner(d['effect']);op(dict(o,op='media_stream_cancel',ticket='wrong'));receipt=feed(d);done=settle(d,receipt);record=done['state']['service_artifact'];check('wrong ticket cancel cannot stop active stream',record['sha256']==hashlib.sha256(audio).hexdigest())
        check('stream adopts complete actual bytes outside conversation state',(root/record['path']).read_bytes()==audio and len(json.dumps(done['state']))<100000 and base64.b64encode(audio).decode()not in json.dumps(done['state']))
        duplicate=settle(dict(d,state=done['state']),receipt);check('duplicate completed callback does not alter state',duplicate['state']==done['state'])
        before=len(calls);target='http://outside.invalid/file';blocked=exchange(start());check('unconfigured artifact origin is refused before download',blocked['effect']is None and blocked['state']['service_result']['status']=='failed'and len(calls)==before+1)
        target=f'http://127.0.0.1:{servers[1].server_port}/file';d=exchange(start());o=owner(d['effect']);bad=raw(dict(o,op='media_stream_feed',purpose='wrong',data=base64.b64encode(b'x').decode()));check('wrong stream purpose cannot feed bytes','value'not in bad);cancel(d)
        d=exchange(start());o=owner(d['effect']);bad=raw(dict(o,op='media_stream_feed',data=base64.b64encode(b'x'*8193).decode()));check('oversized native chunks are refused','value'not in bad);cancel(d)
        d=exchange(start());o=owner(d['effect']);op(dict(o,op='media_stream_feed',data=base64.b64encode(b'HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort').decode()));bad=raw(dict(o,op='media_stream_finish',eof=True));check('truncated response cannot become a receipt','value'not in bad);cancel(d)
        d=exchange(start());o=owner(d['effect']);bad=raw(dict(o,op='media_stream_feed',data=base64.b64encode(b'HTTP/1.1 302 Found\r\nContent-Length: 0\r\n\r\n').decode()));check('stream never follows redirects or replays generation','value'not in bad);cancel(d)
        target=f'http://127.0.0.1:{servers[1].server_port}/large';d=exchange(start(kind='data'))
        try:feed(d);raise AssertionError('large stream was accepted')
        except AssertionError as error:check('configured file size bound is enforced','bound' in str(error))
        cancel(d)
        target=f'http://127.0.0.1:{servers[1].server_port}/file';d=exchange(start());receipt=feed(d);path=max((root/'service-files').iterdir(),key=lambda f:f.stat().st_mtime_ns);original=path.read_bytes();path.write_bytes(bytes([original[0]^1])+original[1:]);done=settle(d,receipt);check('same length disk mutation invalidates ready receipt',done['state']['service_result']['status']=='failed'and not done['state'].get('service_artifact'))
        d=exchange(start());receipt=feed(d);stopped=event(d['state'],'cancel');late=settle(dict(d,state=stopped['state']),receipt);check('cancelled artifact callback is an exact state no-op',late['state']==stopped['state'])
    finally:p.stdin.close();p.wait(timeout=10)
for server in servers:server.shutdown();server.server_close()
report=dict(passed=len(passed),checks=passed,scope='Owner public-core and real local HTTP checks; Android storage/decoder and live configured endpoints need device acceptance.');(PROJECT/'output/checks/media_stream.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
