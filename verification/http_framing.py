"""Owned response framing over real persistent sockets and public controller calls."""
import base64,gzip,json,pathlib,socket,subprocess,tempfile,threading
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
def check(name,value):assert value,name;passed.append(name)
p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
def call(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
def op(v):r=call(v);assert'value'in r,r;return r['value']
def opening(owner='response',limit=1024*1024,head=False):return op(dict(op='network_frame_open',owner=owner,limit=limit,head_only=head))
def feed(i,data,owner='response'):return call(dict(op='network_frame_feed',owner=owner,id=i,data=base64.b64encode(data).decode()))
def finish(i,eof=False,owner='response'):return call(dict(op='network_frame_finish',owner=owner,id=i,eof=eof))
def close(i,owner='response'):return op(dict(op='network_frame_close',owner=owner,id=i))
def persistent(raw,limit=1024*1024,head=False,request=None,listener=None):
 owned=listener is None
 if owned:listener=socket.socket();listener.bind(('127.0.0.1',0));listener.listen()
 release=threading.Event();sent=threading.Event();peer_closed=threading.Event();expired=threading.Event()
 def server():
  try:
   connection,_=listener.accept()
   with connection:
    if request is not None:
     received=bytearray()
     while len(received)<len(request):
      part=connection.recv(min(8192,len(request)-len(received)));assert part;received.extend(part)
     assert received==request
    connection.sendall(raw);sent.set();
    if not release.wait(10):expired.set()
  finally:
   peer_closed.set()
   if owned:listener.close()
 worker=threading.Thread(target=server);worker.start();i=opening(limit=limit,head=head);out=bytearray();ready=False
 try:
  with socket.create_connection(listener.getsockname(),timeout=1)as connection:
   connection.settimeout(1)
   if request is not None:connection.sendall(request)
   while not ready:
    data=connection.recv(8192);assert data,'Unexpected EOF';out.extend(data);r=feed(i,data);assert'value'in r,r;ready=r['value']['ready']
   assert sent.wait(1)and not release.is_set()and not peer_closed.is_set()and not expired.is_set();result=finish(i);assert'value'in result,result;return bytes(out),result['value']
 finally:release.set();worker.join(4);close(i)
try:
 for name,raw,status,head in [
  ('length',b'HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nContent-Length: 5\r\n\r\nhello',200,False),
  ('chunked',b'HTTP/1.1 207 Multi-Status\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\nResult: done\r\n\r\n',207,False),
  ('interim',b'HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n',201,False),
  ('bodyless',b'HTTP/1.1 204 No Content\r\n\r\n',204,False),
  ('unchanged length metadata',b'HTTP/1.1 304 Not Modified\r\nContent-Length: 5000000\r\n\r\n',304,False),
  ('unchanged chunk metadata',b'HTTP/1.1 304 Not Modified\r\nTransfer-Encoding: chunked\r\n\r\n',304,False),
  ('unchanged coded metadata',b'HTTP/1.1 304 Not Modified\r\nContent-Encoding: gzip\r\nContent-Length: 5000000\r\n\r\n',304,False),
  ('head chunk metadata',b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n',200,True),
  ('head',b'HTTP/1.1 200 OK\r\nContent-Length: 5000000\r\n\r\n',200,True),
 ]:
  actual,result=persistent(raw,head=head);check(name+' completes before the persistent peer closes',actual==raw and result['status']==status and result['bytes']==len(raw))
 raw=b'HTTP/1.1 207 Multi-Status\r\nContent-Length: 90000\r\n\r\n'+b'x'*90000;actual,result=persistent(raw);check('large WebDAV207 completes within the response bound',actual==raw and result['status']==207)
 body=gzip.compress(b'<html><body>GZIP_FRAME</body></html>');raw=b'HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: '+str(len(body)).encode()+b'\r\n\r\n'+body;actual,result=persistent(raw);check('framing preserves coded response bytes',actual==raw)
 i=opening();raw=b'HTTP/1.1 200 OK\r\n\r\nclose-delimited';r=feed(i,raw);check('close-delimited response waits for observed EOF',not r['value']['ready']and'value'in finish(i,eof=True));i=opening();feed(i,raw);check('unobserved EOF cannot settle close-delimited response','error'in finish(i))
 for label,raw in [
  ('short length',b'HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\na'),
  ('unfinished chunk',b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\na'),
  ('missing trailer terminator',b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nOne: two\r\n'),
 ]:
  i=opening();feed(i,raw);check(label+' remains a scoped incomplete response','error'in finish(i,eof=True))
 for label,raw in [
  ('duplicate length',b'HTTP/1.1 200 OK\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n'),
  ('ambiguous framing',b'HTTP/1.1 200 OK\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n'),
  ('bodyless extra bytes',b'HTTP/1.1 304 Not Modified\r\nContent-Length: 1\r\n\r\nx'),
  ('extra bytes',b'HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nab'),
  ('framing trailer',b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nContent-Length: 0\r\n\r\n'),
 ]:
  i=opening();check(label+' is refused locally','error'in feed(i,raw));check(label+' retires only its failed session',close(i)is False)
 for header in [b'Content-Length: 5000000\r\n',b'Transfer-Encoding: chunked\r\n',b'Content-Encoding: gzip\r\nContent-Length: 5000000\r\n']:
  r=call(dict(op='network_binary',data=base64.b64encode(b'HTTP/1.1 304 Not Modified\r\n'+header+b'\r\n').decode()));check('downstream bodyless metadata preserves its actual304 diagnostic '+header.split(b':')[0].decode(),r.get('error')=='HTTP 304: server returned representation metadata.')
 i=opening();r=feed(i,b'HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n',owner='foreign');check('foreign owner leaves the actual response session intact','error'in r and close(i,owner='foreign')is False and'value'in feed(i,b'HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n')and'value'in finish(i))
 i=opening(limit=64);check('declared response above its current bound is refused','error'in feed(i,b'HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n'));i=opening(limit=32);check('wire bytes above current bound are refused','error'in feed(i,b'x'*33))
 i=opening();raw=b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nx\r\n0\r\n\r\n';ready=False
 for byte in raw:r=feed(i,bytes([byte]));assert'value'in r,r;ready=r['value']['ready']
 check('one-byte response fragmentation preserves the complete frame',ready and'value'in finish(i))
 slots=[opening(owner='slot-'+str(n))for n in range(8)];check('response slots remain bounded','error'in call(dict(op='network_frame_open',owner='overflow',limit=100)))
 for n,i in enumerate(slots):close(i,owner='slot-'+str(n))
 actual,result=persistent(b'HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n');check('ordinary response remains available after scoped failures',result['status']==200)

 with tempfile.TemporaryDirectory(prefix='http-owners-',dir=PROJECT/'output')as tmp:
  root=pathlib.Path(tmp)
  def listener():
   value=socket.socket();value.bind(('127.0.0.1',0));value.listen();return value
  primary=listener();child=listener();nas=listener()
  def event(st,action,inputs=None):return op(dict(op='event',state=st,action=action,inputs=inputs or{},root=str(root)))
  def model_response(d,text,source):
   data=json.dumps({'choices':[{'index':0,'message':{'content':text},'finish_reason':'stop'}]}).encode();raw=b'HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nContent-Length: '+str(len(data)).encode()+b'\r\n\r\n'+data;actual,_=persistent(raw,request=d['effect']['wire'].encode(),listener=source);return event(d['state'],'network_result',dict(purpose='model',model_ticket=d['state']['model_expected'],data=base64.b64encode(actual).decode()))
  def state():
   st=event(None,'resume')['state'];st['profiles']=[dict(name=name,model=name,protocol='chat',endpoint='http://127.0.0.1:'+str(source.getsockname()[1])+'/model',plain=True,stream=False,tool_format='structured',key='')for name,source in [('Primary',primary),('Child',child)]];st['agents']=[dict(name='Parent',model='0',tools='read,delegate,drive_read',subagents=[1]),dict(name='Child',model='1',tools='read')];return st
  try:
   d=event(state(),'send',{'draft':'Delegate a simple response.'});d=model_response(d,json.dumps({'tool':'delegate','args':{'agent':1,'task':'Return READY_CHILD'}}),primary);check('persistent primary HTTP reply opens the actual child review',d['state']['pending']['tool']=='delegate');d=event(d['state'],'approve');d=model_response(d,'READY_CHILD',child);check('persistent child reply returns through its captured parent',d['effect']is not None and d['state']['request_index']==0 and'READY_CHILD'in d['effect']['wire']);d=model_response(d,'Parent received READY_CHILD.',primary);check('persistent parent continuation settles its actual task',not d['state']['busy']and not d['state'].get('delegation_stack'))
   st=state();st['drive']={'url':'http://127.0.0.1:'+str(nas.getsockname()[1])+'/share/','plain':True,'user':'','password':''};d=event(st,'send',{'draft':'Read actual.txt from the network folder.'});d=model_response(d,json.dumps({'tool':'drive_read','args':{'path':'actual.txt'}}),primary);d=event(d['state'],'approve');e=d['effect'];body=b'ACTUAL_PERSISTENT_NAS_FILE';raw=b'HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: '+str(len(body)).encode()+b'\r\n\r\n'+body;actual,_=persistent(raw,request=e['wire'].encode(),listener=nas);d=event(d['state'],'network_result',dict(purpose=e['purpose'],drive_ticket=e['drive_ticket'],data=base64.b64encode(actual).decode()));check('persistent NAS response resumes its requesting model with actual bytes',d['effect']is not None and d['effect']['purpose']=='model'and body.decode()in d['effect']['wire']);d=model_response(d,'The network file contains ACTUAL_PERSISTENT_NAS_FILE.',primary);check('persistent NAS continuation settles without replay',not d['state']['busy'])
  finally:primary.close();child.close();nas.close()
finally:p.stdin.close();p.wait(timeout=10)
result=dict(passed=passed,scope='Owner public-API and real loopback persistent-socket framing; actual complete wire bytes, gzip preservation, EOF ownership, bounds, malformed input and scoped session failures.');(PROJECT/'output/checks/http-framing.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
