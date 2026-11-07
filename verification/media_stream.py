"""Real HTTP artifact URLs, bounded streaming, ownership and output integrity."""
import base64,copy,hashlib,io,json,pathlib,socket,subprocess,tempfile,threading,time,wave
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from transport import upload
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[];calls=[];posts=[];target='';payload=io.BytesIO()
with wave.open(payload,'wb') as w:w.setnchannels(1);w.setsampwidth(2);w.setframerate(16000);w.writeframes(b'\0\0'*4096)
audio=payload.getvalue()
large=io.BytesIO()
with wave.open(large,'wb')as w:w.setnchannels(1);w.setsampwidth(2);w.setframerate(16000);w.writeframes(b'\0\0'*(5*1024*1024//2))
large_audio=large.getvalue()
def check(name,value):assert value,name;passed.append(name)
class Peer(BaseHTTPRequestHandler):
    def log_message(self,*a):pass
    def do_GET(self):
        calls.append((self.server.server_port,self.path,self.headers.get('X-Key')))
        body=large_audio if self.path in ['/large-file','/created-file']else audio if self.path=='/file'else b'x'*(1024*1024+1)
        self.send_response(201 if self.path=='/created-file'else 200);self.send_header('Content-Length',str(len(body)));self.end_headers()
        try:self.wfile.write(body)
        except (BrokenPipeError,ConnectionResetError):pass
    def do_POST(self):
        posts.append(self.rfile.read(int(self.headers.get('Content-Length','0'))));calls.append((self.server.server_port,self.path,self.headers.get('X-Key')))
        if self.path in ['/post-file','/post-chunked']:
            self.send_response(201);self.send_header('Transfer-Encoding','chunked')if self.path=='/post-chunked'else self.send_header('Content-Length',str(len(large_audio)));self.end_headers()
            try:
                if self.path=='/post-chunked':
                    for i in range(0,len(large_audio),8192):
                        part=large_audio[i:i+8192];self.wfile.write(f'{len(part):x}\r\n'.encode()+part+b'\r\n')
                    self.wfile.write(b'0\r\n\r\n')
                else:self.wfile.write(large_audio)
            except (BrokenPipeError,ConnectionResetError):pass
            return
        statuses={'/post-reject':503,'/post-auth':401,'/post-accepted':202,'/post-redirect':302,'/post-partial':206,'/post-truncated-error':503}
        if self.path in statuses:
            body=audio if self.path=='/post-partial'else b'controlled status';self.send_response(statuses[self.path]);self.send_header('Content-Length',str(len(body)+100 if self.path=='/post-truncated-error'else len(body)));self.end_headers();self.wfile.write(body);return
        body=json.dumps({'url':target}).encode();self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    do_PUT=do_POST
servers=[ThreadingHTTPServer(('127.0.0.1',0),Peer) for _ in range(2)]
for server in servers:threading.Thread(target=server.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='stream-',dir=PROJECT/'output') as temp:
    root=pathlib.Path(temp);opened=[];p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def raw(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
    def op(v):r=raw(v);assert 'value'in r,r;return r['value']
    def event(st,a,i=None):return op(dict(op='event',state=st,action=a,inputs=i or {},root=temp))
    def start(response='artifact_url',path='/generate',kind='audio',limit=1,method=None,fallback=False,timeout=120,file=False):
        cfg=dict(version=1,hosts=[dict(id=f'h{i}',url=f'http://127.0.0.1:{s.server_port}',enabled=True,plain=True,auth_header='X-Key',key=f'key{i}') for i,s in enumerate(servers)],routes=[dict(id='generate',capability='speech_synthesis',host='h0',path=path,method='POST'if response=='artifact_url'else'GET',response=response,response_path='/url'if response=='artifact_url'else'',artifact_kind=kind,artifact_host='h1',artifact_limit_mb=limit,enabled=True)],defaults={})
        if method:cfg['routes'][0]['method']=method
        if file:cfg['routes'][0]['request_format']='binary'
        cfg['routes'][0]['timeout_seconds']=timeout
        if fallback:
            cfg['routes'].append(dict(cfg['routes'][0],id='fallback',host='h1',path='/post-file',method='POST'));cfg['defaults']={'speech_synthesis':dict(routes=['generate','fallback'],fallback_codes=[503],connect_fallback=True)}
        d=event(None,'resume');d=event(d['state'],'service_connections');d=event(d['state'],'service_save',dict(service_config=json.dumps(cfg)));assert not d['state'].get('notice_error'),d['state'].get('notice')
        if file:
            d=event(d['state'],'media_pick');d=event(d['state'],'media_input_result',dict(media_ticket=d['effect']['media_ticket'],name='audio.wav',data=base64.b64encode(audio).decode()))
        d=event(d['state'],'service_request');d=event(d['state'],'service_review',dict(service_route=''if fallback else'generate',service_capability='speech_synthesis'if fallback else'',service_file=file,service_input='{}'));d=event(d['state'],'approve');opened.append(d['effect']);return d
    def owner(e):return dict(purpose=e['purpose'],ticket=e['media_download'])
    def wire(e):return base64.b64decode(e['wire_base64'])if'wire_base64'in e else e['wire'].encode()
    def exchange(d):
        e=d['effect'];out=b''
        with socket.create_connection((e['host'],e['port']),timeout=5)as sock:
            sock.sendall(wire(e));upload(op,e,sock)
            while chunk:=sock.recv(8192):out+=chunk
        return event(d['state'],'network_result',dict(purpose=e['purpose'],data=base64.b64encode(out).decode()))
    def feed(d,wire_bytes=None):
        e=d['effect'];opened.append(e);o=owner(e);ready=False
        def part(data):return op(dict(o,op='media_stream_feed',data=base64.b64encode(data).decode()))['ready']
        if wire_bytes is not None:
            for i in range(0,len(wire_bytes),8192):ready=part(wire_bytes[i:i+8192])
        else:
            with socket.create_connection((e['host'],e['port']),timeout=5)as sock:
                sock.sendall(wire(e));upload(op,e,sock)
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
        d=start(response='binary',path='/post-file',kind='audio',limit=8,method='POST');check('POST binary response has an owned streaming destination',bool(d['effect'].get('media_download')))
        receipt=feed(d);done=settle(d,receipt);record=done['state']['service_artifact'];check('POST created response retains complete media above inline limit',done['state']['service_result']['http_status']==201 and(root/record['path']).read_bytes()==large_audio and record['sha256']==hashlib.sha256(large_audio).hexdigest())
        d=start(response='binary',path='/post-chunked',kind='audio',limit=8,method='POST');done=settle(d,feed(d));record=done['state']['service_artifact'];check('chunked POST created response preserves exact decoded file',(root/record['path']).read_bytes()==large_audio and record['sha256']==hashlib.sha256(large_audio).hexdigest())
        d=start(response='binary',path='/post-file',kind='audio',limit=8,method='PUT');done=settle(d,feed(d));check('PUT created response retains complete generated media',done['state']['service_result']['http_status']==201 and(root/done['state']['service_artifact']['path']).read_bytes()==large_audio)
        d=start(response='binary',path='/post-reject',kind='audio',limit=8,method='POST',fallback=True,file=True);next_request=settle(d,feed(d));check('reviewed input reaches only the initial explicitly rejected endpoint',posts[-1]==audio and calls[-1][2]=='key0');done=settle(next_request,feed(next_request));check('owned streamed input and output survive configured fallback',posts[-1]==audio and calls[-1][2]=='key1'and(root/done['state']['service_artifact']['path']).read_bytes()==large_audio)
        d=start(response='binary',path='/post-reject',kind='audio',limit=8,method='POST',fallback=True);n=len(calls);next_request=settle(d,feed(d));check('complete configured503 permits exactly one initial submission fallback',next_request['effect']and next_request['effect']['port']==servers[1].server_port and len(calls)==n+1);done=settle(next_request,feed(next_request));check('streamed fallback retains its own credentials and actual file',calls[-1][2]=='key1'and done['state']['service_result']['route']=='fallback'and(root/done['state']['service_artifact']['path']).read_bytes()==large_audio)
        for path,status in [('/post-auth',401),('/post-accepted',202),('/post-redirect',302),('/post-partial',206)]:
            d=start(response='binary',path=path,kind='audio',limit=8,method='POST',fallback=True);n=len(calls);done=settle(d,feed(d));check('streamed status stays scoped without replay '+str(status),done['effect']is None and len(calls)==n+1 and done['state']['service_result']['http_status']==status and done['state']['service_result']['status']==('accepted_unconfirmed'if status==202 else'failed')and not done['state'].get('service_artifact'))
        d=start(response='binary',path='/post-truncated-error',kind='audio',limit=8,method='POST',fallback=True);n=len(calls)
        try:feed(d);raise AssertionError('truncated error was adopted')
        except AssertionError as error:check('truncated status body cannot authorize fallback','framing'in str(error))
        op(dict(owner(d['effect']),op='media_stream_cancel'));failed=event(d['state'],'failure',dict(purpose=d['effect']['purpose'],sent=True,error='truncated response'));check('truncated submission remains uncertain without replay',failed['effect']is None and len(calls)==n+1)
        d=start(response='binary',path='/post-file',kind='audio',limit=8,method='POST',fallback=True);reserved=sum(f.stat().st_size for f in(root/'service-files').iterdir());next_request=event(d['state'],'failure',dict(purpose=d['effect']['purpose'],sent=False,error='controlled pre-send refusal'));check('pre-send streamed fallback releases old reservation before allocating next',next_request['effect']and sum(f.stat().st_size for f in(root/'service-files').iterdir())==reserved);done=settle(next_request,feed(next_request));check('pre-send fallback completes actual remaining endpoint',done['state']['service_result']['route']=='fallback')
        d=start(response='binary',path='/post-file',kind='audio',limit=8,method='POST',fallback=True);failed=event(d['state'],'failure',dict(purpose=d['effect']['purpose'],sent=True,error='controlled uncertain send'));check('uncertain initial binary submission never retries',failed['effect']is None and failed['state']['service_result']['status']=='failed')
        d=start(response='binary',path='/large-file',kind='audio',limit=8);done=settle(d,feed(d));check('large GET retrieval remains exact',(root/done['state']['service_artifact']['path']).read_bytes()==large_audio)
        d=start(response='binary',path='/created-file',kind='audio',limit=8)
        try:feed(d);raise AssertionError('retrieval accepted submission-only status')
        except AssertionError as error:check('created-file allowance stays scoped to initial submissions','HTTP 201'in str(error))
        cancel(d)
        # Retire earlier fixture streams before measuring these two independent
        # expiry paths. An unrelated lease may expire under compiler/IO load.
        for effect in opened:
            if effect and effect.get('media_download'):op(dict(owner(effect),op='media_stream_cancel'))
        total=lambda:sum(f.stat().st_size for f in(root/'service-files').iterdir());before=total()
        expired=start(response='binary',path='/post-file',kind='audio',limit=8,method='POST',timeout=5)
        check('expiry read begins with one exact reserved allowance',total()==before+8*1024*1024);time.sleep(5.1)
        bad=raw(dict(owner(expired['effect']),op='media_stream_feed',data=base64.b64encode(b'x').decode()));after=total()
        check('expired stream read releases its unwritten reservation','value'not in bad and after==before)
        abandoned=start(response='binary',path='/post-file',kind='audio',limit=8,method='POST',timeout=5)
        check('admission expiry begins with one exact reserved allowance',total()==before+8*1024*1024);time.sleep(5.1)
        current=start(response='binary',path='/post-file',kind='audio',limit=8,method='POST');check('new stream prunes abandoned expired reservations',total()==before+8*1024*1024);cancel(current)
    finally:p.stdin.close();p.wait(timeout=10)
for server in servers:server.shutdown();server.server_close()
report=dict(passed=len(passed),checks=passed,scope='Owner public-core and real local HTTP checks; Android storage/decoder and live configured endpoints need device acceptance.');(PROJECT/'output/checks/media_stream.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
