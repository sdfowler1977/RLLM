"""Storage-owned public-client grants, actual HTTP storage and model isolation."""
import base64,copy,hashlib,http.client,http.server,json,pathlib,secrets,socket,subprocess,tempfile,threading,time,urllib.parse

PROJECT=pathlib.Path(__file__).resolve().parents[1]
passed=[];calls=[];codes={};refreshes={};accesses={};devices={};files={'/share/alpha.txt':b'Actual authorized storage text.'}
def check(name,value):
 assert value,name
 passed.append(name)
class Peer(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def reply(self,status,data=b'',headers=None):
  if not isinstance(data,bytes):data=json.dumps(data).encode()
  self.send_response(status)
  for key,value in (headers or{}).items():self.send_header(key,value)
  self.send_header('Content-Length',str(len(data)));self.send_header('Connection','close');self.end_headers();self.wfile.write(data)
 def issue(self,scope):
  access=scope+'-access-'+secrets.token_hex(8);refresh=scope+'-refresh-'+secrets.token_hex(8);accesses[access]=scope;refreshes[refresh]=scope
  self.reply(200,dict(token_type='Bearer',access_token=access,refresh_token=refresh,expires_in=3600))
 def do_GET(self):
  u=urllib.parse.urlsplit(self.path);q=urllib.parse.parse_qs(u.query)
  if u.path=='/authorize':
   code=secrets.token_hex(12);codes[code]=q;location=q['redirect_uri'][0]+'?'+urllib.parse.urlencode(dict(code=code,state=q['state'][0],iss=origin));self.reply(302,headers={'Location':location});return
  if u.path.startswith('/model/'):
   self.model();return
  self.storage()
 def do_POST(self):
  if self.path.startswith('/model/'):self.model();return
  body=self.rfile.read(int(self.headers.get('Content-Length','0')));v={k:x[0]for k,x in urllib.parse.parse_qs(body.decode()).items()};calls.append(('POST',self.path,dict(self.headers),body))
  if self.path=='/device':
   code=secrets.token_hex(12);devices[code]=v.get('scope','storage');self.reply(200,dict(device_code=code,user_code='OWNED-CODE',verification_uri=origin+'/verify',expires_in=120,interval=1));return
  if self.path!='/token':self.reply(404);return
  grant=v.get('grant_type')
  if grant=='authorization_code':
   cfg=codes.pop(v.get('code'),None)
   if not cfg:self.reply(400,dict(error='invalid_grant'));return
   proof=base64.urlsafe_b64encode(hashlib.sha256(v.get('code_verifier','').encode()).digest()).rstrip(b'=').decode()
   if proof!=cfg['code_challenge'][0]or v.get('redirect_uri')!=cfg['redirect_uri'][0]or v.get('client_id')!=cfg['client_id'][0]:self.reply(400,dict(error='invalid_grant'));return
   self.issue(cfg.get('scope',['storage'])[0]);return
  if grant=='refresh_token':
   scope=refreshes.pop(v.get('refresh_token'),None)
   if not scope:self.reply(400,dict(error='invalid_grant'));return
   self.issue(scope);return
  if grant=='urn:ietf:params:oauth:grant-type:device_code':
   scope=devices.pop(v.get('device_code'),None)
   if not scope:self.reply(400,dict(error='invalid_grant'));return
   self.issue(scope);return
  self.reply(400,dict(error='unsupported_grant_type'))
 def model(self):
  body=self.rfile.read(int(self.headers.get('Content-Length','0')));calls.append(('MODEL',self.path,dict(self.headers),body))
  auth=self.headers.get('Authorization','');scope=accesses.get(auth.removeprefix('Bearer '))
  if auth not in['Bearer primary-key','Bearer child-key']and self.headers.get('x-api-key')!='primary-key'and self.headers.get('x-goog-api-key')!='primary-key'and scope!='conversation':self.reply(401);return
  fmt=self.path.rsplit('/',1)[-1];self.reply(200,{'chat':{'choices':[{'message':{'role':'assistant','content':'Actual primary response.'},'finish_reason':'stop'}]},'local':{'message':{'content':'Actual primary response.'},'done':True},'messages':{'role':'assistant','content':[{'type':'text','text':'Actual primary response.'}],'stop_reason':'end_turn'},'content':{'candidates':[{'content':{'parts':[{'text':'Actual primary response.'}]},'finishReason':'STOP'}]}}[fmt])
 def storage(self):
  body=self.rfile.read(int(self.headers.get('Content-Length','0')));headers={k.lower():v for k,v in self.headers.items()};calls.append((self.command,self.path,headers,body))
  scope=accesses.get(headers.get('authorization','').removeprefix('Bearer '))
  if scope!='storage'and headers.get('x-storage-key')!='Manual retained-key':self.reply(401);return
  if self.command=='PROPFIND':self.reply(207,b'<d:multistatus xmlns:d="DAV:"><d:response><d:href>/share/alpha.txt</d:href><d:propstat><d:prop><d:displayname>alpha.txt</d:displayname></d:prop></d:propstat></d:response></d:multistatus>');return
  if self.command=='PUT':
   if self.headers.get('If-None-Match')=='*'and self.path in files:self.reply(412);return
   files[self.path]=body;self.reply(201);return
  if self.command=='MKCOL':self.reply(201);return
  if self.path in files:self.reply(200,files[self.path]);return
  self.reply(404)
 do_PROPFIND=storage;do_PUT=storage;do_MKCOL=storage

server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Peer);threading.Thread(target=server.serve_forever,daemon=True).start();origin=f'http://127.0.0.1:{server.server_port}'
with tempfile.TemporaryDirectory(prefix='storage-oauth-',dir=PROJECT/'output')as folder:
 p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def raw(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
 def op(v):r=raw(v);assert'value'in r,r;return r['value']
 def event(st,a,i=None):return op(dict(op='event',state=st,action=a,inputs=i or{},root=folder))
 def wire(e):return base64.b64decode(e['wire_base64'])if'wire_base64'in e else e['wire'].encode()
 def exchange(e,tail=b''):
  with socket.create_connection((e['host'],e['port']),timeout=5)as sock:
   sock.sendall(wire(e)+tail);chunks=[]
   while True:
    part=sock.recv(65536)
    if not part:break
    chunks.append(part)
  return b''.join(chunks)
 def complete(d):
  e=d['effect'];assert e and e['kind']=='network',d['state'].get('notice');v={k:e[k]for k in['purpose','drive_ticket','model_ticket','child_ticket','child_owner']if k in e};v['data']=base64.b64encode(exchange(e)).decode();return event(d['state'],'network_result',v),v
 def configure(st):
  d=event(st,'storage_oauth_settings');return event(d['state'],'oauth_save',dict(authorization_endpoint=origin+'/authorize',device_endpoint=origin+'/device',token_endpoint=origin+'/token',client_id='public-client',scope='storage',issuer=origin,plain=True))
 def browser(d):
  started=event(d['state'],'oauth_sign_in');e=started['effect'];assert e and e['kind']=='oauth_browser',started['state'].get('notice');u=urllib.parse.urlsplit(e['url']);c=http.client.HTTPConnection(u.hostname,u.port,timeout=5);c.request('GET',u.path+'?'+u.query);r=c.getresponse();assert r.status==302;uri=r.getheader('Location');r.read();c.close();return started,uri
 def authorize(d):
  started,uri=browser(d);return complete(event(started['state'],'oauth_return',dict(uri=uri)))[0]
 def setup():
  d=event(None,'resume');d=event(d['state'],'drive_edit');d=event(d['state'],'drive_save',dict(url=origin+'/share/',plain=True,user='retained-user',password='retained-password',key='retained-key',auth_header='X-Storage-Key',auth_prefix='Manual'));return configure(d['state'])
 try:
  original=Peer.issue
  override={}
  def issue(self,scope):
   if override:
    value=dict(token_type='Bearer',access_token='replacement-access',expires_in=3600);value.update(override);self.reply(200,value)
   else:original(self,scope)
  Peer.issue=issue
  d=authorize(setup());saved=copy.deepcopy(d['state']['drive']['oauth_tokens'])
  for value in [42,True,[],{},['rotated-secret']]:
   override['refresh_token']=value
   fresh=authorize(event(d['state'],'storage_oauth_settings'))
   check(str(type(value))+' malformed initial grant retains exact prior tokens',fresh['state']['drive']['oauth_tokens']==saved and fresh['state'].get('notice_error'))
   refreshes[saved['refresh_token']]='storage'
   pending=event(event(d['state'],'storage_oauth_settings')['state'],'oauth_refresh')
   done,_=complete(pending)
   actual=copy.deepcopy(done['state']['drive']['oauth_tokens']);actual.pop('refresh_uncertain',None)
   check(str(type(value))+' malformed refresh retains original tokens and uncertainty',actual==saved and done['state']['drive']['oauth_tokens'].get('refresh_uncertain') and done['state'].get('notice_error'))
   again=event(done['state'],'oauth_refresh');check('uncertain rotation is never replayed '+str(type(value)),again['effect']is None)
   manual=event(done['state'],'oauth_key_mode');listed,_=complete(event(manual['state'],'drive_list'));check('manual storage remains available '+str(type(value)),len(listed['state']['remote'])==1)
  override.clear()
  refreshes[saved['refresh_token']]='storage'
  renewed,_=complete(event(event(d['state'],'storage_oauth_settings')['state'],'oauth_refresh'));check('valid rotation still replaces refresh credential',renewed['state']['drive']['oauth_tokens']['refresh_token']!=saved['refresh_token'])
  override['refresh_token']=None
  omitted,_=complete(event(event(renewed['state'],'storage_oauth_settings')['state'],'oauth_refresh'));check('null omission retains prior refresh compatibility',omitted['state']['drive']['oauth_tokens']['refresh_token']==renewed['state']['drive']['oauth_tokens']['refresh_token'] and not omitted['state'].get('notice_error'))
  for fmt in ['chat','local','messages','content']:
   override.clear()
   model=event(None,'resume');model=event(model['state'],'profile_new');model=event(model['state'],'profile_save',dict(name='Rotation peer',protocol=fmt,endpoint=origin+'/model/'+fmt,model='controlled',key='primary-key',plain=True,stream=False));model=event(model['state'],'oauth_settings');model=event(model['state'],'oauth_save',dict(authorization_endpoint=origin+'/authorize',token_endpoint=origin+'/token',client_id='public-client',scope='conversation',issuer=origin,plain=True));model=authorize(model);prior=copy.deepcopy(model['state']['profiles'][0]['oauth_tokens'])
   for value in [42,True,[],{},['rotated-secret']]:
    override['refresh_token']=value
    fresh=authorize(event(model['state'],'oauth_settings'));check(fmt+' malformed new grant preserves credentials '+str(type(value)),fresh['state']['profiles'][0]['oauth_tokens']==prior and fresh['state'].get('notice_error'))
    refreshes[prior['refresh_token']]='conversation'
    done,_=complete(event(event(model['state'],'oauth_settings')['state'],'oauth_refresh'));actual=copy.deepcopy(done['state']['profiles'][0]['oauth_tokens']);actual.pop('refresh_uncertain',None);check(fmt+' malformed rotation remains uncertain '+str(type(value)),actual==prior and done['state']['profiles'][0]['oauth_tokens'].get('refresh_uncertain') and done['state'].get('notice_error'))
    manual=event(done['state'],'oauth_key_mode');sent=event(manual['state'],'send',dict(draft='Continue with retained manual credentials.'));answered,_=complete(sent);check(fmt+' scoped failure preserves primary request '+str(type(value)),'Actual primary response.'in json.dumps(answered['state']['chats']))
  record=dict(passed=len(passed),checks=passed,scope='Owner execution through actual local HTTP authorization/token/storage/model peers. Malformed grant and rotation across all four model formats, typed storage owner, manual recovery and single-operation uncertainty; no independent or installed-device acceptance.')
  (PROJECT/'output/checks/credential_rotation.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record,indent=2))
 finally:
  p.stdin.close();p.wait(timeout=10);server.shutdown();server.server_close()
