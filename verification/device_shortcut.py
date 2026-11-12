"""Device verification shortcuts: actual local grant HTTP and shared native events."""
import argparse,base64,http.client,http.server,json,pathlib,subprocess,tempfile,threading,urllib.parse
PROJECT=pathlib.Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser();parser.add_argument('--controller',default=str(PROJECT/'output/headless/RLLM'));parser.add_argument('--probe',action='store_true');args=parser.parse_args()
checks=[];mode='valid';issued={};opened=[]
def check(name,value):assert value,name;checks.append(name)
class Peer(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=self.rfile.read(int(self.headers['Content-Length']));assert urllib.parse.parse_qs(body.decode())['client_id']==['public-shortcut']
  code='private-device-'+str(len(issued));uri=origin+'/verify';value=dict(device_code=code,user_code='CODE-7392',verification_uri=uri,expires_in=120,interval=5)
  complete=uri+'?user_code=CODE-7392&session=%2F'
  if mode=='off-origin':complete='https://other.invalid/verify?user_code=CODE-7392'
  elif mode=='private':complete=uri+'?x='+code
  elif mode=='encoded-private':complete=uri+'?x='+''.join('%%%02X'%b for b in code.encode())
  elif mode=='private-field':complete=uri+'?%2564evice_code=hidden'
  elif mode=='malformed':complete='file:///private'
  elif mode=='wrong-type':complete=True
  elif mode=='oversized':complete=uri+'?x='+'x'*2048
  if mode!='omitted':value['verification_uri_complete']=complete
  issued[code]=value;raw=json.dumps(value).encode();self.send_response(200);self.send_header('Content-Length',str(len(raw)));self.end_headers();self.wfile.write(raw)
 def do_GET(self):
  opened.append(self.path);raw=b'Controlled verification page';self.send_response(200);self.send_header('Content-Length',str(len(raw)));self.end_headers();self.wfile.write(raw)
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Peer);origin=f'http://127.0.0.1:{server.server_port}';threading.Thread(target=server.serve_forever,daemon=True).start()
(PROJECT/'output/checks').mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(dir=PROJECT/'output',prefix='device-shortcut-')as temp:
 p=subprocess.Popen([str(pathlib.Path(args.controller).resolve()),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def op(value):
  p.stdin.write(json.dumps(value)+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert'value'in r,r;return r['value']
 def event(st,action,inputs=None):return op(dict(op='event',root=temp,state=st,action=action,inputs=inputs or{}))
 def setup(target):
  d=event(None,'resume')
  if target=='storage':
   d=event(d['state'],'drive_edit');d=event(d['state'],'drive_save',dict(url=origin+'/share/',plain=True,key='retained-key'));d=event(d['state'],'storage_oauth_settings')
  else:
   d=event(d['state'],'profile_new');d=event(d['state'],'profile_save',dict(name='Shortcut model',protocol=target,endpoint=origin+'/model',model='controlled',key='retained-key'));d=event(d['state'],'oauth_settings')
  return event(d['state'],'oauth_save',dict(device_endpoint=origin+'/device',token_endpoint=origin+'/token',client_id='public-shortcut',plain=True))
 def grant(d):
  started=event(d['state'],'oauth_device_start');e=started['effect'];assert e['kind']=='network';c=http.client.HTTPConnection(e['host'],e['port'],timeout=5)
  c.request('POST','/device',body=e['wire'].split('\r\n\r\n',1)[1].encode(),headers={'Content-Type':'application/x-www-form-urlencoded'});r=c.getresponse();body=r.read();c.close();raw=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(body)).encode()+b'\r\n\r\n'+body
  return event(started['state'],'network_result',dict(purpose=e['purpose'],data=base64.b64encode(raw).decode()))
 try:
  targets=['chat']if args.probe else['chat','local','messages','content','storage']
  for target in targets:
   modes=['valid']if args.probe else['valid','omitted','off-origin','private','encoded-private','private-field','malformed','wrong-type','oversized']
   for mode in modes:
    waiting=grant(setup(target));value=next(reversed(issued.values()));code=value['device_code'];ui=json.dumps(waiting['ui']);state=json.dumps(waiting['state']);opened_effect=event(waiting['state'],'oauth_device_open')['effect'];wanted=value['verification_uri_complete']if mode=='valid'else origin+'/verify'
    if args.probe:
     print(json.dumps(dict(observed=opened_effect['url'],advertised=value['verification_uri_complete'],used_shortcut=opened_effect['url']==value['verification_uri_complete'])));event(waiting['state'],'oauth_cancel');break
    check(target+'/'+mode+' keeps manual address and user code',origin+'/verify'in ui and 'CODE-7392'in ui)
    check(target+'/'+mode+' private credential absent from UI/state',code not in ui+state)
    check(target+'/'+mode+' emits no automatic browser or polling effect',waiting['effect']is None and waiting['state']['dialog']=='oauth_device')
    check(target+'/'+mode+' explicit action chooses safe destination',opened_effect==dict(kind='oauth_browser',url=wanted))
    if mode=='valid':
     check(target+' does not persist optional shortcut',value['verification_uri_complete']not in state)
     check(target+' review displays exact shortcut destination',wanted in ui)
     address=urllib.parse.urlsplit(wanted);c=http.client.HTTPConnection(address.hostname,address.port,timeout=5);c.request('GET',address.path+'?'+address.query);r=c.getresponse();r.read();c.close();check(target+' actual controlled browser-target request preserves URI spelling',opened[-1]=='/verify?user_code=CODE-7392&session=%2F')
    cancelled=event(waiting['state'],'oauth_cancel');late=event(cancelled['state'],'oauth_device_open');check(target+'/'+mode+' cancelled grant cannot reopen shortcut',late['effect']is None)
 finally:p.stdin.close();p.wait(timeout=10);server.shutdown();server.server_close()
if not args.probe:
 result=dict(count=len(checks),checks=checks,scope='Owner actual local grant HTTP, native shared-core modal/effects and controlled GET destination across four model formats and storage. No physical Android browser/visual acceptance or live user endpoint.')
 (PROJECT/'output/checks/device-shortcut.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
