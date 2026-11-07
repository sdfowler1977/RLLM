"""Device grant lifecycle through actual local HTTP and the public event interface."""
from transport import upload,response
import copy,http.server,json,pathlib,secrets,socket,subprocess,tempfile,threading,time,urllib.parse

PROJECT=pathlib.Path(__file__).resolve().parents[1]
checks=[];received=[];sessions={};refreshes={};options={}
def check(name,condition):
    assert condition,name
    checks.append(name)
class Peer(http.server.BaseHTTPRequestHandler):
    def log_message(self,*args):pass
    def reply(self,status,value):
        b=json.dumps(value).encode();self.send_response(status);self.send_header('Content-Length',str(len(b)));self.end_headers();self.wfile.write(b)
    def do_POST(self):
        raw=self.rfile.read(int(self.headers['Content-Length']));received.append((self.path,dict(self.headers),raw))
        if self.path=='/device':
            q=urllib.parse.parse_qs(raw.decode());assert q['client_id']==['public-device'];assert 'Authorization'not in self.headers
            code=secrets.token_urlsafe(32);sessions[code]=dict(ready=False)
            value=dict(device_code=code,user_code='CODE-7392',verification_uri=origin+'/verify',expires_in=120,interval=1)
            value.update(options.get('response',{}));return self.reply(options.get('status',200),value)
        if self.path=='/token':
            q=urllib.parse.parse_qs(raw.decode());grant=q['grant_type'][0];assert q['client_id']==['public-device'];assert 'Authorization'not in self.headers
            if grant=='urn:ietf:params:oauth:grant-type:device_code':
                assert q['device_code'][0]in sessions
                error=options.get('error','authorization_pending')
                if error:return self.reply(400,dict(error=error,error_description='PRIVATE_SERVER_DETAIL'))
            else:assert grant=='refresh_token'and refreshes.pop(q['refresh_token'][0],None)
            token=secrets.token_urlsafe(32);refresh=secrets.token_urlsafe(32);refreshes[refresh]=True
            return self.reply(200,dict(token_type=options.get('token_type','Bearer'),access_token=token,refresh_token=refresh,expires_in=3600))
        fmt=self.path.rsplit('/',1)[-1];assert self.headers.get('Authorization','').startswith('Bearer ')
        reply={'chat':{'choices':[{'message':{'content':'Device-authorized response.'},'finish_reason':'stop'}]},'local':{'message':{'content':'Device-authorized response.'},'done':True},'messages':{'role':'assistant','content':[{'type':'text','text':'Device-authorized response.'}],'stop_reason':'end_turn'},'content':{'candidates':[{'content':{'parts':[{'text':'Device-authorized response.'}]},'finishReason':'STOP'}]}}[fmt]
        self.reply(200,reply)

