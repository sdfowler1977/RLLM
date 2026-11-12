from transport import upload
"""Real asynchronous fixture server, tracked status and validated artifact result."""
import base64,copy,hashlib,io,json,pathlib,socket,subprocess,tempfile,threading,wave
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[];calls=[];phase='running';acceptance=202
def check(name,value):assert value,name;passed.append(name)
buf=io.BytesIO()
with wave.open(buf,'wb')as w:w.setnchannels(1);w.setsampwidth(2);w.setframerate(16000);w.writeframes(b'\0\0'*(3*1024*1024))
artifact=buf.getvalue()
class Peer(BaseHTTPRequestHandler):
    def log_message(self,*a):pass
    def do_POST(self):
        body=self.rfile.read(int(self.headers.get('Content-Length','0')));calls.append((self.command,self.path,body))
        status=acceptance if self.path=='/generate'else 202 if self.path=='/cancel/server-job'else 200
        payload=artifact if self.path=='/result/server-job'else json.dumps({'id':'server-job','state':phase,'asset':'server-job'}).encode()
        self.send_response(status);self.send_header('Content-Length',str(len(payload)));self.end_headers();self.wfile.write(payload)
    do_GET=do_POST
server=ThreadingHTTPServer(('127.0.0.1',0),Peer);threading.Thread(target=server.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='jobs-',dir=PROJECT/'output')as temp:
    p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def op(v):
        p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert'value'in r,r;return r['value']
    def event(st,a,i=None):return op(dict(op='event',state=st,action=a,inputs=i or {},root=temp))
    def transport(d):
        e=d['effect'];assert e,d['state'].get('notice');wire=base64.b64decode(e['wire_base64'])if'wire_base64'in e else e['wire'].encode()
        with socket.create_connection((e['host'],e['port']),timeout=5)as sock:
            sock.sendall(wire);upload(op,e,sock);out=b''
            if e.get('media_download'):
                owner=dict(purpose=e['purpose'],ticket=e['media_download']);op(dict(owner,op='media_stream_cancel',ticket='wrong-ticket'));ready=False
                while part:=sock.recv(8192):
                    response=op(dict(owner,op='media_stream_feed',data=base64.b64encode(part).decode()))
                    if response['ready']:ready=True;break
                receipt=op(dict(owner,op='media_stream_finish',eof=not ready));op(dict(owner,op='media_stream_cancel'));return event(d['state'],'network_result',dict(purpose=e['purpose'],media_receipt=receipt))
            while part:=sock.recv(8192):out+=part
        return event(d['state'],'network_result',dict(purpose=e['purpose'],data=base64.b64encode(out).decode()))
    def request(st,action):
        d=event(st,'job_'+action);assert d['state'].get('dialog')=='approval',d['state'].get('notice');return event(d['state'],'approve')
    try:
        mapping=dict(id='/id',state='/state',success=['done'],failure=['failed'],status_route='status',result_route='result',cancel_route='cancel')
        routes=[dict(id='submit',host='peer',capability='video_generation',method='POST',path='/generate',response='json',response_path='',enabled=True,job=mapping)]
        for action in ['status','result','cancel']:routes.append(dict(id=action,host='peer',capability='job_'+action,method='POST'if action=='cancel'else'GET',path='/'+action+'/{job}',response='binary'if action=='result'else'json',response_path='',artifact_kind='audio'if action=='result'else'data',enabled=True))
        cfg=dict(version=1,hosts=[dict(id='peer',url=f'http://127.0.0.1:{server.server_port}',plain=True,enabled=True)],routes=routes,defaults={})
        d=event(None,'resume');d=event(d['state'],'service_connections');d=event(d['state'],'service_save',dict(service_config=json.dumps(cfg)));assert not d['state'].get('notice_error'),d['state'].get('notice')
        d=event(d['state'],'service_request');d=event(d['state'],'service_review',dict(service_route='submit',service_input='{"prompt":"fixture generation"}'));d=transport(event(d['state'],'approve'));job=d['state']['service_jobs'][0];check('202 tracks actual server job ID',job['server_id']=='server-job'and job['state']=='accepted');check('acceptance does not claim artifact completion',d['state']['service_result']['status']=='accepted'and not job.get('artifact'))
        count=len(calls);d=event(d['state'],'resume');check('resume preserves job without resubmission',d['state']['service_jobs']==[job] and len(calls)==count and d['effect']is None)
        d=event(d['state'],'job_view:'+job['id']);d=transport(request(d['state'],'status'));check('status uses captured ID and separate route',calls[-1][1]=='/status/server-job'and d['state']['service_jobs'][0]['state']=='running')
        phase='done';d=transport(request(d['state'],'status'));check('server success remains ready until actual file exists',d['state']['service_jobs'][0]['state']=='ready'and not d['state']['service_jobs'][0].get('artifact'))
        d=transport(request(d['state'],'result'));record=d['state']['service_jobs'][0];check('only validated returned file completes job',record['state']=='completed'and record['artifact']['sha256']==hashlib.sha256(artifact).hexdigest()and(pathlib.Path(temp)/record['artifact']['path']).read_bytes()==artifact)
        check('status and result never repeat generation',sum(path=='/generate'for method,path,body in calls)==1)
        d=transport(request(d['state'],'cancel'));check('202 cancellation is a request rather than proof',d['state']['service_jobs'][0]['state']=='cancel_requested')
        changed=copy.deepcopy(d['state']);changed['service_connections']['hosts'][0]['url']='http://different.invalid';blocked=event(changed,'job_status');check('changed origin cannot receive tracked server ID',blocked['effect']is None and blocked['state'].get('notice_error'))
        before=copy.deepcopy(d['state']['service_jobs'][0]);started=request(d['state'],'status');failed=event(started['state'],'failure',dict(purpose=started['effect']['purpose'],sent=True,error='fixture interruption'));after=failed['state']['service_jobs'][0];check('failed poll preserves known server state',after['state']==before['state']and after['server_id']==before['server_id']and after.get('last_error'))
        archived=event(failed['state'],'job_archive');check('job archive is reversible',archived['state']['service_jobs'][0]['archived']);restored=event(archived['state'],'job_restore');check('job history restore preserves identity',not restored['state']['service_jobs'][0]['archived']and restored['state']['service_jobs'][0]['server_id']=='server-job')
        acceptance=200;phase='queued';mapped=copy.deepcopy(cfg);mapped['routes'][0]['job'].update(accept_codes=[200],success=[True],failure=[False],result_input={'asset':{'$input':'/asset'}});mapped['routes'][2]['path']='/result/{asset}'
        d=event(d['state'],'service_connections');d=event(d['state'],'service_save',dict(service_config=json.dumps(mapped)));assert not d['state'].get('notice_error'),d['state'].get('notice')
        d=event(d['state'],'service_request');d=event(d['state'],'service_review',dict(service_route='submit',service_input='{}'));d=transport(event(d['state'],'approve'));job=d['state']['service_jobs'][-1];check('configured HTTP 200 submission tracks acceptance',job['state']=='accepted' and d['state']['service_result']['status']=='accepted')
        d=event(d['state'],'job_view:'+job['id']);phase=True;d=transport(request(d['state'],'status'));check('boolean terminal status retains its type',d['state']['service_jobs'][-1]['state']=='ready' and d['state']['service_jobs'][-1]['server_state'] is True)
        d=transport(request(d['state'],'result'));check('result path maps actual status response field',calls[-1][1]=='/result/server-job' and d['state']['service_jobs'][-1]['state']=='completed')
        untouched=copy.deepcopy(d['state']['service_jobs'][0]);before=len(calls);refused=event(d['state'],'job_forget_confirm');check('job record removal requires selected review',refused['state'].get('notice_error') and len(refused['state']['service_jobs'])==2)
        d=event(d['state'],'job_forget');d=event(d['state'],'job_forget_confirm');check('reviewed job removal preserves other records and local artifact',d['state']['service_jobs']==[untouched] and len(calls)==before and (pathlib.Path(temp)/job.get('artifact',record['artifact'])['path']).exists())
        malformed=copy.deepcopy(mapped);malformed['routes'][0]['job']['id']='/missing';d=event(d['state'],'service_connections');d=event(d['state'],'service_save',dict(service_config=json.dumps(malformed)));d=event(d['state'],'service_request');d=event(d['state'],'service_review',dict(service_route='submit',service_input='{}'));before=len(calls);d=transport(event(d['state'],'approve'));check('HTTP 200 acceptance without usable identifier remains unconfirmed without replay',d['state']['service_result']['status']=='accepted_unconfirmed' and len(calls)==before+1)
    finally:p.stdin.close();p.wait(timeout=10);server.shutdown();server.server_close()
report=dict(passed=len(passed),checks=passed,scope='Owner asynchronous HTTP fixture tests. Explicit polling; no background scheduler or live user generation service acceptance.');(PROJECT/'output/checks/jobs.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
