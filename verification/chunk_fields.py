"""Chunk-extension integrity checks through actual peers and public storage events."""
import base64,json,pathlib,socket,socketserver,subprocess,tempfile,threading
ROOT=pathlib.Path(__file__).resolve().parents[1]
body=b'<d:multistatus xmlns:d="DAV:"><d:response><d:href>/share/ok.txt</d:href><d:propstat><d:prop><d:displayname>ok.txt</d:displayname></d:prop></d:propstat></d:response></d:multistatus>'
valid=b'HTTP/1.1 207 Multi-Status\r\nContent-Length: '+str(len(body)).encode()+b'\r\nConnection: close\r\n\r\n'+body
reply=valid;requests=[];passed=[]
class Peer(socketserver.BaseRequestHandler):
 def handle(self):
  self.request.settimeout(5);data=b''
  while b'\r\n\r\n'not in data:
   part=self.request.recv(4096)
   if not part:return
   data+=part
   assert len(data)<65536
  requests.append(data);self.request.sendall(reply)
class Server(socketserver.ThreadingTCPServer):allow_reuse_address=True;daemon_threads=True
server=Server(('127.0.0.1',0),Peer);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
controller=pathlib.Path(__import__('sys').argv[1])if len(__import__('sys').argv)>1 else ROOT/'output/headless/RLLM'
p=subprocess.Popen([str(controller.resolve()),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
def check(name,value):assert value,name;passed.append(name)
def call(v):
 p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert'value'in r,r;return r['value']
with tempfile.TemporaryDirectory(dir=ROOT/'output',prefix='chunk-fields-')as root:
 def event(st,a,inputs=None):return call(dict(op='event',state=st,action=a,inputs=inputs or{},root=root))
 def listing(st):
  d=event(st,'drive_list');e=d['effect'];assert e['kind']=='network'
  with socket.create_connection((e['host'],e['port']),timeout=5)as s:
   s.sendall(e['wire'].encode());parts=[]
   while True:
    v=s.recv(65536)
    if not v:break
    parts.append(v)
  values={k:e[k]for k in['purpose','drive_ticket','child_ticket','child_owner','model_ticket']if k in e};values['data']=base64.b64encode(b''.join(parts)).decode()
  return event(d['state'],'network_result',values)
 try:
  d=event(None,'resume');d=event(d['state'],'drive_edit');d=event(d['state'],'drive_save',dict(url=f'http://127.0.0.1:{server.server_address[1]}/share/',plain=True));d=listing(d['state'])
  check('ordinary WebDAV works without a model or extra services',len(d['state']['remote'])==1)
  chunked=b'HTTP/1.1 207 Multi-Status\r\nTransfer-Encoding: chunked\r\n\r\n'
  def chunk(extension,last=False):
   return chunked+format(len(body),'x').encode()+(b''if last else extension)+b'\r\n'+body+b'\r\n0'+(extension if last else b'')+b'\r\n\r\n'
  extensions=[b';',b';=x',b';name=',b';name="open',b';name="closed"tail',b';name=two words',b';na(me=x',b';name=x\x00',b';name="x\rY"',b';name="x\nY"',b';name="x\x7fY"',b';name=x;',b' ',b';name ']
  malformed=[chunk(extension,last)for extension in extensions for last in[False,True]]
  expected=['Invalid chunk extension']*len(malformed)
  for index,raw in enumerate(malformed):
   old=d['state']['drive'].copy();reply=raw;d=listing(d['state'])
   check(f'malformed response {index} refused by actual storage event',not d['state']['busy']and d['state']['notice']==expected[index])
   check(f'refusal {index} retains destination and credentials',d['state']['drive']==old)
   reply=valid;d=listing(d['state']);check(f'ordinary storage recovers after refusal {index}',len(d['state']['remote'])==1 and'error'not in d['state']['notice'].lower())
  for extension in[b';flag',b';key=value;other=token',b' \t; \tkey \t= \t"quoted; value";flag',b';key=""']:
   for last in[False,True]:
    reply=chunk(extension,last);d=listing(d['state']);check('valid extension remains available '+repr((extension,last)),len(d['state']['remote'])==1 and'error'not in d['state']['notice'].lower())
  for index,raw in enumerate(malformed):
   ident=call(dict(op='network_frame_open',owner='chunks',limit=65536))
   p.stdin.write(json.dumps(dict(op='network_frame_feed',owner='chunks',id=ident,data=base64.b64encode(raw).decode()))+'\n');p.stdin.flush();r=json.loads(p.stdout.readline())
   check('stream refusal '+str(index),r.get('error')=='Invalid chunk extension')
   check('failed stream retires its own session '+str(index),call(dict(op='network_frame_close',owner='chunks',id=ident))is False)
  ident=call(dict(op='network_frame_open',owner='chunks',limit=65536));raw=chunk(b' \t; flag; key="value"')
  for byte in raw:
   r=call(dict(op='network_frame_feed',owner='chunks',id=ident,data=base64.b64encode(bytes([byte])).decode()))
  check('fragmented valid stream recovers after malformed peers',r['ready']and call(dict(op='network_frame_finish',owner='chunks',id=ident,eof=False))['status']==207)
  for status in (b'200 OK',b'207 Multi-Status'):
   owner='split-chunk-'+status.decode().split()[0]
   head=b'HTTP/1.1 '+status+b'\r\nTransfer-Encoding: chunked\r\n\r\n'
   for last in (False,True):
    line=(b'0;flag=' if last else b'1;flag=').ljust(8192,b'x')
    prefix=head+(b'1\r\nA\r\n' if last else b'')
    ident=call(dict(op='network_frame_open',owner=owner,limit=65536))
    def feed(data):return call(dict(op='network_frame_feed',owner=owner,id=ident,data=base64.b64encode(data).decode()))
    check('exact-size chunk line accepts a split delimiter '+str((status,last)),not feed(prefix+line+b'\r')['ready'])
    end=feed(b'\n\r\n' if last else b'\nA\r\n0\r\n\r\n')
    check('split delimiter completes the owned response '+str((status,last)),end['ready']and call(dict(op='network_frame_finish',owner=owner,id=ident,eof=False))['status']==int(status[:3]))
    ident=call(dict(op='network_frame_open',owner=owner,limit=65536))
    p.stdin.write(json.dumps(dict(op='network_frame_feed',owner=owner,id=ident,data=base64.b64encode(prefix+line+b'x\r').decode()))+'\n');p.stdin.flush()
    check('one extra chunk-header byte fails before LF '+str((status,last)),json.loads(p.stdout.readline()).get('error')=='Chunk header exceeds bound')
  check('every exchange used actual configured storage peer',len(requests)==65 and all(r.startswith(b'PROPFIND /share/')for r in requests))
  print(json.dumps(dict(passed=len(passed),checks=passed),indent=2))
 finally:
  p.stdin.close();p.wait(timeout=5);server.shutdown();server.server_close();thread.join(timeout=5)
