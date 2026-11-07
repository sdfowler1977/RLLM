"""Owned optional background intent through actual PCM and speech HTTP peers."""
import base64,copy,io,json,pathlib,socket,struct,subprocess,tempfile,threading,wave
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from transport import upload,response
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[];requests=[]
def check(name,value):assert value,name;passed.append(name)
buf=io.BytesIO()
with wave.open(buf,'wb')as w:w.setnchannels(1);w.setsampwidth(2);w.setframerate(16000);w.writeframes(b'\0\0'*1600)
audio=buf.getvalue();tone=b''.join(struct.pack('<h',3000 if n%2==0 else -3000)for n in range(320));marker=b''.join(struct.pack('<h',2600 if n%2==0 else -2600)for n in range(320));quiet=b'\0'*640
class Peer(BaseHTTPRequestHandler):
    def log_message(self,*args):pass
    def do_POST(self):
        payload=self.rfile.read(int(self.headers.get('Content-Length','0')));requests.append((self.path,payload));body=audio if self.path=='/speak'else json.dumps({'text':'Interrupted question.'}).encode();self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
server=ThreadingHTTPServer(('127.0.0.1',0),Peer);threading.Thread(target=server.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='interruption-',dir=PROJECT/'output')as temp:
    p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def raw(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
    def op(v):r=raw(v);assert'value'in r,r;return r['value']
    def event(st,a,i=None):return op(dict(op='event',state=st,action=a,inputs=i or {},root=temp))
    def transport(d,delay_callback=False):
        e=d['effect'];assert e,d['state'].get('notice');wire=base64.b64decode(e['wire_base64'])if'wire_base64'in e else e['wire'].encode()
        with socket.create_connection((e['host'],e['port']),timeout=5)as sock:sock.sendall(wire);upload(op,e,sock);fields=response(op,e,sock)
        values=dict(fields,purpose=e['purpose']);return values if delay_callback else event(d['state'],'network_result',values)
    def reply(d,fmt,answer):
        value={'chat':{'choices':[{'message':{'content':answer},'finish_reason':'stop'}]},'local':{'message':{'content':answer},'done':True},'messages':{'content':[{'type':'text','text':answer}],'stop_reason':'end_turn'},'content':{'candidates':[{'content':{'parts':[{'text':answer}]},'finishReason':'STOP'}]}}[fmt];b=json.dumps(value).encode();wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(b)).encode()+b'\r\n\r\n'+b;return event(d['state'],'network_result',dict(purpose='model',model_ticket=d['state']['model_expected'],data=base64.b64encode(wire).decode()))
    def configured(fmt='chat'):
        cfg=dict(version=1,hosts=[dict(id='audio',url=f'http://127.0.0.1:{server.server_port}',plain=True,enabled=True)],routes=[dict(id='recognition',host='audio',capability='speech_recognition',path='/recognize',method='POST',request_format='multipart',file_field='audio',request={},response='json',response_path='/text',enabled=True),dict(id='synthesis',host='audio',capability='speech_synthesis',path='/speak',method='POST',response='binary',artifact_kind='audio',request={'text':{'$input':'/text'}},enabled=True)],defaults={'speech_recognition':{'routes':['recognition']},'speech_synthesis':{'routes':['synthesis']}})
        d=event(None,'resume');d=event(d['state'],'service_connections');d=event(d['state'],'service_save',dict(service_config=json.dumps(cfg)));st=d['state'];st['profiles']=[dict(name='Controlled',endpoint='http://model.test/v1/chat/completions',protocol=fmt,model='controlled',plain=True,stream=False)];st['agents']=[dict(name='General',tools='read,service',investigation_preferences=False)];return st
    def feed(owner,frame):return op(dict(owner,op='voice_audio_feed',data=base64.b64encode(frame).decode()))
    def spoken(d):
        e=d['effect'];owner=dict(voice_session=e['voice_session'],voice_audio_ticket=e['voice_audio_ticket']);d=event(d['state'],'voice_audio_start',owner)
        for frame in [quiet]*20+[tone]*20+[quiet]*40:
            v=feed(owner,frame)
            if v['ready']:return event(d['state'],'voice_audio_result',dict(owner,receipt=v['receipt']))
        raise AssertionError('utterance did not complete')
    def playback(fmt='chat',interruption=True):
        d=event(configured(fmt),'voice_open_review');d=event(d['state'],'voice_consent',dict(voice_interruption=interruption));d=spoken(d);d=transport(d);d=reply(d,fmt,'Reply before interruption.');d=transport(d);e=d['effect'];owner=dict(voice_session=e['voice_session'],voice_playback_ticket=e['voice_playback_ticket']);return d,owner
    def watched(fmt='chat'):
        d,play=playback(fmt);d=event(d['state'],'voice_playback_start',play);assert d['effect']and d['effect']['kind']=='voice_watch',d['state'].get('notice');e=d['effect'];owner=dict(voice_session=e['voice_session'],voice_audio_ticket=e['voice_audio_ticket'],voice_playback_ticket=e['voice_playback_ticket']);d=event(d['state'],'voice_watch_start',dict(owner,echo_active=True));assert d['state']['voice_interruption_status']=='listening';return d,owner,play
    def onset(d,owner):
        for frame in [quiet]*20+[marker]+[tone]*11:
            v=feed(owner,frame)
            if 'onset_receipt'in v:return dict(owner,onset_receipt=v['onset_receipt'])
        raise AssertionError('speech onset did not complete')
    try:
        for fmt in ['chat','local','messages','content']:
            d=event(configured(fmt),'voice_open_review');modal=d['ui']['modal']
            control=next(f for f in modal['fields']if f['key']=='voice_background')
            check(fmt+' background consent defaults off',control['value']=='false')
            d=event(d['state'],'voice_consent',dict(voice_background=True));e=d['effect'];session=e['voice_session'];lease=e['voice_lease_ms']
            check(fmt+' explicit background intent reaches capture with a bounded remaining lease',e['kind']=='voice_listen'and e['voice_background']and 0<lease<=600000)
            wrong=event(d['state'],'voice_session_stop',dict(voice_session=session+'-stale'))
            check(fmt+' unrelated service stop is an exact no-op',wrong['state']==d['state']and wrong['effect']is None)
            d=spoken(d);d=transport(d)
            check(fmt+' actual recognition still reaches the configured primary model',d['effect']['purpose']=='model'and d['state']['voice_open'])
            d=reply(d,fmt,'Background voice response.');d=transport(d);e=d['effect']
            check(fmt+' actual returned speech retains its owned session',e['kind']=='media_preview'and e['voice_session']==session and e['media_kind']=='audio')
            owner=dict(voice_session=session,voice_playback_ticket=e['voice_playback_ticket']);d=event(d['state'],'voice_playback_start',owner)
            check(fmt+' notification status exposes background intent and actual phase',d['ui']['voice_status']['background']and d['ui']['voice_status']['phase']=='speaking')
            d=event(d['state'],'voice_playback_result',dict(owner,completed=True))
            check(fmt+' next real capture keeps the same session without extending consent',d['effect']['voice_background']and d['effect']['voice_session']==session and 0<d['effect']['voice_lease_ms']<=lease)
            capture=copy.deepcopy(d['effect']);d=event(d['state'],'voice_session_stop',dict(voice_session=session))
            check(fmt+' owned service stop retires the session and actual PCM owner',not d['state']['voice_open']and not d['state'].get('voice_consent')and d['effect']['kind']=='media_playback_stop'and d['effect']['voice_only'])
            stale=event(d['state'],'voice_audio_start',dict(voice_session=session,voice_audio_ticket=capture['voice_audio_ticket']))
            check(fmt+' stopped session cannot revive capture',stale['state']==d['state']and stale['effect']is None)
            d=event(configured(fmt),'voice_open_review');d=event(d['state'],'voice_consent',dict(voice_background=True));d=spoken(d);d=transport(d);model=copy.deepcopy(d)
            stopped=event(d['state'],'voice_session_stop',dict(voice_session=d['state']['voice_consent']))
            check(fmt+' stopping service preserves an in-flight primary answer',stopped['state']['busy']and stopped['state']['model_expected']==model['state']['model_expected'])
            done=reply(stopped,fmt,'Complete primary text survives voice stop.')
            check(fmt+' primary result remains text without restarting speech',done['effect']is None and not done['state']['busy']and any('Complete primary text survives voice stop.'in m.get('content','')for c in done['state']['chats']for m in c['messages']))
            d=event(configured(fmt),'voice_open_review');d=event(d['state'],'voice_consent')
            check(fmt+' ordinary foreground voice still needs no background service',d['effect']['kind']=='voice_listen'and not d['effect']['voice_background']);event(d['state'],'voice_stop')
        d=event(configured(),'voice_review');refused=event(d['state'],'voice_consent',dict(voice_background=True,voice_open=False))
        check('background cannot silently enable a microphone for manual-only voice',refused['effect']is None and refused['state'].get('notice_error')and not refused['state'].get('voice_consent'))
        d=event(configured(),'voice_open_review');d=event(d['state'],'voice_consent',dict(voice_background=True));old=d['state']['voice_consent'];d=event(d['state'],'voice_stop');d=event(d['state'],'voice_open_review');d=event(d['state'],'voice_consent',dict(voice_background=True));late=event(d['state'],'voice_session_stop',dict(voice_session=old))
        check('old notification cannot stop a newly reviewed session',late['state']==d['state']and late['effect']is None);event(d['state'],'voice_stop')
        d=event(configured(),'voice_open_review');d=event(d['state'],'voice_consent',dict(voice_background=True));state=copy.deepcopy(d['state']);state['draft']='Retained core draft from a paused turn.';saved=event(state,'status');check('background callback without stale form input preserves the actual unsent draft',saved['state']['draft']==state['draft']);resumed=event(saved['state'],'phone_access_status');check('activity return before form refresh keeps the retained draft',resumed['state']['draft']==state['draft']);event(resumed['state'],'voice_stop')
    finally:p.stdin.close();p.wait(timeout=10);server.shutdown();server.server_close()
record=dict(passed=len(passed),checks=passed,scope='Owner execution through actual Rust PCM, local files and speech HTTP peers across all four model formats. Platform service notification/permission/lifecycle still requires separate adapter and physical Android acceptance.')
out=PROJECT/'output/checks';out.mkdir(parents=True,exist_ok=True);(out/'voice_background.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record,indent=2))
