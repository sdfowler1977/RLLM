"""Current shared request fields through native events and real local HTTP peers."""
import argparse,base64,http.server,json,pathlib,socket,subprocess,tempfile,threading
PROJECT=pathlib.Path(__file__).resolve().parents[1]
a=argparse.ArgumentParser();a.add_argument('--controller',default=str(PROJECT/'output/headless/RLLM'));a.add_argument('--probe',action='store_true');args=a.parse_args();passed=[];seen=[]
def check(name,value):assert value,name;passed.append(name)
class Peer(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=self.rfile.read(int(self.headers.get('Content-Length','0')));seen.append((self.path,list(self.headers.items()),body));fmt=self.path.strip('/')
  value={'chat':{'choices':[{'message':{'content':'Actual header-bound reply.'},'finish_reason':'stop'}]},'local':{'message':{'content':'Actual header-bound reply.'},'done':True},'messages':{'content':[{'type':'text','text':'Actual header-bound reply.'}],'stop_reason':'end_turn'},'content':{'candidates':[{'content':{'parts':[{'text':'Actual header-bound reply.'}]},'finishReason':'STOP'}]}}.get(fmt,{'actual':'service reply'})
  raw=json.dumps(value).encode();self.send_response(200);self.send_header('Content-Length',str(len(raw)));self.send_header('Connection','close');self.end_headers();self.wfile.write(raw)
 def do_PROPFIND(self):
  body=self.rfile.read(int(self.headers.get('Content-Length','0')));seen.append((self.path,list(self.headers.items()),body));raw=b'<d:multistatus xmlns:d="DAV:"/>';self.send_response(207);self.send_header('Content-Length',str(len(raw)));self.send_header('Connection','close');self.end_headers();self.wfile.write(raw)
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Peer);origin=f'http://127.0.0.1:{server.server_port}';threading.Thread(target=server.serve_forever,daemon=True).start();(PROJECT/'output/checks').mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(dir=PROJECT/'output',prefix='request-fields-')as root:
 p=subprocess.Popen([str(pathlib.Path(args.controller).resolve()),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def op(v):
  p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert'value'in r,r;return r['value']
 def event(st,action,inputs=None):return op(dict(op='event',root=root,state=st,action=action,inputs=inputs or{}))
 def transfer(d,name):
  e=d['effect'];assert e and e['kind']=='network',d['state'].get('notice');raw=base64.b64decode(e['wire_base64'])if'wire_base64'in e else e['wire'].encode();head,body=raw.split(b'\r\n\r\n',1)
  for managed in [b'host:',b'connection:',b'accept-encoding:',b'content-length:']:
   check(name+' owns exactly one '+managed.decode(),sum(line.lower().startswith(managed)for line in head.split(b'\r\n')[1:])==1)
  with socket.create_connection((e['host'],e['port']),timeout=5)as s:
   s.sendall(raw);reply=b''
   while chunk:=s.recv(65536):reply+=chunk
  fields={k:e[k]for k in ['purpose','model_ticket','child_ticket','child_owner','drive_ticket']if k in e};fields['data']=base64.b64encode(reply).decode();return event(d['state'],'network_result',fields)
 try:
  if args.probe:
   observed=[]
   for name in ['X_API_Key','Expect','TE']:
    d=event(None,'resume');d=event(d['state'],'drive_edit');d=event(d['state'],'drive_save',dict(url=origin+'/share/',plain=True,key='secret',auth_header=name,auth_prefix=''));observed.append(dict(name=name,accepted=not d['state'].get('notice_error',False)))
   print(json.dumps(observed))
  else:
   for fmt in ['chat','local','messages','content']:
    d=event(None,'resume');d=event(d['state'],'profile_new');d=event(d['state'],'profile_save',dict(name='Header model',protocol=fmt,endpoint=origin+'/'+fmt,model='controlled',key='secret-model',plain=True,stream=False));st=d['state'];st['profiles'][0].update(auth_header='X_API_Key',auth_prefix='')
    sent=event(st,'send',dict(draft='Use this configured model.'));done=transfer(sent,fmt);check(fmt+' real peer receives the exact custom field',('X_API_Key','secret-model')in seen[-1][1]);check(fmt+' ordinary response completes',not done['state']['busy'])
   d=event(None,'resume');d=event(d['state'],'drive_edit');d=event(d['state'],'drive_save',dict(url=origin+'/share/',plain=True,key='secret-storage',auth_header='X.Storage_Key',auth_prefix=''));check('native storage settings accept token field syntax',not d['state'].get('notice_error'));st=d['state'];done=transfer(event(st,'drive_list'),'storage');check('actual storage peer receives exact credential field',('X.Storage_Key','secret-storage')in seen[-1][1])
   for name in ['Host','Content-Length','Transfer-Encoding','Connection','Accept-Encoding','TE','Trailer','Expect','Upgrade','Proxy-Connection','bad name','Bad:Key']:
    edit=event(st,'drive_edit');bad=event(edit['state'],'drive_save',dict(auth_header=name,url=origin+'/share/',plain=True,key='secret-storage',auth_prefix=''))
    check(name+' refuses before sending or replacing saved storage settings',bad['effect']is None and bad['state'].get('notice_error')and bad['state']['drive']==st['drive'])
   done=transfer(event(st,'drive_list'),'after-refusal');check('refusal leaves ordinary storage available',not done['state'].get('busy'))
   cfg=dict(version=1,hosts=[dict(id='peer',url=origin,plain=True,enabled=True,auth_header='X_Service_Key',auth_prefix='',key='secret-service',headers={'X.Meta_Field':'metadata'})],routes=[dict(id='main',host='peer',capability='image_generation',path='/service',method='POST',request={'prompt':{'$input':'/prompt'}},response='json',response_path='',enabled=True)],defaults={})
   d=event(None,'resume');d=event(d['state'],'service_connections');d=event(d['state'],'service_save',dict(service_config=json.dumps(cfg)));check('service settings accept the same token syntax: '+str(d['state'].get('notice')),not d['state'].get('notice_error'));st=d['state'];d=event(st,'service_request');d=event(d['state'],'service_review',dict(service_route='main',service_capability='',service_input=json.dumps({'prompt':'actual input'})));d=event(d['state'],'approve');done=transfer(d,'service');check('reviewed service sends exact credential and metadata names',('X_Service_Key','secret-service')in seen[-1][1]and('X.Meta_Field','metadata')in seen[-1][1])
   for name in ['TE','Proxy-Connection']:
    cfg['hosts'][0]['auth_header']=name;edit=event(st,'service_connections');bad=event(edit['state'],'service_save',dict(service_config=json.dumps(cfg)));check('service '+name+' cannot replace current transport settings',bad['effect']is None and bad['state'].get('notice_error')and 'transport framing'in bad['state'].get('notice','')and bad['state']['service_connections']==st['service_connections'])
 finally:p.stdin.close();p.wait(timeout=10);server.shutdown();server.server_close()
if not args.probe:
 result=dict(count=len(passed),checks=passed,scope='Owner shared native settings/ordinary model and reviewed service events; actual local HTTP model, storage and service peers; no installed Android/live user endpoint/independent acceptance.')
 (PROJECT/'output/checks/request-fields.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
