"""Configurable storage credentials across actual WebDAV and owned payload transports."""
import base64,copy,http.server,json,pathlib,socket,subprocess,tempfile,threading
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[];calls=[];files={'/share/alpha.txt':b'Observed storage text.'};expected={};refuse=False
def check(name,value):assert value,name;passed.append(name)
class Peer(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def exchange(self):
  body=self.rfile.read(int(self.headers.get('Content-Length','0')));actual={k.lower():v for k,v in self.headers.items()if k.lower()in['authorization','x-storage-key']};calls.append((self.command,self.path,actual,body));status=200;data=b''
  if actual!=expected or refuse:status=401
  elif self.command=='PROPFIND':status=207;data=b'<d:multistatus xmlns:d="DAV:"><d:response><d:href>/share/alpha.txt</d:href><d:propstat><d:prop><d:displayname>alpha.txt</d:displayname></d:prop></d:propstat></d:response></d:multistatus>'
  elif self.command=='PUT':
   if self.headers.get('If-None-Match')=='*'and self.path in files:status=412
   else:files[self.path]=body;status=201
  elif self.command=='MKCOL':status=201
  elif self.path in files:data=files[self.path]
  else:status=404
  self.send_response(status);self.send_header('Content-Length',str(len(data)));self.send_header('Connection','close');self.end_headers();self.wfile.write(data)
 do_PROPFIND=exchange;do_GET=exchange;do_PUT=exchange;do_MKCOL=exchange
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Peer);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start();url=f'http://127.0.0.1:{server.server_port}/share/'
with tempfile.TemporaryDirectory(prefix='storage-auth-',dir=PROJECT/'output')as folder:
 p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def raw(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
 def op(v):r=raw(v);assert'value'in r,r;return r['value']
 def event(st,a,i=None):return op(dict(op='event',state=st,action=a,inputs=i or{},root=folder))
 def exchange(e):
  wire=base64.b64decode(e['wire_base64'])if'wire_base64'in e else e['wire'].encode()
  with socket.create_connection((e['host'],e['port']),timeout=5)as sock:
   sock.sendall(wire);chunks=[]
   while True:
    chunk=sock.recv(65536)
    if not chunk:break
    chunks.append(chunk)
  return b''.join(chunks)
 def complete(d):
  e=d['effect'];values={k:e[k]for k in['purpose','drive_ticket','child_ticket','child_owner','model_ticket']if k in e};values['data']=base64.b64encode(exchange(e)).decode();return event(d['state'],'network_result',values)
 def save(st,**changes):return event(event(st,'drive_edit')['state'],'drive_save',dict(url=url,plain=True,**changes))
 def listing(st):return complete(event(st,'drive_list'))
 try:
  d=event(None,'resume');d=save(d['state']);d=listing(d['state']);check('empty credential settings retain real unauthenticated WebDAV',calls[-1][2]=={}and len(d['state']['remote'])==1)
  d=save(d['state'],user='saved-user',password='saved-password');basic='Basic '+base64.b64encode(b'saved-user:saved-password').decode();expected={'authorization':basic};d=listing(d['state']);check('legacy username/password settings retain exact Basic authentication',calls[-1][2]==expected and len(d['state']['remote'])==1)
  d=save(d['state'],key='controlled-storage-token');expected={'authorization':'Bearer controlled-storage-token'};d=listing(d['state']);check('default key uses one Bearer header without Basic fallback',calls[-1][2]==expected and len(d['state']['remote'])==1);check('key mode preserves existing username/password credentials',d['state']['drive']['user']=='saved-user'and d['state']['drive']['password']=='saved-password')
  opened=event(d['state'],'drive_edit');fields={f['key']:f for f in opened['ui']['modal']['fields']};check('storage key has a secret control and configurable protocol fields',fields['key']['kind']=='secret'and fields['auth_header']['value']=='Authorization'and fields['auth_prefix']['value']=='Bearer');check('saved credential mode is explicit without printing the key','Key/token header active'in fields['auth_status']['value']and'controlled-storage-token'not in fields['auth_status']['value'])
  d=save(d['state'],auth_header='X-Storage-Key',auth_prefix='Token');expected={'x-storage-key':'Token controlled-storage-token'};d=listing(d['state']);check('custom credential header and prefix reach the real server',calls[-1][2]==expected and len(d['state']['remote'])==1)
  d=save(d['state'],auth_prefix='');expected={'x-storage-key':'controlled-storage-token'};d=listing(d['state']);check('explicit empty prefix sends the exact raw key',calls[-1][2]==expected)
  d=event(d['state'],'remote:0');d=complete(d);check('file reads retain the same configured credential method',calls[-1][0]=='GET'and calls[-1][2]==expected and'Observed storage text.'in json.dumps(d['ui']))
  d=event(d['state'],'drive_editor');d=event(d['state'],'drive_create_review',dict(path='created.txt',content='Actual saved content.'));check('custom-key write remains behind ordinary review',d['effect']is None and d['state']['dialog']=='approval');d=event(d['state'],'approve');d=complete(d);d=complete(d);check('conditional create and exact readback use the configured key',files['/share/created.txt']==b'Actual saved content.'and calls[-2][0]=='PUT'and calls[-1][0]=='GET'and calls[-2][2]==expected and calls[-1][2]==expected and'readback verified'in json.dumps(d['ui']))
  d=event(d['state'],'drive_folder');d=event(d['state'],'drive_folder_review',dict(path='created-folder'));d=complete(event(d['state'],'approve'));check('folder creation uses the same credential boundary',calls[-1][0]=='MKCOL'and calls[-1][2]==expected and not d['state']['busy'])
  exported=event(d['state'],'configuration_export');content=exported['effect']['content'];check('configuration export omits key and retained credentials',all(v not in content for v in['controlled-storage-token','saved-user','saved-password'])and json.loads(content)['credentials_omitted']);d=event(exported['state'],'export_cancel',dict(ticket=exported['effect']['ticket']))
  stable=copy.deepcopy(d['state']['drive'])
  invalid=[dict(auth_header=name)for name in['Host','Connection','Content-Length','Transfer-Encoding','Content-Type','Accept-Encoding','X-Key: injected']]+[dict(auth_prefix='Bearer\r\nInjected'),dict(key='secret\nInjected'),dict(key='x'*8193),dict(key=True),dict(auth_prefix=True),dict(auth_header=True)]
  for change in invalid:
   before=len(calls);rejected=save(d['state'],**change);check('invalid active credential refuses atomically '+str(list(change)),rejected['effect']is None and rejected['state']['drive']==stable and rejected['state'].get('notice_error')and len(calls)==before)
  d=save(d['state'],key='');expected={'authorization':basic};d=listing(d['state']);check('clearing the key explicitly restores retained Basic mode',calls[-1][2]==expected)
  d=save(d['state'],key='controlled-storage-token',auth_header='Authorization',auth_prefix='Bearer');expected={'authorization':'Bearer controlled-storage-token'};refuse=True;before=len(calls);d=listing(d['state']);check('401 refusal does not replay with another credential',len(calls)==before+1 and d['effect']is None and not d['state']['busy']and calls[-1][2]==expected);refuse=False
  d=listing(d['state']);check('a scoped authentication refusal preserves later storage use',len(d['state']['remote'])==1 and calls[-1][2]==expected)
  d['state']['profiles']=[dict(name='Primary',endpoint='http://model.test/model',protocol='chat',model='controlled',plain=True,stream=False)];sent=event(d['state'],'send',dict(draft='Keep ordinary text usable.'));check('storage credentials never enter the primary model request','controlled-storage-token'not in sent['effect']['wire']and'saved-password'not in sent['effect']['wire']);ticket=sent['state']['model_expected'];blocked=save(sent['state'],key='replacement');check('editing storage during a model request preserves its current owner',blocked['state']['busy']and blocked['state']['model_expected']==ticket and blocked['effect']is None);d=event(sent['state'],'cancel')
  payload=b'\x00\xffopaque saved payload\x80'
  for streamed in[False,True]:
   label='streamed'if streamed else'legacy'
   for phase in['write','probe','verify']:
    target='/share/'+label+'-snapshot.bin';r=dict(stage_ticket='owned-'+label,purpose='project_transfer:owned-'+phase,bytes=len(payload),phone=False,tree=None,connection=copy.deepcopy(d['state']['drive']),target=url+target.removeprefix('/share/'),mime='application/octet-stream',source=None,phase=phase)
    if streamed:r['format']=2
    stored=copy.deepcopy(r);values=dict(op='project_chunks'if streamed else'project_payload',action='open',record=r,stored=stored,archive=base64.b64encode(payload).decode());e=op(values)
    try:
     wire=base64.b64decode(e['wire_base64'])if'wire_base64'in e else e['wire'].encode();check(label+' '+phase+' retains its pinned key and excludes Basic credentials',b'Authorization: Bearer controlled-storage-token\r\n'in wire and b'Authorization: Basic 'not in wire)
     if streamed and phase=='write':
      with socket.create_connection((e['host'],e['port']),timeout=5)as sock:
       sock.sendall(wire+payload);chunks=[]
       while True:
        part=sock.recv(65536)
        if not part:break
        chunks.append(part)
       received=b''.join(chunks)
     else:received=exchange(e)
     check(label+' '+phase+' reaches the actual authenticated storage peer',received.startswith(b'HTTP/1.0 '+(b'201'if phase=='write'else b'200'))and calls[-1][2]==expected)
     check(label+' '+phase+' preserves exact source payload or readback',calls[-1][3]==(payload if phase=='write'else b'')and files[target]==payload)
    finally:op(dict(op='project_chunks'if streamed else'project_close',action='close',handle=e['snapshot_handle']))
    changed=copy.deepcopy(stored);changed['connection']['key']='another-key';rejected=raw(dict(values,stored=changed));check(label+' '+phase+' refuses changed pinned credentials before transport','error'in rejected and'value'not in rejected)
 finally:p.stdin.close();p.wait(timeout=10);server.shutdown();server.server_close();thread.join(timeout=5)
record=dict(passed=len(passed),checks=passed,scope='Owner public core events and actual loopback WebDAV sockets for no-auth/Basic/Bearer/custom headers, conditional create/readback/folder operations, and legacy/current opaque snapshot writes/probes/readback. Model callbacks and stored records are controlled inputs. Live user NAS/cloud account and Android secure-state acceptance remains separate; no application-owned cloud OAuth or SMB is claimed.')
(PROJECT/'output/checks/storage_auth.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record,indent=2))
