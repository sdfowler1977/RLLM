"""Opaque validators through reviewed native-core updates and actual HTTP peers."""
import base64,json,pathlib,socket,socketserver,subprocess,tempfile,threading
ROOT=pathlib.Path(__file__).resolve().parents[1]
checks=[];files={};requests=[];tag='"café-版"';extra=''
class Peer(socketserver.BaseRequestHandler):
 def handle(self):
  self.request.settimeout(5);raw=b''
  while b'\r\n\r\n'not in raw:raw+=self.request.recv(4096)
  header,body=raw.split(b'\r\n\r\n',1);lines=header.split(b'\r\n');method,path,_=lines[0].split(b' ');fields=dict(line.split(b': ',1)for line in lines[1:]);length=int(fields.get(b'Content-Length',b'0'))
  while len(body)<length:body+=self.request.recv(length-len(body))
  requests.append((method,path,fields,body));status=200;headers=b'';out=b''
  if method==b'GET':
   out=files[path]
   if path==b'/share/file.txt':headers=('ETag: '+tag+'\r\n'+extra).encode()
  elif method==b'PUT':
   if path==b'/share/file.txt':assert fields[b'If-Match']==tag.encode()
   else:assert fields[b'If-None-Match']==b'*'and path not in files
   files[path]=body;status=204 if path==b'/share/file.txt'else 201
  else:raise AssertionError(method)
  reply=f'HTTP/1.1 {status} Result\r\nContent-Length: {len(out)}\r\nConnection: close\r\n'.encode()+headers+b'\r\n'+out;self.request.sendall(reply)
class Server(socketserver.ThreadingTCPServer):allow_reuse_address=True;daemon_threads=True
server=Server(('127.0.0.1',0),Peer);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
controller=pathlib.Path(__import__('sys').argv[1])if len(__import__('sys').argv)>1 else ROOT/'output/headless/RLLM'
p=subprocess.Popen([str(controller.resolve()),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
def check(label,value):assert value,label;checks.append(label)
with tempfile.TemporaryDirectory(dir=ROOT/'output',prefix='storage-etags-')as root:
 def event(st,action,inputs=None):
  p.stdin.write(json.dumps(dict(op='event',state=st,action=action,inputs=inputs or{},root=root))+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert'value'in r,r;return r['value']
 def exchange(d):
  e=d['effect'];assert e['kind']=='network'
  with socket.create_connection((e['host'],e['port']),timeout=5)as s:
   s.sendall(e['wire'].encode());raw=b''
   while True:
    v=s.recv(65536)
    if not v:break
    raw+=v
  inputs={k:e[k]for k in ['purpose','drive_ticket','model_ticket','child_ticket','child_owner']if k in e};inputs['data']=base64.b64encode(raw).decode();return event(d['state'],'network_result',inputs)
 def update(st):
  d=event(st,'drive_update_editor');d=event(d['state'],'drive_update_review',dict(path='file.txt',content='new text'));check('manual update remains explicitly reviewed',d['effect']is None and d['state']['dialog']=='approval');return event(d['state'],'approve')
 try:
  st=event(None,'resume')['state'];st['drive']=dict(url=f'http://127.0.0.1:{server.server_address[1]}/share/',plain=True)
  for value in ['"café-版"','"e\u0301"','"\u00a0opaque\u00a0"','"ascii"']:
   tag=value;extra='';files.clear();files[b'/share/file.txt']=b'old text';requests.clear();d=update(st)
   while d['effect']is not None:d=exchange(d)
   check('valid opaque tag completes exact update '+repr(tag),files[b'/share/file.txt']==b'new text'and d['state'].get('drive_revision')is None)
   check('prior bytes survive in one separate revision '+repr(tag),len(files)==2 and b'old text'in files.values())
   target=[r for r in requests if r[0]==b'PUT'and r[1]==b'/share/file.txt'];check('conditional bytes are unchanged '+repr(tag),len(target)==1 and target[0][2][b'If-Match']==tag.encode())
   check('backup readback precedes target write '+repr(tag),[r[0]for r in requests]==[b'GET',b'PUT',b'GET',b'PUT',b'GET'])
  for value,duplicate in [('\u00a0"café"\u00a0',''),('W/"café"',''),('"bad space"',''),('"bad\u0085"',''),('"café"','ETag: "other"\r\n')]:
   tag=value;extra=duplicate;files.clear();files[b'/share/file.txt']=b'old text';requests.clear();d=exchange(update(st))
   check('unsafe validator refuses before any write '+repr((tag,duplicate)),d['effect']is None and d['state'].get('notice_error')and len(requests)==1 and files=={b'/share/file.txt':b'old text'})
  check('ordinary local settings remain available after scoped refusal',event(d['state'],'drive_edit')['state']['dialog']=='drive')
  result=dict(passed=len(checks),checks=checks,scope='Owner real HTTP peer and reviewed shared native-core events; no model/services configured; exact backup and conditional-write/readback bytes. No installed-device or independent acceptance.')
  (ROOT/'output/checks/storage-etags.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
 finally:p.stdin.close();p.wait(timeout=5);server.shutdown();server.server_close();thread.join(timeout=5)
