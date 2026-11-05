from transport import upload
"""Owner checks: shared Rust UI/harness plus real local HTTP fixture peers."""
import base64, copy, json, pathlib, socket, subprocess, tempfile, threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PROJECT=pathlib.Path(__file__).resolve().parents[1]
passed=[]; requests=[]
def check(name,value):
    assert value,name
    passed.append(name)
class Peer(BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def do_POST(self):
        body=self.rfile.read(int(self.headers.get('Content-Length','0')))
        requests.append((self.server.server_port,self.path,dict(self.headers),body))
        status=503 if self.path=='/unavailable' else 401 if self.path=='/auth-failed' else 202 if self.path=='/accepted' else 200
        payload=json.dumps({'actual':json.loads(body) if body else {},'job':'server-job','items':[{'value':'returned fixture result'}]}).encode()
        self.send_response(status);self.send_header('Content-Length',str(len(payload)));self.end_headers();self.wfile.write(payload)
    do_GET=do_POST
servers=[ThreadingHTTPServer(('127.0.0.1',0),Peer) for _ in range(2)]
for server in servers: threading.Thread(target=server.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='services-',dir=PROJECT/'output') as temp:
    root=pathlib.Path(temp);(root/'evidence.txt').write_text('actual local file')
    p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def raw(v):
        p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
    def event(st,a,i=None):
        out=raw(dict(op='event',state=st,action=a,inputs=i or {},root=str(root)))
        assert 'value' in out,out
        return out['value']
    def cfg():
        return dict(version=1,hosts=[dict(id=f'host{i}',url=f'http://127.0.0.1:{s.server_port}',plain=True,enabled=True,auth_header='X-Token',auth_prefix='',key=f'fixture-key-{i}') for i,s in enumerate(servers)],routes=[dict(id='main',host='host0',capability='image_generation',path='/generate',method='POST',request={'prompt':{'$input':'/prompt'},'number':{'$input':'/number'}},response='json',response_path='/actual',enabled=True),dict(id='fallback',host='host1',capability='image_generation',path='/generate',method='POST',response='json',response_path='',enabled=True)],defaults={'image_generation':dict(routes=['main','fallback'],fallback_codes=[503],connect_fallback=True)})
    def configured(config=None):
        st=event(None,'resume')['state'];d=event(st,'service_connections');d=event(d['state'],'service_save',{'service_config':json.dumps(config or cfg())});assert not d['state'].get('notice_error'),d['state'].get('notice');return d['state']
    def review(st,route='main',capability='',input=None):
        d=event(st,'service_request');d=event(d['state'],'service_review',dict(service_route=route,service_capability=capability,service_input=json.dumps(input or {'prompt':'actual input','number':7})));assert d['state'].get('dialog')=='approval',d['state'].get('notice');return d
    def send_effect(d):
        e=d['effect'];assert e and e['kind']=='network',d['state'].get('notice')
        wire=base64.b64decode(e['wire_base64']) if 'wire_base64'in e else e['wire'].encode()
        with socket.create_connection((e['host'],e['port']),timeout=5) as sock:
            sock.sendall(wire);upload(lambda v:raw(v)['value'],e,sock);out=b''
            while data:=sock.recv(8192):out+=data
        fields={k:e[k] for k in ['purpose','child_ticket','child_owner'] if k in e};fields['data']=base64.b64encode(out).decode()
        return event(d['state'],'network_result',fields),fields
    def model_reply(d,fmt,content):
        value={'chat':{'choices':[{'message':{'content':content},'finish_reason':'stop'}]},'local':{'message':{'content':content},'done':True},'messages':{'content':[{'type':'text','text':content}],'stop_reason':'end_turn'},'content':{'candidates':[{'content':{'parts':[{'text':content}]},'finishReason':'STOP'}]}}[fmt]
        data=json.dumps(value).encode();wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(data)).encode()+b'\r\n\r\n'+data
        fields=dict(purpose='model',model_ticket=d['state']['model_expected'],data=base64.b64encode(wire).decode());fields.update({k:d['effect'][k] for k in ['child_ticket','child_owner'] if k in d['effect']})
        return event(d['state'],'network_result',fields)
    try:
        st=configured();editor=event(st,'service_connections');fields=editor['ui']['modal']['fields'];masked=json.loads(next(f['value'] for f in fields if f['key']=='service_config'))
        check('configuration editor masks existing keys','fixture-key' not in json.dumps(editor['ui']['modal']) and masked['hosts'][0]['key']=='<unchanged>')
        saved=event(editor['state'],'service_save',dict(service_config=json.dumps(masked)));check('masked save retains original credentials',saved['state']['service_connections']['hosts'][0]['key']=='fixture-key-0')
        changed=copy.deepcopy(masked);changed['hosts'][0]['url']='http://changed.invalid';refused=event(editor['state'],'service_save',dict(service_config=json.dumps(changed)));check('host change cannot silently retain credentials',refused['state'].get('notice_error') and refused['state']['service_connections']==st['service_connections'])
        config=cfg();config['hosts'][0]['headers']={'X-Extra':'secret-header'};st=configured(config);editor=event(st,'service_connections');check('custom header values are masked','secret-header' not in json.dumps(editor['ui']['modal']))
        config['hosts'][0]['headers']={'X-Extra':'one','x-extra':'two'};refused=event(event(None,'resume')['state'],'service_save',dict(service_config=json.dumps(config)));check('case variant duplicate headers are refused',refused['state'].get('notice_error'))
        st=configured();d=review(st);check('review discloses exact destination without host key','/generate' in json.dumps(d['ui']['modal']) and 'fixture-key' not in json.dumps(d['ui']['modal']))
        d=event(d['state'],'approve');check('shared approval dispatches real network effect',d['effect']['purpose'].startswith('service_request:'))
        wrong=event(d['state'],'network_result',{'purpose':'service_request:wrong','draft':'injected'});check('wrong callback is exact state no-op',wrong['state']==d['state'])
        completed,callback=send_effect(d);check('typed request mapping executes exact input',json.loads(requests[-1][3])=={'prompt':'actual input','number':7});check('host credential sent only to selected server',requests[-1][2]['X-Token']=='fixture-key-0')
        check('response mapping retains actual fixture response',completed['state']['service_result']['value']=={'prompt':'actual input','number':7})
        duplicate=event(completed['state'],'network_result',dict(callback,draft='late'));check('duplicate callback is exact state no-op',duplicate['state']==completed['state'])
        config=cfg();config['routes'][0]['path']='/unavailable';st=configured(config);d=event(review(st,route='',capability='image_generation')['state'],'approve');first,_=send_effect(d);check('explicit503 advances exactly one configured fallback',first['effect'] and first['effect']['port']==servers[1].server_port);finished,_=send_effect(first);check('fallback has independent host credentials',requests[-1][2]['X-Token']=='fixture-key-1' and finished['state']['service_result']['route']=='fallback')
        config=cfg();config['routes'][0]['path']='/auth-failed';d=event(review(configured(config),route='',capability='image_generation')['state'],'approve');n=len(requests);failed,_=send_effect(d);check('unlisted401 is scoped and never cascades',failed['effect'] is None and len(requests)==n+1 and failed['state']['service_result']['status']=='failed')
        config=cfg();config['routes'][0]['path']='/accepted';config['routes'][0]['response_path']='';d=event(review(configured(config),route='',capability='image_generation')['state'],'approve');accepted,_=send_effect(d);check('202 remains acceptance and no resubmission',accepted['effect'] is None and accepted['state']['service_result']['status']=='accepted')
        st=configured();pending=review(st);changed=copy.deepcopy(pending['state']);changed['service_connections']['hosts'][0]['key']='changed-key';blocked=event(changed,'approve');check('changed credentials invalidate displayed review',blocked['effect'] is None and blocked['state']['notice_error'])
        d=event(review(st)['state'],'approve');cancelled=event(d['state'],'cancel');late=event(cancelled['state'],'network_result',dict(purpose=d['effect']['purpose'],data='bad',draft='late'));check('cancelled result cannot change draft or state',late['state']==cancelled['state'])
        d=event(review(st)['state'],'approve');changed=copy.deepcopy(d['state']);changed['service_connections']['hosts'][0]['key']='changed';blocked,_=send_effect(dict(d,state=changed));check('changed callback context withholds all response values',blocked['state']['service_result']['status']=='failed' and 'withheld' in blocked['state']['service_result']['error'])
        for sent in [False,True]:
            d=event(review(st,route='',capability='image_generation')['state'],'approve');failed=event(d['state'],'failure',dict(purpose=d['effect']['purpose'],sent=sent,error='controlled transport failure'));check('pre-send fallback' if not sent else 'uncertain sent request never replays',bool(failed['effect'])== (not sent));event(failed['state'],'cancel')
        for mutation in ['header','duplicate','pointer','capability']:
            config=cfg()
            if mutation=='header':config['hosts'][0]['headers']={'Content-Length':'7'}
            elif mutation=='duplicate':config['routes'].append(copy.deepcopy(config['routes'][0]))
            elif mutation=='pointer':config['routes'][0]['response_path']='not-a-pointer'
            else:config['routes'][1]['capability']='other'
            d=event(event(st,'service_connections')['state'],'service_save',{'service_config':json.dumps(config)});check('invalid '+mutation+' rejected without replacing working settings',d['state'].get('notice_error') and d['state']['service_connections']==st['service_connections'])
        config=cfg();config['hosts'][0]['key']='';st_keyless=configured(config);d=event(review(st_keyless)['state'],'approve');keyless,_=send_effect(d);check('keyless hosts work','X-Token' not in requests[-1][2] and keyless['state']['service_result']['status']=='response_received')
        for fmt in ['chat','local','messages','content']:
            model=configured();model['profiles']=[dict(name='Controlled',endpoint='http://model.test/v1/chat/completions',protocol=fmt,model='controlled',plain=True,stream=False)];model['agents']=[dict(name='General',tools='read,service',investigation_preferences=False)]
            d=event(model,'send',{'draft':'Use the configured image route.'});check(fmt+' service catalog excludes host credentials','service' in d['effect']['wire'] and 'fixture-key' not in d['effect']['wire'])
            d=model_reply(d,fmt,json.dumps(dict(tool='service',args=dict(route='main',input={'prompt':'model input','number':9}))));check(fmt+' model proposal uses ordinary approval',d['state'].get('dialog')=='approval')
            d=event(d['state'],'approve');d,_=send_effect(d);check(fmt+' actual result returns to requesting model',d['effect'] and d['effect']['purpose']=='model' and 'model input' in d['effect']['wire']);d=model_reply(d,fmt,'The actual fixture result was returned.');check(fmt+' answer completes',not d['state']['busy'])
            model['service_connections']={'version':999};model['profiles'].append({'protocol':'unsupported'});d=event(model,'send',{'draft':'Ordinary chat still works.'});check(fmt+' malformed extras do not block primary',d['effect'] and d['effect']['purpose']=='model');event(d['state'],'cancel')
        optional=configured();optional['service_connections']['routes'][1]['enabled']=False;d=event(review(optional,route='',capability='image_generation')['state'],'approve');done,_=send_effect(d);check('disabled fallback cannot block primary service',done['state']['service_result']['status']=='response_received')
        optional=configured();optional['service_connections']['routes'].append(dict(id='unrelated',host='absent',capability='other',path='invalid',method='POST',response='invalid',enabled=True));d=event(review(optional)['state'],'approve');done,_=send_effect(d);check('malformed unrelated service remains scoped',done['state']['service_result']['status']=='response_received')
        for fmt in ['chat','local','messages','content']:
            model=configured();model['profiles']=[dict(name='Parent',endpoint='http://parent.test/v1/chat/completions',protocol=fmt,model='parent',plain=True,stream=False),dict(name='Child',endpoint='http://child.test/v1/chat/completions',protocol=fmt,model='child',plain=True,stream=False)];model['agents']=[dict(name='Parent',model='0',tools='delegate',subagents=[1]),dict(name='Child',model='1',tools='service',role='coding')]
            d=event(model,'send',{'draft':'Ask the child to get server data.'});d=model_reply(d,fmt,json.dumps(dict(tool='delegate',args=dict(agent=1,task='Use the configured service.'))));d=event(d['state'],'approve');check(fmt+' child starts on captured route',d['state']['request_index']==1)
            d=model_reply(d,fmt,json.dumps(dict(tool='service',args=dict(route='main',input={'prompt':'actual child input','number':3}))));check(fmt+' child service uses review',d['state']['dialog']=='approval');d=event(d['state'],'approve');d,_=send_effect(d);check(fmt+' actual result returns to child',d['effect']and d['state']['request_index']==1 and 'actual child input'in d['effect']['wire']);d=model_reply(d,fmt,'ACTUAL_CHILD_SERVICE_RESULT');check(fmt+' child returns to captured parent',d['effect']and d['state']['request_index']==0 and 'ACTUAL_CHILD_SERVICE_RESULT'in d['effect']['wire']);d=model_reply(d,fmt,'Parent answer.');check(fmt+' child service settles',not d['state']['busy']and not d['state'].get('delegation_stack'))
        actual=raw(dict(op='tool',tool='read',root=str(root),args={'path':'evidence.txt'}));check('model-free local work remains available',actual.get('value')=='actual local file')
    finally:
        p.stdin.close();p.wait(timeout=10)
        for server in servers:server.shutdown();server.server_close()
report={'passed':len(passed),'checks':passed,'scope':'Owner checks with actual local HTTP fixture peers and controlled model responses; no device or live configured provider acceptance.'}
(PROJECT/'output/checks/services.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