server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Peer)
threading.Thread(target=server.serve_forever,daemon=True).start();origin=f'http://127.0.0.1:{server.server_port}'
with tempfile.TemporaryDirectory(prefix='device-authorization-',dir=PROJECT/'output') as directory:
    root=pathlib.Path(directory)
    process=None
    def boot():return subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    process=boot()
    def op(v):
        process.stdin.write(json.dumps(v)+'\n');process.stdin.flush();r=json.loads(process.stdout.readline());assert 'value'in r,r;return r['value']
    def event(st,a,inputs=None):return op(dict(op='event',state=st,action=a,inputs=inputs or{},root=str(root)))
    def setup(fmt='chat',**overrides):
        options.clear();d=event(None,'resume');d=event(d['state'],'profile_new');d=event(d['state'],'profile_save',dict(name='Private model',protocol=fmt,endpoint=origin+'/model/'+fmt,model='controlled',key='retained-api-key',stream=False));d=event(d['state'],'oauth_settings')
        cfg=dict(device_endpoint=origin+'/device',token_endpoint=origin+'/token',client_id='public-device',scope='model offline',plain=True);cfg.update(overrides)
        return event(d['state'],'oauth_save',cfg)
    def exchange(d):
        e=d['effect'];assert e and e['kind']=='network',d['state'].get('notice')
        wire=e['wire'].encode()
        with socket.create_connection((e['host'],e['port']),timeout=5)as s:
            s.sendall(wire);upload(op,e,s);fields=response(op,e,s)
        fields=dict(fields,purpose=e['purpose']);fields.update({k:e[k]for k in ['model_ticket','child_ticket','child_owner']if k in e})
        return event(d['state'],'network_result',fields),fields
    def start(d):
        started=event(d['state'],'oauth_device_start');return exchange(started)
    def poll(d,delay=1.05):
        time.sleep(delay);return event(d['state'],'oauth_device_poll')
    try:
        for fmt in ['chat','local','messages','content']:
            d=setup(fmt);check(fmt+' accepts a device-only saved public-client configuration',not d['state'].get('notice_error'))
            check(fmt+' native authorization form exposes device method','oauth_device_start'in json.dumps(d['ui']))
            waiting,initial=start(d);code=next(reversed(sessions));encoded=json.dumps(waiting)
            check(fmt+' private device credential remains out of UI and saved state',code not in encoded and waiting['state']['dialog']=='oauth_device'and waiting['effect']is None)
            check(fmt+' actual user code and verification address are displayed','CODE-7392'in json.dumps(waiting['ui'])and origin+'/verify'in json.dumps(waiting['ui']))
            immediate=event(waiting['state'],'oauth_device_poll');check(fmt+' early polling emits no network request',immediate['effect']is None and not immediate['state']['busy'])
            opened=event(waiting['state'],'oauth_device_open');check(fmt+' browser opens only by explicit action',opened['effect']==dict(kind='oauth_browser',url=origin+'/verify'))
            recovered=event(opened['state'],'resume');check(fmt+' same-process activity return preserves waiting flow without network',recovered['effect']is None and recovered['state']['dialog']=='oauth_device')
            pending=poll(recovered);late=event(pending['state'],'network_result',initial);check(fmt+' previous phase callback cannot satisfy a new poll',late['state']==pending['state']and late['effect']is None)
            waiting,pending_result=exchange(pending);check(fmt+' actual pending response preserves user instructions',waiting['effect']is None and waiting['state']['dialog']=='oauth_device'and not waiting['state']['busy'])
            options['error']='';pending=poll(waiting);late=event(pending['state'],'network_result',pending_result);check(fmt+' prior poll callback cannot satisfy the next request',late['state']==pending['state']and late['effect']is None)
            done,callback=exchange(pending);profile=done['state']['profiles'][0];check(fmt+' actual grant adopts bearer and retains key',profile['oauth_enabled']and profile['key']=='retained-api-key'and profile['oauth_tokens']['access_token'])
            duplicate=event(done['state'],'network_result',callback);check(fmt+' successful grant callback remains single-use',duplicate['state']==done['state']and duplicate['effect']is None)
            sent=event(done['state'],'send',dict(draft='Use the device-authorized model.'));check(fmt+' grant reaches original configured model','Bearer '+profile['oauth_tokens']['access_token']in sent['effect']['wire']);answered,_=exchange(sent);check(fmt+' actual model response completes normally',not answered['state']['busy'])
            refreshed=event(answered['state'],'oauth_refresh');done,_=exchange(refreshed);check(fmt+' device grant uses existing atomic refresh lifecycle',done['state']['profiles'][0]['oauth_tokens']['refresh_token']!=profile['oauth_tokens']['refresh_token'])
            exported=event(done['state'],'configuration_export');check(fmt+' export excludes credentials',all(v not in json.dumps(exported['effect'])for v in [done['state']['profiles'][0]['oauth_tokens']['access_token'],done['state']['profiles'][0]['oauth_tokens']['refresh_token'],'retained-api-key',code]))
        d=setup();waiting,_=start(d);options['error']='slow_down';waiting,_=exchange(poll(waiting));time.sleep(1.05);early=event(waiting['state'],'oauth_device_poll');check('slow_down permanently adds five seconds before another poll',early['effect']is None and 'Wait'in early['state']['notice']);options['error']='';done,_=exchange(poll(waiting,5.05));check('poll succeeds after increased interval',done['state']['profiles'][0]['oauth_enabled'])
        d=setup();options['response']={'interval':None};waiting,_=start(d);check('null interval is treated as omitted default',waiting['state']['dialog']=='oauth_device');early=event(waiting['state'],'oauth_device_poll');check('default five-second interval is enforced',early['effect']is None);event(waiting['state'],'oauth_cancel')
        d=setup();options['response']={'expires_in':1};waiting,_=start(d);time.sleep(1.05);retired=event(waiting['state'],'status');late=event(retired['state'],'oauth_device_poll');check('expired local grant cannot send another request',late['effect']is None and not late['state']['busy'])
        for error in ['access_denied','expired_token','invalid_client','unknown_error']:
            d=setup();waiting,_=start(d);options['error']=error;done,_=exchange(poll(waiting));check(error+' retires the flow without exposing server details',done['effect']is None and done['state'].get('notice_error')and'PRIVATE_SERVER_DETAIL'not in done['state']['notice']);check(error+' leaves existing API key available',done['state']['profiles'][0]['key']=='retained-api-key')
        for label,reply in [('empty code',{'device_code':''}),('control code',{'user_code':'bad\ncode'}),('non-http URI',{'verification_uri':'file:///private'}),('private URI fields',{'verification_uri':origin+'/verify?device_code=secret'}),('negative lifetime',{'expires_in':-1}),('string timing',{'interval':'1'}),('zero interval',{'interval':0}),('large timing',{'expires_in':86401})]:
            d=setup();options['response']=reply;done,_=start(d);check(label+' is refused without adopting credentials',done['effect']is None and done['state'].get('notice_error')and not done['state']['profiles'][0].get('oauth_enabled'))
        d=setup();waiting,initial=start(d);cancelled=event(waiting['state'],'oauth_cancel');late=event(cancelled['state'],'network_result',initial);check('cancel retains key and rejects late response exactly',late['state']==cancelled['state']and late['effect']is None and cancelled['state']['profiles'][0]['key']=='retained-api-key')
        d=setup();started=event(d['state'],'oauth_device_start');cancelled=event(started['state'],'oauth_cancel');done,callback=exchange(started);late=event(cancelled['state'],'network_result',callback);check('cancelled in-flight initialization cannot adopt or reopen code',late['state']==cancelled['state']and late['effect']is None)
        d=setup();waiting,_=start(d);request=poll(waiting);failed=event(request['state'],'failure',dict(purpose=request['effect']['purpose'],sent=True,error='controlled uncertain result'));again=event(failed['state'],'oauth_device_poll');check('uncertain token exchange retires without replay',again['effect']is None and not again['state']['busy'])
        d=setup();waiting,_=start(d);changed=copy.deepcopy(waiting['state']);changed['profiles'][0]['endpoint']=origin+'/other';refused=event(changed,'oauth_device_poll');check('changed model destination cannot use held device code',refused['effect']is None and not refused['state']['busy'])
        d=setup();refused=event(d['state'],'oauth_device_start',dict(device_endpoint=origin+'/unsaved'));check('unsaved device endpoint cannot receive a request',refused['effect']is None and refused['state'].get('notice_error'))
        d=setup(device_endpoint=origin+'/device?device_code=override');check('configured endpoints cannot override protocol parameters',d['state'].get('notice_error'))
        d=setup();waiting,_=start(d);process.stdin.close();process.wait(timeout=10);process=boot();resumed=event(waiting['state'],'resume');check('new process never reconstructs or replays a private device code',resumed['effect']is None and resumed['state']['dialog']=='oauth_settings'and not resumed['state'].get('oauth_expected'))
    finally:
        process.stdin.close();process.wait(timeout=10);server.shutdown();server.server_close()
record=dict(passed=len(checks),checks=checks,scope='Owner execution through actual local HTTP device/token/model peers and shared event interface across four formats. Explicit rate-limited status checks; no autonomous polling or physical Android/browser/live server acceptance claimed.')
out=PROJECT/'output/checks';out.mkdir(exist_ok=True,parents=True);(out/'device_authorization.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record,indent=2))
