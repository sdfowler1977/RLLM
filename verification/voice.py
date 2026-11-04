"""Foreground voice orchestration with actual HTTP and audio fixture bytes."""
import base64,io,json,pathlib,socket,subprocess,tempfile,threading,wave
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[];requests=[]
def check(name,value):assert value,name;passed.append(name)
buf=io.BytesIO()
with wave.open(buf,'wb')as w:w.setnchannels(1);w.setsampwidth(2);w.setframerate(16000);w.writeframes(b'\0\0'*1600)
audio=buf.getvalue()
class Peer(BaseHTTPRequestHandler):
    def log_message(self,*args):pass
    def do_POST(self):
        data=self.rfile.read(int(self.headers.get('Content-Length','0')));requests.append((self.path,data));body=audio if self.path=='/speak'else json.dumps({'text':'Recognized fixture question.'}).encode();self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
server=ThreadingHTTPServer(('127.0.0.1',0),Peer);threading.Thread(target=server.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='voice-',dir=PROJECT/'output')as temp:
    p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def event(st,a,i=None):
        p.stdin.write(json.dumps(dict(op='event',state=st,action=a,inputs=i or {},root=temp))+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert'value'in r,r;return r['value']
    def transport(d):
        e=d['effect'];assert e,d['state'].get('notice');wire=base64.b64decode(e['wire_base64'])if'wire_base64'in e else e['wire'].encode()
        with socket.create_connection((e['host'],e['port']),timeout=5)as s:
            s.sendall(wire);out=b''
            while part:=s.recv(8192):out+=part
        return event(d['state'],'network_result',dict(purpose=e['purpose'],data=base64.b64encode(out).decode()))
    def reply(d,fmt,answer):
        value={'chat':{'choices':[{'message':{'content':answer},'finish_reason':'stop'}]},'local':{'message':{'content':answer},'done':True},'messages':{'content':[{'type':'text','text':answer}],'stop_reason':'end_turn'},'content':{'candidates':[{'content':{'parts':[{'text':answer}]},'finishReason':'STOP'}]}}[fmt];data=json.dumps(value).encode();wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(data)).encode()+b'\r\n\r\n'+data;return event(d['state'],'network_result',dict(purpose='model',model_ticket=d['state']['model_expected'],data=base64.b64encode(wire).decode()))
    def configured(fmt):
        cfg=dict(version=1,hosts=[dict(id='audio',url=f'http://127.0.0.1:{server.server_port}',plain=True,enabled=True)],routes=[dict(id='recognition',host='audio',capability='speech_recognition',path='/recognize',method='POST',request_format='multipart',file_field='audio',request={},response='json',response_path='/text',enabled=True),dict(id='synthesis',host='audio',capability='speech_synthesis',path='/speak',method='POST',request_format='json',request={'text':{'$input':'/text'}},response='binary',response_path='',artifact_kind='audio',enabled=True)],defaults={'speech_recognition':{'routes':['recognition']},'speech_synthesis':{'routes':['synthesis']}})
        d=event(None,'resume');d=event(d['state'],'service_connections');d=event(d['state'],'service_save',dict(service_config=json.dumps(cfg)));st=d['state'];st['profiles']=[dict(name='Controlled',endpoint='http://model.test/v1/chat/completions',protocol=fmt,model='controlled',plain=True,stream=False)];st['agents']=[dict(name='General',tools='read,service',investigation_preferences=False)];return st
    try:
        for fmt in ['chat','local','messages','content']:
            st=configured(fmt);n=len(requests);d=event(st,'voice_review');check(fmt+' consent review discloses destinations without capturing',d['effect']is None and 'Recognition'in json.dumps(d['ui']['modal'])and len(requests)==n);d=event(d['state'],'voice_consent');check(fmt+' enabling voice alone never opens microphone',d['effect']is None and d['state'].get('voice_consent'))
            d=event(d['state'],'voice_record');check(fmt+' recording begins only through explicit action',d['effect']['kind']=='media_record');check(fmt+' capture is visible in quota-managed media storage',pathlib.Path(d['effect']['capture_path']).parent==pathlib.Path(temp)/'service-files' and pathlib.Path(d['effect']['capture_path']).is_file() and 0<d['effect']['capture_limit']<=2*1024*1024);d=event(d['state'],'media_input_result',dict(media_ticket=d['effect']['media_ticket'],name='fixture.wav',data=base64.b64encode(audio).decode()));check(fmt+' consented capture starts actual recognition route',d['effect'] and d['effect']['purpose'].startswith('service_request:'))
            d=transport(d);check(fmt+' transcript becomes ordinary conversation turn',d['effect'] and d['effect']['purpose']=='model'and 'Recognized fixture question.'in d['effect']['wire']);check(fmt+' exact selected audio reached recognition host',audio in requests[-1][1]);d=reply(d,fmt,'Actual controlled reply.');check(fmt+' final answer starts configured synthesis',d['effect'] and d['effect']['purpose'].startswith('service_request:'))
            d=transport(d);check(fmt+' synthesis receives actual answer text',json.loads(requests[-1][1])=={'text':'Actual controlled reply.'});check(fmt+' actual returned audio reaches native playback effect',d['effect'] and d['effect']['kind']=='media_preview'and d['effect']['media_kind']=='audio');check(fmt+' text reply remains in history',any(m.get('content')=='Actual controlled reply.'for c in d['state']['chats'] for m in c.get('messages',[])))
            stopped=event(d['state'],'voice_stop');check(fmt+' stop revokes consent without removing text',not stopped['state'].get('voice_consent')and stopped['effect']['kind']=='media_playback_stop');restored=event(d['state'],'resume');check(fmt+' process recovery does not restore automatic voice',not restored['state'].get('voice_consent'))
        st=configured('chat');d=event(st,'voice_review');d=event(d['state'],'voice_consent');d=event(d['state'],'send',{'draft':'Manual text question.'});ticket=d['state']['model_expected'];stopped=event(d['state'],'voice_stop');check('stop during primary generation preserves primary owner',stopped['state']['busy']and stopped['state']['model_expected']==ticket and stopped['effect']['kind']=='media_playback_stop');done=reply(stopped,'chat','Text remains usable.');check('stopped voice still returns the primary text answer',done['effect']is None and not done['state']['busy'])
        st=configured('chat');d=event(st,'voice_review');d=event(d['state'],'voice_consent');d=event(d['state'],'voice_record',dict(draft='Existing unsent draft.'));d=event(d['state'],'media_input_result',dict(media_ticket=d['effect']['media_ticket'],name='fixture.wav',data=base64.b64encode(audio).decode()));done=transport(d);check('recognition preserves and appends to existing draft for review',done['state']['draft']=='Existing unsent draft.\nRecognized fixture question.' and done['effect']is None)
        d=event(done['state'],'voice_stop');d=event(d['state'],'send',dict(draft='Read this reply aloud.'));d=reply(d,'chat','Manual spoken reply.');d=event(d['state'],'voice_speak');check('manual speech first uses ordinary request review',d['state']['dialog']=='approval' and d['effect']is None);d=transport(event(d['state'],'approve'));check('manual speech plays actual returned audio',d['effect'] and d['effect']['kind']=='media_preview' and d['effect']['media_kind']=='audio')
    finally:p.stdin.close();p.wait(timeout=10);server.shutdown();server.server_close()
report=dict(passed=len(passed),checks=passed,scope='Owner HTTP/audio fixture orchestration through all four model formats. Capture callback is controlled; physical microphone and installed playback not exercised.');(PROJECT/'output/checks/voice.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
