"""PCM, file and HTTP execution through owned spoken-interruption callbacks."""
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
        d=event(configured(),'voice_open_review');modal=json.dumps(d['ui']['modal']);check('voice review explicitly offers separate interruption and background consent','voice_interruption'in modal and'echo control'in modal and'voice_background'in modal and'notification'in modal)
        d=event(configured(),'voice_open_review');d=event(d['state'],'voice_consent',dict(voice_interruption=True,voice_open=True,voice_silence_ms='600',voice_threshold='700',voice_max_seconds='10'));check('reviewed microphone controls save their actual input values',d['state']['voice_settings']==dict(interruption=True,silence_ms=600,threshold=700,max_seconds=10));event(d['state'],'voice_stop')
        d=event(configured(),'voice_open_review');d=event(d['state'],'voice_consent',dict(voice_silence_ms='299'));check('invalid reviewed microphone timing cannot start capture',d['effect']is None and d['state'].get('notice_error') and not d['state'].get('voice_consent'))
        d,play=playback(interruption=False);started=event(d['state'],'voice_playback_start',play);check('default-off retains half-duplex playback without a watch',started['effect']is None and not started['state'].get('voice_watch_expected'));event(started['state'],'voice_stop')
        for fmt in ['chat','local','messages','content']:
            d,play=playback(fmt);check(fmt+' reviewed setting reaches owned playback',d['effect']['voice_interruption']is True)
            d=event(d['state'],'voice_playback_start',play);e=d['effect'];check(fmt+' actual playback start opens only an owned watch',e['kind']=='voice_watch'and e['voice_playback_ticket']==play['voice_playback_ticket']and d['state']['voice_status']=='speaking')
            owner=dict(voice_session=e['voice_session'],voice_audio_ticket=e['voice_audio_ticket'],voice_playback_ticket=e['voice_playback_ticket']);wrong=event(d['state'],'voice_watch_start',dict(owner,echo_active=False,draft='injected'));check(fmt+' inactive echo cannot announce a live microphone',wrong['state']==d['state'])
            d=event(d['state'],'voice_watch_start',dict(owner,echo_active=True));check(fmt+' active echo handshake exposes microphone state','microphone on'in json.dumps(d['ui'])and d['state']['voice_interruption_status']=='listening');again=event(d['state'],'voice_watch_start',dict(owner,echo_active=True));check(fmt+' duplicate watch start is an exact no-op',again['state']==d['state'])
            for frame in [quiet]*30+[tone]*5+[quiet]*5:assert'onset_receipt'not in feed(owner,frame)
            check(fmt+' silence and short noise do not interrupt playback',d['state']['voice_status']=='speaking'and not d['state'].get('service_expected'))
            notice=onset(d,owner);bad=event(d['state'],'voice_watch_onset',dict(notice,onset_receipt='wrong',draft='injected'));check(fmt+' unowned onset receipt is an exact no-op',bad['state']==d['state'])
            previous=d['state']['media_input'];d=event(d['state'],'voice_watch_onset',notice);check(fmt+' onset promotes the same capture and stops only playback',d['effect']['kind']=='voice_interrupt_listen'and d['state']['voice_status']=='listening'and d['state']['voice_audio_expected']['ticket']==owner['voice_audio_ticket']and not d['state'].get('voice_watch_expected')and d['state']['media_input']==previous)
            for action,values in [('voice_watch_onset',notice),('voice_watch_unavailable',owner),('voice_playback_result',dict(play,completed=True))]:
                stale=event(d['state'],action,values);check(fmt+' retired callback cannot alter promoted capture '+action,stale['state']==d['state'])
            for frame in [tone]*10+[quiet]*40:
                v=feed(owner,frame)
                if v['ready']:break
            assert v['ready'];d=event(d['state'],'voice_audio_result',dict(owner,receipt=v['receipt']));check(fmt+' promoted utterance starts ordinary recognition',d['effect']and d['state']['voice_status']=='recognizing')
            d=transport(d);payload=requests[-1][1];offset=payload.index(b'RIFF');size=struct.unpack_from('<I',payload,offset+4)[0]+8
            with wave.open(io.BytesIO(payload[offset:offset+size]),'rb')as w:pcm=w.readframes(w.getnframes());check(fmt+' leading speech PCM survives promotion into actual HTTP audio',marker+tone*11 in pcm and w.getframerate()==16000)
            check(fmt+' interrupted speech reaches captured primary model',d['effect']['purpose']=='model'and'Interrupted question.'in d['effect']['wire']);pending=d
            stale=event(d['state'],'voice_watch_onset',dict(notice,draft='injected'));check(fmt+' stale watch cannot alter the primary request',stale['state']==pending['state'])
            d=reply(d,fmt,'Reply after interruption.');d=transport(d);check(fmt+' next reply keeps configured output and a new playback owner',d['effect']['kind']=='media_preview'and d['effect']['voice_playback_ticket']!=play['voice_playback_ticket']and json.loads(requests[-1][1])=={'text':'Reply after interruption.'});event(d['state'],'voice_stop')
        d,owner,play=watched();notice=onset(d,owner)
        for frame in [tone]*10+[quiet]*40:
            v=feed(owner,frame)
            if v['ready']:break
        blocked=event(d['state'],'voice_audio_result',dict(owner,receipt=v['receipt']));check('complete watch audio cannot submit before onset promotion',blocked['state']==d['state'] and blocked['effect']is None)
        d=event(d['state'],'voice_watch_onset',notice);d=event(d['state'],'voice_audio_result',dict(owner,receipt=v['receipt']));check('queued onset promotes already completed PCM without losing its receipt',d['state']['voice_status']=='recognizing' and d['effect']);event(d['state'],'voice_stop')
        d,owner,play=watched();before=d['state']['voice_playback_expected'];n=len(requests);done=event(d['state'],'voice_watch_unavailable',owner);check('echo failure preserves current reply and consent',done['effect']is None and done['state']['voice_status']=='speaking'and done['state']['voice_playback_expected']==before and done['state']['voice_consent']==owner['voice_session']and len(requests)==n);check('echo failure retires only its microphone watch','value'not in raw(dict(owner,op='voice_audio_feed',data=base64.b64encode(tone).decode())));manual=event(done['state'],'voice_interrupt',play);check('manual interruption remains usable after echo failure',manual['effect']['kind']=='voice_listen');event(manual['state'],'voice_stop')
        d,owner,play=watched();notice=onset(d,owner);edited=copy.deepcopy(d['state']);edited['draft']='Keep these typed words.';done=event(edited,'voice_watch_onset',notice);check('onset preserves an unsent draft and makes no recognition request',done['state']['draft']=='Keep these typed words.'and done['state']['voice_status']=='paused'and done['effect']['kind']=='media_playback_stop'and not done['state'].get('service_expected'));event(done['state'],'voice_stop')
        d,owner,play=watched();edited=event(d['state'],'capture',dict(draft='Editing during the reply.'));check('draft editing stops watch without stopping returned speech',edited['state']['voice_status']=='speaking'and not edited['state'].get('voice_watch_expected')and edited['state']['voice_playback_expected']==d['state']['voice_playback_expected']);event(edited['state'],'voice_stop')
        for action in ['voice_suspend','voice_stop','service_connections']:
            d,owner,play=watched();notice=onset(d,owner);stopped=event(d['state'],action);late=event(stopped['state'],'voice_watch_onset',dict(notice,draft='injected'));check(action+' revokes the watch and ignores late onset',not stopped['state'].get('voice_consent')and late['state']==stopped['state']);check(action+' retires microphone PCM ownership','value'not in raw(dict(owner,op='voice_audio_feed',data=base64.b64encode(tone).decode())))
        d,owner,play=watched();notice=onset(d,owner);sent=event(d['state'],'send',dict(draft='Typed primary request.'));late=event(sent['state'],'voice_watch_onset',notice);check('new typed primary owner survives a late spoken interruption',sent['effect']['purpose']=='model'and late['state']==sent['state']);reply(sent,'chat','Typed result retained.')
        d,owner,play=watched();done=event(d['state'],'voice_playback_result',dict(play,completed=True));check('normal playback completion replaces watch with a normal listener',done['effect']['kind']=='voice_listen'and done['effect']['voice_audio_ticket']!=owner['voice_audio_ticket']and not done['state'].get('voice_watch_expected'));event(done['state'],'voice_stop')
        for phase in ['listening','recognizing','thinking','synthesizing','speaking']:
            d=event(configured(),'voice_open_review');d=event(d['state'],'voice_consent',dict(voice_interruption=True));session=d['state']['voice_consent']
            if phase!='listening':d=spoken(d)
            if phase in ['thinking','synthesizing','speaking']:d=transport(d)
            if phase in ['synthesizing','speaking']:d=reply(d,'chat','Retained route test reply.')
            if phase=='speaking':
                d=transport(d);play=dict(voice_session=session,voice_playback_ticket=d['effect']['voice_playback_ticket']);d=event(d['state'],'voice_playback_start',play)
            delayed=transport(d,True)if phase in ['recognizing','synthesizing']else None
            before=len(requests);wrong=event(d['state'],'voice_route_lost',dict(voice_session='retired-session',draft='injected'));check(phase+' wrong route owner cannot alter the conversation',wrong['state']==d['state'])
            stopped=event(d['state'],'voice_route_lost',dict(voice_session=session));check(phase+' route loss retires consent and capture without replay',not stopped['state'].get('voice_consent')and not stopped['state'].get('voice_open')and not stopped['state'].get('voice_audio_expected')and not stopped['state'].get('voice_watch_expected')and not stopped['state'].get('voice_playback_expected')and len(requests)==before)
            again=event(stopped['state'],'voice_route_lost',dict(voice_session=session,draft='injected'));check(phase+' late route callback is an exact no-op',again['state']==stopped['state'])
            if phase=='thinking':
                done=reply(stopped,'chat','Primary answer retained after route loss.');check('route loss preserves an in-flight primary answer without speaking or rearming',done['effect']is None and not done['state'].get('voice_open')and'Primary answer retained after route loss.'in json.dumps(done['state'])and len(requests)==before)
            elif phase in ['recognizing','synthesizing']:
                late=event(stopped['state'],'network_result',delayed);check(phase+' late actual HTTP result stays retired',late['state']==stopped['state']and late['effect']is None and len(requests)==before)
            else:
                fresh=event(stopped['state'],'voice_open_review');fresh=event(fresh['state'],'voice_consent');old=event(fresh['state'],'voice_route_lost',dict(voice_session=session));check(phase+' previous route owner cannot stop a newly reviewed microphone',old['state']==fresh['state']and fresh['effect']['kind']=='voice_listen');event(fresh['state'],'voice_stop')
        (pathlib.Path(temp)/'local.txt').write_text('local operation retained');check('local work remains usable without voice or service availability',op(dict(op='tool',tool='read',root=temp,args={'path':'local.txt'}))=='local operation retained')
    finally:p.stdin.close();p.wait(timeout=10)
server.shutdown();server.server_close()
report=dict(passed=len(passed),checks=passed,scope='Owner execution with actual Rust PCM segmentation, files, HTTP service fixtures and controlled model replies/echo-start callbacks. Physical echo suppression, Android microphone/audio routing/lifecycle and live user providers remain unverified.');(PROJECT/'output/checks/voice_interruption.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
