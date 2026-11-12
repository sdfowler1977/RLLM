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
  if auth not in['Bearer primary-key','Bearer child-key']and scope!='conversation':self.reply(401);return
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
  d=setup();check('storage authorization configures without any model',not d['state']['profiles']and d['state']['oauth_target']=='storage'and not d['state'].get('notice_error'))
  opened=event(d['state'],'drive_edit');check('network connection exposes actual authorization route',any(b['event']=='storage_oauth_settings'for b in opened['ui']['modal']['buttons']))
  d=event(opened['state'],'storage_oauth_settings');check('storage authorization has its own visible owner',d['ui']['modal']['title']=='Network storage authorization');fields={f['key']:f for f in d['ui']['modal']['fields']};check('authorization review identifies credential use and destination','network storage connection only'in fields['authorization_owner']['value']and'127.0.0.1'in fields['authorization_destination']['value'])
  started,uri=browser(d);q=urllib.parse.parse_qs(urllib.parse.urlsplit(started['effect']['url']).query);check('storage browser flow uses a fresh external S256 request',len(q['state'][0])==43 and q['code_challenge_method']==['S256']and'code_verifier'not in q)
  wrong=event(started['state'],'oauth_return',dict(uri=uri.replace('state=','state=wrong')));check('wrong storage return is an exact no-op',wrong['state']==started['state']and wrong['effect']is None)
  pending=event(started['state'],'oauth_return',dict(uri=uri));check('storage token transport owns one exact request',pending['effect']['purpose'].startswith('oauth_token:')and pending['state']['busy'])
  d,callback=complete(pending);drive=copy.deepcopy(d['state']['drive']);token=drive['oauth_tokens']['access_token'];refresh=drive['oauth_tokens']['refresh_token'];check('real authorization adopts tokens only into storage',drive['oauth_enabled']and not d['state']['profiles']and accesses[token]=='storage')
  check('storage grant retains every manually configured credential',all(drive[k]==v for k,v in dict(user='retained-user',password='retained-password',key='retained-key',auth_header='X-Storage-Key',auth_prefix='Manual').items()))
  check('authorization UI omits access and refresh tokens',token not in json.dumps(d['ui'])and refresh not in json.dumps(d['ui']))
  late=event(d['state'],'network_result',callback);check('storage token callback cannot adopt twice',late['state']==d['state']and late['effect']is None)
  opened=event(d['state'],'drive_edit');status=next(f['value']for f in opened['ui']['modal']['fields']if f['key']=='auth_status');check('network settings show active authorization without secrets','Storage authorization active'in status and token not in status)
  d,_=complete(event(opened['state'],'drive_list'));check('real WebDAV listing uses storage token and no manual header',calls[-1][2].get('authorization')=='Bearer '+token and'x-storage-key'not in calls[-1][2]and len(d['state']['remote'])==1)
  d,_=complete(event(d['state'],'remote:0'));check('real authorized read completes ordinary viewer','Actual authorized storage text.'in json.dumps(d['ui']))
  d=event(d['state'],'drive_editor');d=event(d['state'],'drive_create_review',dict(path='authorized.txt',content='Actual OAuth-owned write.'));check('authorized writes retain ordinary user review',d['effect']is None and d['state']['dialog']=='approval');d,_=complete(event(d['state'],'approve'));d,_=complete(d);check('authorized conditional write and exact readback complete',files['/share/authorized.txt']==b'Actual OAuth-owned write.'and'readback verified'in json.dumps(d['ui']))
  export=event(d['state'],'configuration_export');check('configuration export omits manual and authorization credentials',all(v not in export['effect']['content']for v in[token,refresh,'retained-key','retained-user','retained-password']));d=event(export['state'],'export_cancel',dict(ticket=export['effect']['ticket']))
  manual=event(event(d['state'],'storage_oauth_settings')['state'],'oauth_key_mode');manual,_=complete(event(manual['state'],'drive_list'));check('explicit manual-mode choice restores configured key without losing grant',calls[-1][2].get('x-storage-key')=='Manual retained-key'and'authorization'not in calls[-1][2]and manual['state']['drive']['oauth_tokens']['access_token']==token)
  d=authorize(event(manual['state'],'storage_oauth_settings'));token=d['state']['drive']['oauth_tokens']['access_token']
  expired=copy.deepcopy(d['state']);expired['drive']['oauth_tokens']['expires_at']=1;before=len(calls);blocked=event(expired,'drive_list');check('expired storage grant refuses only its request with no credential fallback',blocked['effect']is None and blocked['state'].get('notice_error')and len(calls)==before)
  renewal=event(event(blocked['state'],'storage_oauth_settings')['state'],'oauth_refresh');check('storage refresh marks only its own uncertain credential',renewal['state']['drive']['oauth_tokens']['refresh_uncertain']and not renewal['state']['profiles']);d,_=complete(renewal);check('real storage refresh rotates credentials and retains manual key',d['state']['drive']['oauth_tokens']['access_token']!=token and d['state']['drive']['key']=='retained-key'and not d['state']['drive']['oauth_tokens'].get('refresh_uncertain'))
  for fmt in ['chat','local','messages','content']:
   st=copy.deepcopy(d['state']);st['profiles']=[dict(name='Primary',protocol=fmt,endpoint=origin+'/model/'+fmt,model='controlled',key='primary-key',plain=True,stream=False,auth_header='Authorization',auth_prefix='Bearer')]
   sent=event(st,'send',dict(draft='Keep text independent of storage.'));check(fmt+' primary request never borrows storage credentials',sent['effect']and token not in json.dumps(sent['effect'])and'retained-key'not in json.dumps(sent['effect'])and'storage-access-'not in json.dumps(sent['effect']));done,_=complete(sent);check(fmt+' real primary reply completes with storage still authorized','Actual primary response.'in json.dumps(done['state']['chats'])and done['state']['drive']==st['drive'])
   busy=event(st,'send',dict(draft='Keep this primary owner.'));owner=busy['state']['model_expected'];refused=event(busy['state'],'storage_oauth_settings');check(fmt+' starting storage authorization cannot replace a primary operation',refused['effect']is None and refused['state']['busy']and refused['state']['model_expected']==owner);event(refused['state'],'cancel')
  d=done;st=copy.deepcopy(d['state']);st['profiles'][0]['protocol']='chat';st['profiles'][0]['endpoint']=origin+'/model/chat';d=event(st,'oauth_settings');check('model authorization explicitly selects its own owner after storage settings',d['state']['oauth_target']=='model'and d['ui']['modal']['title']=='Connection authorization');d=event(d['state'],'oauth_save',dict(authorization_endpoint=origin+'/authorize',token_endpoint=origin+'/token',client_id='public-client',scope='conversation',issuer=origin,plain=True));d=authorize(d);model_token=d['state']['profiles'][0]['oauth_tokens']['access_token'];storage_token=d['state']['drive']['oauth_tokens']['access_token'];check('model grant does not overwrite storage credentials',accesses[model_token]=='conversation'and accesses[storage_token]=='storage'and model_token!=storage_token)
  d=event(d['state'],'storage_oauth_settings');listed,_=complete(event(d['state'],'drive_list'));check('storage request never borrows an authorized model token',calls[-1][2].get('authorization')=='Bearer '+storage_token and model_token not in repr(calls[-1]))
  d,_=complete(event(listed['state'],'send',dict(draft='Use only model authorization.')));check('authorized model still completes while storage was selected','Actual primary response.'in json.dumps(d['state']['chats'])and calls[-1][2].get('Authorization')=='Bearer '+model_token)
  st=copy.deepcopy(d['state']);st['profiles'][0]['oauth_tokens']['expires_at']=int(time.time())+20;pending=event(st,'send',dict(draft='Refresh the model, not storage.'));check('automatic model refresh ignores selected storage settings',pending['effect']['purpose'].startswith('oauth_token:')and pending['state']['profiles'][0]['oauth_tokens']['refresh_uncertain']and not pending['state']['drive']['oauth_tokens'].get('refresh_uncertain'));sent,_=complete(pending);d,_=complete(sent);check('automatic model refresh resumes the exact primary turn','Actual primary response.'in json.dumps(d['state']['chats'])and d['state']['drive']==st['drive'])
  waiting,waiting_uri=browser(event(d['state'],'storage_oauth_settings'));primary=event(waiting['state'],'send',dict(draft='Continue text while storage sign-in is waiting.'));check('waiting optional storage sign-in does not block the primary model',primary['effect']and primary['effect']['purpose']=='model'and primary['state']['busy']);finished,_=complete(primary);check('primary reply completes while an unfinished storage sign-in is retired','Actual primary response.'in json.dumps(finished['state']['chats'])and finished['state']['drive']==d['state']['drive']);late=event(finished['state'],'oauth_return',dict(uri=waiting_uri));check('retired storage sign-in cannot adopt after the primary reply',late['state']==finished['state']and late['effect']is None)
  start=event(event(d['state'],'storage_oauth_settings')['state'],'oauth_refresh');failed=event(start['state'],'failure',dict(purpose=start['effect']['purpose'],sent=False,error='Controlled pre-send refusal'));check('confirmed storage pre-send failure permits explicit retry',not failed['state']['drive']['oauth_tokens']['refresh_uncertain']);retry=event(failed['state'],'oauth_refresh');d,_=complete(retry)
  start=event(d['state'],'oauth_refresh');failed=event(start['state'],'failure',dict(purpose=start['effect']['purpose'],sent=True,error='Controlled uncertain outcome'));before=len(calls);retry=event(failed['state'],'oauth_refresh');check('uncertain storage refresh is never replayed automatically or explicitly',retry['effect']is None and retry['state']['drive']['oauth_tokens']['refresh_uncertain']and len(calls)==before)
  d=authorize(event(retry['state'],'storage_oauth_settings'));started,uri=browser(d);cancelled=event(started['state'],'oauth_cancel');late=event(cancelled['state'],'oauth_return',dict(uri=uri));check('cancelled storage browser return cannot change credentials',late['state']==cancelled['state']and late['effect']is None)
  started,uri=browser(event(cancelled['state'],'storage_oauth_settings'));restored=event(started['state'],'resume');check('storage browser recovery retains its typed owner',restored['state']['oauth_target']=='storage'and restored['state']['dialog']=='oauth_wait'and restored['state']['oauth_pending']['target']=='storage');d,_=complete(event(restored['state'],'oauth_return',dict(uri=uri)));check('recovered storage browser exchange updates storage only',d['state']['drive']['oauth_enabled']and accesses[d['state']['drive']['oauth_tokens']['access_token']]=='storage')
  started,uri=browser(d);changed=copy.deepcopy(started['state']);changed['drive']['url']=origin+'/different/';late=event(changed,'oauth_return',dict(uri=uri));check('changed storage destination refuses old browser callback without transport',late['state']==changed and late['effect']is None)
  st=copy.deepcopy(d['state']);st['drive']['url']=origin+'/different/';before=len(calls);blocked=event(st,'drive_list');check('saved grant never follows a changed storage destination or falls back',blocked['effect']is None and blocked['state'].get('notice_error')and len(calls)==before)
  device=event(event(d['state'],'storage_oauth_settings')['state'],'oauth_device_start');device,_=complete(device);check('device-code storage authorization reaches its owned review',device['state']['dialog']=='oauth_device'and device['state']['oauth_target']=='storage');early=event(device['state'],'oauth_device_poll');check('storage device polling respects server interval',early['effect']is None and'Wait'in early['state'].get('notice',''));time.sleep(1.1);polled=event(early['state'],'oauth_device_poll');d,_=complete(polled);check('actual storage device grant adopts only storage credentials',d['state']['drive']['oauth_enabled']and accesses[d['state']['drive']['oauth_tokens']['access_token']]=='storage'and d['state']['profiles'][0]['oauth_tokens']['access_token']!=d['state']['drive']['oauth_tokens']['access_token'])
  cfg=dict(version=1,hosts=[dict(id='speech',url=origin,plain=True,enabled=True)],routes=[dict(id='recognition',host='speech',capability='speech_recognition',path='/recognize',method='POST',request_format='multipart',file_field='audio',request={},response='json',response_path='/text',enabled=True),dict(id='synthesis',host='speech',capability='speech_synthesis',path='/speak',method='POST',request={'text':{'$input':'/text'}},response='binary',artifact_kind='audio',enabled=True)],defaults={'speech_recognition':{'routes':['recognition']},'speech_synthesis':{'routes':['synthesis']}})
  voice=event(d['state'],'service_connections');voice=event(voice['state'],'service_save',dict(service_config=json.dumps(cfg)));voice=event(voice['state'],'voice_open_review');voice=event(voice['state'],'voice_consent');owner=dict(voice_session=voice['effect']['voice_session'],voice_audio_ticket=voice['effect']['voice_audio_ticket']);voice=event(voice['state'],'voice_audio_start',owner);frame=dict(op='voice_audio_feed',data=base64.b64encode(b'\0'*640).decode(),**owner);check('real microphone owner accepts PCM before changing authorization',not op(frame)['ready']);before=len(calls);stopped=event(voice['state'],'storage_oauth_settings');check('opening storage authorization retires the microphone without a network request',not stopped['state']['voice_open']and not stopped['state'].get('voice_consent')and len(calls)==before and stopped['state']['dialog']=='oauth_settings');check('retired microphone cannot accept more PCM','error'in raw(frame));late=event(stopped['state'],'voice_audio_start',owner);check('old voice callback cannot restart capture from authorization settings',late['state']==stopped['state']and late['effect']is None)
  for value in ['true',1,{},[]]:
   st=copy.deepcopy(d['state']);st['drive']['oauth_enabled']=value;before=len(calls);refused=event(st,'drive_list');check('invalid authorization mode never selects manual credentials '+repr(value),refused['effect']is None and refused['state'].get('notice_error')and len(calls)==before)
  payload=b'\x00\xffopaque authorized payload\x80';storage_token=d['state']['drive']['oauth_tokens']['access_token']
  for streamed in [False,True]:
   label='streamed'if streamed else'legacy'
   for phase in ['write','probe','verify']:
    target='/share/'+label+'-oauth.bin';record=dict(stage_ticket='oauth-'+label,purpose='project_transfer:oauth-'+phase,bytes=len(payload),phone=False,tree=None,connection=copy.deepcopy(d['state']['drive']),target=origin+target,mime='application/octet-stream',source=None,phase=phase)
    if streamed:record['format']=2
    values=dict(op='project_chunks'if streamed else'project_payload',action='open',record=record,stored=copy.deepcopy(record),archive=base64.b64encode(payload).decode());e=op(values)
    try:
     check(label+' '+phase+' uses pinned storage authorization only',('Authorization: Bearer '+storage_token+'\r\n').encode()in wire(e)and b'X-Storage-Key'not in wire(e));response=exchange(e,payload if streamed and phase=='write'else b'');check(label+' '+phase+' completes at actual authorized storage peer',response.startswith(b'HTTP/1.0 '+(b'201'if phase=='write'else b'200'))and files[target]==payload)
    finally:
     op(dict(op='project_chunks'if streamed else'project_close',action='close',handle=e['snapshot_handle']))
    mismatch=copy.deepcopy(values);mismatch['stored']['connection']['oauth_tokens']['access_token']='changed-storage-credential';check(label+' '+phase+' refuses a changed pinned storage credential','value'not in raw(mismatch))
  (PROJECT/'output/checks/storage_oauth.json').write_text(json.dumps(dict(passed=len(passed),checks=passed),indent=2)+'\n');print(json.dumps(dict(passed=passed),indent=2))
 finally:
  p.terminate();p.wait(timeout=10);server.shutdown();server.server_close()
