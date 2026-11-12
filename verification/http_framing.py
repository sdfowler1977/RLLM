"""Owned response framing over real persistent sockets and public controller calls."""
import base64,gzip,json,os,pathlib,socket,subprocess,tempfile,threading
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
def check(name,value):assert value,name;passed.append(name)
p=subprocess.Popen([os.environ.get('RLLM_CONTROLLER',str(PROJECT/'output/headless/RLLM')),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
def call(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
def op(v):r=call(v);assert'value'in r,r;return r['value']
def opening(owner='response',limit=1024*1024,head=False):return op(dict(op='network_frame_open',owner=owner,limit=limit,head_only=head))
def feed(i,data,owner='response'):return call(dict(op='network_frame_feed',owner=owner,id=i,data=base64.b64encode(data).decode()))
def finish(i,eof=False,owner='response'):return call(dict(op='network_frame_finish',owner=owner,id=i,eof=eof))
def close(i,owner='response'):return op(dict(op='network_frame_close',owner=owner,id=i))
def exact_limit_chunked(body,mime):
 head=b'HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nTransfer-Encoding: chunked\r\nContent-Type: '+mime+b'\r\n\r\n'
 line=f'{len(body):x};'.encode().ljust(8192,b'x')
 prefix=head+line+b'\r'
 return prefix+b'\n'+body+b'\r\n0\r\n\r\n',len(prefix)
def exact_limit_header(body,status=b'200 OK'):
 head=b'HTTP/1.1 '+status+b'\r\nContent-Length: '+str(len(body)).encode()+b'\r\nX-Padding: '
 return head.ljust(65536,b'x')+b'\r\n\r\n'+body
def padded_header(fields,size=65536):
 return (fields+b'\r\nX-Pad: ').ljust(size,b'x')
def exact_limit_response(body,mime):
 head=padded_header(b'HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nContent-Length: '+str(len(body)).encode()+b'\r\nContent-Type: '+mime)
 return head+b'\r\n\r\n'+body,len(head)+1
def persistent(raw,limit=1024*1024,head=False,request=None,listener=None,split=None):
 owned=listener is None
 if owned:listener=socket.socket();listener.bind(('127.0.0.1',0));listener.listen()
 release=threading.Event();sent=threading.Event();peer_closed=threading.Event();expired=threading.Event();prefix_read=threading.Event()
 def server():
  try:
   connection,_=listener.accept()
   with connection:
    if request is not None:
     received=bytearray()
     while len(received)<len(request):
      part=connection.recv(min(8192,len(request)-len(received)));assert part;received.extend(part)
     assert received==request
    if split is not None:
     connection.sendall(raw[:split])
     if not prefix_read.wait(4) or release.is_set():return
     connection.sendall(raw[split:])
    else:connection.sendall(raw)
    sent.set()
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
    if split is not None and not prefix_read.is_set() and len(out)==split:
     assert not ready,'Response completed before the split delimiter'
     prefix_read.set()
   assert sent.wait(1)and not release.is_set()and not peer_closed.is_set()and not expired.is_set();result=finish(i);assert'value'in result,result;return bytes(out),result['value']
 finally:release.set();prefix_read.set();worker.join(4);close(i)
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
 trailer_prefix=b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n0\r\n'
 def trailer_of_size(size):
  return b'X-Pad: '+b'x'*(size-11)+b'\r\n\r\n'
 valid=trailer_prefix+trailer_of_size(16384)
 actual,metadata=persistent(valid,split=len(valid)-1)
 check('the exact 16 KiB trailer section completes after its split final LF',
       actual==valid and metadata==dict(status=200,bytes=len(valid)))
 for suffix in (trailer_of_size(16385),b'X-Pad: '+b'x'*16378):
  wire=trailer_prefix+suffix
  i=opening()
  for offset in range(0,len(wire),8192):
   result=feed(i,wire[offset:offset+8192])
  check('an overlong trailer section retires only the affected owned response',
        result.get('error')=='Trailers exceed bound' and close(i) is False)
 i=opening();raw=b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nx\r\n0\r\n\r\n';ready=False
 for byte in raw:r=feed(i,bytes([byte]));assert'value'in r,r;ready=r['value']['ready']
 check('one-byte response fragmentation preserves the complete frame',ready and'value'in finish(i))
 raw,split=exact_limit_response(b'header limit payload',b'text/plain')
 actual,result=persistent(raw,split=split)
 check('an exact-limit final header split after CR completes before peer close',
       actual==raw and result==dict(status=200,bytes=len(raw)))
 delimiter=b'\r\n\r\n'
 fields=b'HTTP/1.1 200 OK\r\nContent-Length: 0'
 for size,partial in [(65536,1),(65536,2),(65536,3),(65535,2),(65534,3)]:
  i=opening();head=padded_header(fields,size)
  assert feed(i,head)['value']['ready'] is False
  check(f'{size}-byte header remains pending after {partial} delimiter bytes',
        feed(i,delimiter[:partial])['value']['ready'] is False)
  check(f'{size}-byte header completes with the rest of its delimiter',
        feed(i,delimiter[partial:])['value']['ready'] and finish(i)['value']==dict(status=200,bytes=size+4))
 interim=padded_header(b'HTTP/1.1 103 Early Hints\r\nLink: </resource>')
 raw=interim+delimiter+b'HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n'
 actual,result=persistent(raw,split=len(interim)+3)
 check('an exact-limit split informational header preserves the final response',
       actual==raw and result==dict(status=201,bytes=len(raw)))
 stream_body=b'data: {"choices":[{"delta":{"content":"BOUNDARY_OK"},"finish_reason":"stop","index":0}]}\n\ndata: [DONE]\n\n'
 stream_fields=(b'HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: '
                +str(len(stream_body)).encode())
 stream_interim=b'HTTP/1.1 103 Early Hints\r\nLink: </resource>\r\n\r\n'
 def stream_feed(i,data):
  return op(dict(op='stream_feed',id=i,data=base64.b64encode(data).decode()))
 for size in (65535,65536):
  i=op(dict(op='stream_open',protocol='chat'))
  pending=stream_interim+padded_header(stream_fields,size)
  for offset in range(0,len(pending),8192):
   part=stream_feed(i,pending[offset:offset+8192])
   assert 'error' not in part and not part['complete'] and not part['delta'],part
  check(f'consumed interim bytes do not count against a {size}-byte model header',not part['complete'])
  for byte in delimiter[:-1]:
   part=stream_feed(i,bytes([byte]))
   assert 'error' not in part and not part['complete'] and not part['delta'],part
  check(f'model header of {size} bytes waits for the last delimiter byte',not part['complete'])
  part=stream_feed(i,delimiter[-1:]+stream_body)
  settled=op(dict(op='stream_finish',id=i,eof=False))
  check(f'split model header of {size} bytes preserves its real stream result',
        part['complete'] and part['delta']=='BOUNDARY_OK' and b'BOUNDARY_OK' in base64.b64decode(settled['data']))
 i=op(dict(op='stream_open',protocol='chat'))
 assert not stream_feed(i,padded_header(stream_fields))['complete']
 part=stream_feed(i,b'x'+delimiter+stream_body)
 check('overlong streaming model header cannot expose text or complete',
       part.get('error')=='HTTP headers exceed limit' and not part['delta'] and not part['complete'])
 check('an overlong streaming model header retires its frame',
       'error' in call(dict(op='stream_finish',id=i,eof=True)))
 for protocol,first,last in (
  ('chat',b'data: {"choices":[{"index":0,"delta":{"content":"EARLIER"},"finish_reason":null}]}\n\n',
   b'data: {"choices":[{"index":0,"delta":{"content":"LATER"},"finish_reason":"stop"}]}\n\ndata: [DONE]\n\n'),
  ('local',b'{"message":{"content":"EARLIER"},"done":false}\n',
   b'{"message":{"content":"LATER"},"done":true}\n'),
 ):
  head=b'HTTP/1.1 200 OK\r\n'
  body=first+last
  length=str(len(body)).encode()
  no_content=b'HTTP/1.1 204 No Content\r\n'
  for label,payload in (
   ('body without length',b'\r\n'+body),
   ('nonzero declared length',b'Content-Length: '+length+b'\r\n\r\n'),
   ('chunked framing',b'Transfer-Encoding: chunked\r\n\r\n'),
  ):
   i=op(dict(op='stream_open',protocol=protocol))
   part=stream_feed(i,no_content+payload)
   check(f'{protocol} 204 {label} exposes no text and cannot complete',
         part.get('error')=='Body on bodyless response' and not part['delta'] and not part['complete'])
   check(f'{protocol} 204 {label} retires the affected stream',
         'error' in call(dict(op='stream_finish',id=i,eof=True)))
  for label,field in (('implicit empty body',b''),('explicit zero length',b'Content-Length: 0\r\n')):
   i=op(dict(op='stream_open',protocol=protocol))
   part=stream_feed(i,no_content+field+b'\r\n')
   check(f'{protocol} 204 {label} completes framing without model output',
         'error' not in part and part['complete'] and not part['delta'])
   final=call(dict(op='stream_finish',id=i,eof=False))
   check(f'{protocol} 204 {label} reports the missing model reply',
         final.get('error')=='Connection ended before the model completed its reply')
  i=op(dict(op='stream_open',protocol=protocol))
  assert stream_feed(i,no_content+b'\r\n')['complete']
  part=stream_feed(i,body)
  check(f'{protocol} 204 later body bytes are refused before output',
        part.get('error')=='Body on bodyless response' and not part['delta'] and not part['complete'])
  check(f'{protocol} 204 later body bytes retire the affected stream',
        'error' in call(dict(op='stream_finish',id=i,eof=True)))
  cases=[
   ('zero-length overflow',b'Content-Length: 0\r\n',body,'Streaming response exceeds Content-Length'),
   ('nonzero overflow',b'Content-Length: '+length+b'\r\n',body+b'x','Streaming response exceeds Content-Length'),
   ('duplicate length',b'Content-Length: 0\r\ncontent-length: '+length+b'\r\n',body,'Duplicate streaming framing header'),
   ('nondecimal length',b'Content-Length: +'+length+b'\r\n',body,'Invalid response length'),
   ('unsupported transfer coding',b'Transfer-Encoding: gzip\r\n',body,'Unsupported streaming transfer encoding'),
   ('unsupported content coding',b'Content-Encoding: gzip\r\n',body,'Unsupported streaming content encoding'),
   ('duplicate content coding',b'Content-Encoding: identity\r\ncontent-encoding: identity\r\n',body,'Duplicate streaming framing header'),
   ('duplicate transfer coding',b'Transfer-Encoding: chunked\r\ntransfer-encoding: chunked\r\n',body,'Duplicate streaming framing header'),
   ('ambiguous length',b'Content-Length: 999\r\nTransfer-Encoding: chunked\r\n',
    f'{len(body):x}\r\n'.encode()+body+b'\r\n0\r\n\r\n','Ambiguous streaming response framing'),
  ]
  for label,length_fields,payload,error in cases:
   i=op(dict(op='stream_open',protocol=protocol))
   part=stream_feed(i,head+length_fields+b'\r\n'+payload)
   check(f'{protocol} {label} exposes no text and cannot complete',
         part.get('error')==error and not part['delta'] and not part['complete'])
   check(f'{protocol} {label} retires the affected stream',
         'error' in call(dict(op='stream_finish',id=i,eof=True)))
  i=op(dict(op='stream_open',protocol=protocol))
  part=stream_feed(i,head+b'Content-Length: '+str(len(first)+1).encode()+b'\r\n\r\n'+first)
  check(f'{protocol} publishes earlier valid text before the full declared body arrives',
        'error' not in part and part['delta']=='EARLIER' and not part['complete'])
  part=stream_feed(i,last)
  check(f'{protocol} subsequent overflow exposes no further text',
        part.get('error')=='Streaming response exceeds Content-Length' and not part['delta'] and not part['complete'])
  i=op(dict(op='stream_open',protocol=protocol))
  prefix=stream_feed(i,head+b'Content-Length: '+length+b'\r\n\r\n'+first)
  final=stream_feed(i,last)
  completed=op(dict(op='stream_finish',id=i,eof=False))
  check(f'{protocol} exact-length split body still completes with both text fragments',
        prefix['delta']=='EARLIER' and not prefix['complete'] and final['delta']=='LATER' and final['complete']
        and b'EARLIERLATER' in base64.b64decode(completed['data']))
  i=op(dict(op='stream_open',protocol=protocol))
  frames=b''.join(f'{len(part):x}\r\n'.encode()+part+b'\r\n' for part in (first,last))+b'0\r\n\r\n'
  chunked=stream_feed(i,head+b'Transfer-Encoding: Chunked\r\nContent-Encoding: identity\r\n\r\n'+frames)
  check(f'{protocol} still accepts chunked framing with identity encoding',chunked['delta']=='EARLIERLATER' and chunked['complete'] and 'value' in call(dict(op='stream_finish',id=i,eof=False)))
  chunk_head=head+b'Transfer-Encoding: chunked\r\n\r\n'
  terminal=chunk_head+f'{len(body):x}\r\n'.encode()+body+b'\r\n0\r\n'
  for label,trailer,error in (
   ('malformed trailer field',b'X-Bad : value\r\n\r\n','Invalid response header'),
   ('framing trailer',b'Content-Length: 0\r\n\r\n','Framing trailer forbidden'),
   ('overlong unfinished trailer',b'X-Note: '+b'x'*16384,'Chunk trailers exceed limit'),
   ('bytes after final trailer delimiter',b'\r\nextra','Bytes after framed response'),
  ):
   i=op(dict(op='stream_open',protocol=protocol))
   part=stream_feed(i,terminal+trailer)
   check(f'{protocol} {label} refuses completion and preserves validated text',
         part.get('error')==error and part['delta']=='EARLIERLATER' and not part['complete'])
   check(f'{protocol} {label} retires the affected stream',
         'error' in call(dict(op='stream_finish',id=i,eof=True)))
  i=op(dict(op='stream_open',protocol=protocol))
  part=stream_feed(i,terminal+b'X-Note: valid\r')
  check(f'{protocol} split trailer field retains text and waits for completion',
        'error' not in part and part['delta']=='EARLIERLATER' and not part['complete'])
  part=stream_feed(i,b'\n\r')
  check(f'{protocol} validated trailer still waits for the final blank line',
        'error' not in part and not part['delta'] and not part['complete'])
  part=stream_feed(i,b'\n')
  completed=op(dict(op='stream_finish',id=i,eof=False))
  check(f'{protocol} valid split trailers complete without losing the reply',
        part['complete'] and not part['delta'] and b'EARLIERLATER' in base64.b64decode(completed['data']))
  first_chunk=f'{len(first):x}\r\n'.encode()+first
  last_chunk=f'{len(last):x}\r\n'.encode()+last
  i=op(dict(op='stream_open',protocol=protocol))
  part=stream_feed(i,chunk_head+first_chunk+b'\r')
  check(f'{protocol} chunk payload waits for its split trailing LF',
        'error' not in part and not part['delta'] and not part['complete'])
  part=stream_feed(i,b'\n')
  check(f'{protocol} valid split chunk delimiter releases its text promptly',
        'error' not in part and part['delta']=='EARLIER' and not part['complete'])
  part=stream_feed(i,last_chunk+b'\r\n0\r\n\r\n')
  completed=op(dict(op='stream_finish',id=i,eof=False))
  check(f'{protocol} validated chunks preserve the complete reply',
        part['complete'] and part['delta']=='LATER' and b'EARLIERLATER' in base64.b64decode(completed['data']))
  i=op(dict(op='stream_open',protocol=protocol))
  part=stream_feed(i,chunk_head+first_chunk+b'\r\n'+last_chunk)
  check(f'{protocol} one read exposes only the chunk with a validated delimiter',
        'error' not in part and part['delta']=='EARLIER' and not part['complete'])
  part=stream_feed(i,b'xx')
  check(f'{protocol} bad split chunk delimiter refuses the pending text',
        part.get('error')=='Invalid stream chunk terminator' and not part['delta'] and not part['complete'])
  check(f'{protocol} bad split chunk delimiter retires the affected stream',
        'error' in call(dict(op='stream_finish',id=i,eof=True)))
  i=op(dict(op='stream_open',protocol=protocol))
  part=stream_feed(i,chunk_head+first_chunk+b'\r\n'+last_chunk+b'xx')
  check(f'{protocol} same-read delimiter refusal preserves only the earlier validated chunk',
        part.get('error')=='Invalid stream chunk terminator' and part['delta']=='EARLIER' and not part['complete'])
  check(f'{protocol} same-read delimiter refusal retires the affected stream',
        'error' in call(dict(op='stream_finish',id=i,eof=True)))
 for suffix in (b'x',b'x\r',b'x'+delimiter,b'\rx',b'\r\n\rx'):
  i=opening();assert feed(i,padded_header(fields))['value']['ready'] is False
  reply=feed(i,suffix)
  check(f'overlong response header suffix {suffix!r} retires only that frame',
        reply.get('error')=='HTTP headers exceed bound' and close(i) is False)
 raw,split=exact_limit_chunked(b'limit payload',b'text/plain')
 actual,result=persistent(raw,split=split)
 check('an exact-limit chunk header split after CR completes before peer close',
       actual==raw and result==dict(status=200,bytes=len(raw)))
 for split in (65537,65538,65539):
  raw=exact_limit_header(b'actual body')
  actual,result=persistent(raw,split=split)
  check('exact-limit final response header accepts delimiter split at '+str(split),actual==raw and result==dict(status=200,bytes=len(raw)))
 interim=b'HTTP/1.1 100 Continue\r\nX-Padding: '.ljust(65536,b'x')+b'\r\n\r\n'
 raw=interim+b'HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\na'
 actual,result=persistent(raw,split=65539)
 check('exact-limit informational header accepts a partial delimiter before the final response',actual==raw and result==dict(status=200,bytes=len(raw)))
 for extra in (b'x',b'x\r',b'\r\nx',b'\r\n\rx'):
  i=opening();assert 'value' in feed(i,exact_limit_header(b'')[:65536]);r=feed(i,extra)
  check('one extra metadata byte or a broken delimiter fails within the limit '+repr(extra),r.get('error')=='HTTP headers exceed bound' and close(i) is False)
 head=b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n'
 i=opening();terminal=b'0;'+b'x'*8190
 check('an exact-limit terminal chunk stays pending after CR',not feed(i,head+terminal+b'\r')['value']['ready'])
 check('the terminal LF and empty trailer section complete the exact-limit frame',
       feed(i,b'\n\r\n')['value']['ready'] and 'value' in finish(i))
 for label,suffix in [('unterminated overflow',b'x'),('overflow followed by CR',b'x\r'),
                      ('overflow followed by CRLF',b'x\r\n'),('CR followed by a non-LF byte',b'\rx')]:
  i=opening();reply=feed(i,head+terminal+suffix)
  check(label+' is refused and retires only that frame',reply.get('error')=='Chunk header exceeds bound' and close(i) is False)
 slots=[opening(owner='slot-'+str(n))for n in range(8)];check('response slots remain bounded','error'in call(dict(op='network_frame_open',owner='overflow',limit=100)))
 for n,i in enumerate(slots):close(i,owner='slot-'+str(n))
 actual,result=persistent(b'HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n');check('ordinary response remains available after scoped failures',result['status']==200)

 with tempfile.TemporaryDirectory(prefix='http-owners-',dir=PROJECT/'output')as tmp:
  root=pathlib.Path(tmp)
  def listener():
   value=socket.socket();value.bind(('127.0.0.1',0));value.listen();return value
  primary=listener();child=listener();nas=listener()
  def event(st,action,inputs=None):return op(dict(op='event',state=st,action=action,inputs=inputs or{},root=str(root)))
  def model_response(d,text,source,framing=None):
   data=json.dumps({'choices':[{'index':0,'message':{'content':text},'finish_reason':'stop'}]}).encode()
   if framing=='chunk':raw,split=exact_limit_chunked(data,b'application/json')
   elif framing=='header':raw,split=exact_limit_response(data,b'application/json')
   else:raw=b'HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nContent-Length: '+str(len(data)).encode()+b'\r\n\r\n'+data;split=None
   actual,_=persistent(raw,request=d['effect']['wire'].encode(),listener=source,split=split)
   return event(d['state'],'network_result',dict(purpose='model',model_ticket=d['state']['model_expected'],data=base64.b64encode(actual).decode()))
  def state():
   st=event(None,'resume')['state'];st['profiles']=[dict(name=name,model=name,protocol='chat',endpoint='http://127.0.0.1:'+str(source.getsockname()[1])+'/model',plain=True,stream=False,tool_format='structured',key='')for name,source in [('Primary',primary),('Child',child)]];st['agents']=[dict(name='Parent',model='0',tools='read,delegate,drive_read',subagents=[1]),dict(name='Child',model='1',tools='read')];return st
  try:
   d=event(state(),'send',{'draft':'Delegate a simple response.'});d=model_response(d,json.dumps({'tool':'delegate','args':{'agent':1,'task':'Return READY_CHILD'}}),primary);check('persistent primary HTTP reply opens the actual child review',d['state']['pending']['tool']=='delegate');d=event(d['state'],'approve');d=model_response(d,'READY_CHILD',child,framing='chunk');check('persistent child reply with an exact-limit split chunk header returns through its captured parent',d['effect']is not None and d['state']['request_index']==0 and'READY_CHILD'in d['effect']['wire']);d=model_response(d,'Parent received READY_CHILD.',primary,framing='header');check('persistent parent continuation with an exact-limit split response header settles its actual task',not d['state']['busy']and not d['state'].get('delegation_stack'))
   st=state();st['drive']={'url':'http://127.0.0.1:'+str(nas.getsockname()[1])+'/share/','plain':True,'user':'','password':''};d=event(st,'send',{'draft':'Read actual.txt from the network folder.'});d=model_response(d,json.dumps({'tool':'drive_read','args':{'path':'actual.txt'}}),primary);d=event(d['state'],'approve');e=d['effect'];body=b'ACTUAL_PERSISTENT_NAS_FILE';raw,split=exact_limit_response(body,b'text/plain');actual,_=persistent(raw,request=e['wire'].encode(),listener=nas,split=split);d=event(d['state'],'network_result',dict(purpose=e['purpose'],drive_ticket=e['drive_ticket'],data=base64.b64encode(actual).decode()));check('persistent NAS response with an exact-limit split response header resumes its requesting model with actual bytes',d['effect']is not None and d['effect']['purpose']=='model'and body.decode()in d['effect']['wire']);d=model_response(d,'The network file contains ACTUAL_PERSISTENT_NAS_FILE.',primary);check('persistent NAS continuation settles without replay',not d['state']['busy'])
  finally:primary.close();child.close();nas.close()
finally:p.stdin.close();p.wait(timeout=10)
result=dict(passed=passed,scope='Owner public-API and real loopback persistent-socket framing; actual complete wire bytes, gzip preservation, EOF ownership, bounds, malformed input and scoped session failures.');(PROJECT/'output/checks/http-framing.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
