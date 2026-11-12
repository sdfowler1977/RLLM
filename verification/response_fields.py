"""Response-integrity checks through actual peers and public storage events."""
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
with tempfile.TemporaryDirectory(dir=ROOT/'output',prefix='response-fields-')as root:
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
  malformed=[valid.replace(b'HTTP/1.1',b'HTTP/1.x',1),valid.replace(b'207 ',b'+207 ',1),valid.replace(b'Content-Length: ',b'Content-Length: +',1)]
  malformed += [valid.replace(b'Connection: close',f.encode(),1)for f in['Bad Name: x','X: bad\rInjected: yes','X: bad\nInjected: yes','X: bad\x00value']]
  malformed += [h+valid for h in[b'HTTP/1.1 101 Switching Protocols\r\n\r\n',b'HTTP/1.1 100 Continue\r\nBad Name: x\r\n\r\n',b'HTTP/1.1 103 Early Hints\r\nContent-Length: 2\r\n\r\n',b'HTTP/1.1 100 Continue\r\nTransfer-Encoding: chunked\r\n\r\n']]
  chunked=b'HTTP/1.1 207 Multi-Status\r\nTransfer-Encoding: chunked\r\n\r\n'
  malformed += [chunked+b'+'+format(len(body),'x').encode()+b'\r\n'+body+b'\r\n0\r\n\r\n',chunked+format(len(body),'x').encode()+b'\r\n'+body+b'\r\n0\r\nBad Name: x\r\n\r\n',chunked+format(len(body),'x').encode()+b'\r\n'+body+b'\r\n0\r\nContent-Length: 0\r\n\r\n']
  expected=['Invalid HTTP status line','Invalid HTTP status line','Invalid response length']+['Invalid response header']*4+['Unsupported informational response','Invalid response header','Unsupported informational response','Unsupported informational response']+['Invalid chunk size','Invalid response header','Framing trailer forbidden']
  for index,raw in enumerate(malformed):
   old=d['state']['drive'].copy();reply=raw;d=listing(d['state'])
   check(f'malformed response {index} refused by actual storage event',not d['state']['busy']and d['state']['notice']==expected[index])
   check(f'refusal {index} retains destination and credentials',d['state']['drive']==old)
   reply=valid;d=listing(d['state']);check(f'ordinary storage recovers after refusal {index}',len(d['state']['remote'])==1 and'error'not in d['state']['notice'].lower())
  reply=b'HTTP/1.1 103 Early Hints\r\nLink: </resource>\r\n\r\n'+valid.replace(b'Connection: close',b'X_Custom: \tvalue\t',1);d=listing(d['state']);check('valid interim and token field names remain supported',len(d['state']['remote'])==1 and'error'not in d['state']['notice'].lower())
  check('every exchange used actual configured storage peer',len(requests)==30 and all(r.startswith(b'PROPFIND /share/')for r in requests))
  print(json.dumps(dict(passed=len(passed),checks=passed),indent=2))
 finally:
  p.stdin.close();p.wait(timeout=5);server.shutdown();server.server_close();thread.join(timeout=5)
