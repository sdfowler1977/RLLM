from transport import upload,response
"""Actual file bytes and local HTTP transfers through public shared-core events."""
import base64,copy,hashlib,io,json,pathlib,socket,struct,subprocess,tempfile,threading,wave,zlib
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[];received=[]
def check(name,value):assert value,name;passed.append(name)
buffer=io.BytesIO()
with wave.open(buffer,'wb')as out:out.setnchannels(1);out.setsampwidth(2);out.setframerate(16000);out.writeframes(b'\0\0'*1600)
audio=buffer.getvalue()
class Peer(BaseHTTPRequestHandler):
    def log_message(self,*a):pass
    def do_GET(self):
        payload=self.rfile.read(int(self.headers.get('Content-Length','0')));received.append((self.path,dict(self.headers),payload))
        body=json.dumps({'text':'fixture transcription'}).encode();self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    def do_POST(self):
        payload=self.rfile.read(int(self.headers.get('Content-Length','0')));received.append((self.path,dict(self.headers),payload))
        body=audio if self.path=='/audio' else json.dumps({'text':'fixture transcription','length':len(payload)}).encode()
        self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    do_PUT=do_POST
server=ThreadingHTTPServer(('127.0.0.1',0),Peer);threading.Thread(target=server.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='media-',dir=PROJECT/'output')as temp:
    root=pathlib.Path(temp);p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def op(v):
        p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert 'value'in r,r;return r['value']
    def event(st,a,i=None):return op(dict(op='event',state=st,action=a,inputs=i or {},root=str(root)))
    def pick(st,content,name):
        d=event(st,'media_pick');ticket=d['effect']['media_ticket'];r=event(d['state'],'media_input_result',dict(media_ticket=ticket,name=name,data=base64.b64encode(content).decode()));assert r['state'].get('media_input'),r['state'].get('notice');return r
    def configure(format='multipart',path='/transcribe',response='json',pointer='/text',request=None,kind='data'):
        cfg=dict(version=1,hosts=[dict(id='local',url=f'http://127.0.0.1:{server.server_port}',plain=True,enabled=True)],routes=[dict(id='media',host='local',capability='speech_recognition',path=path,method='POST',request_format=format,request=request or {'language':{'$input':'/language'}},file_field='audio',response=response,response_path=pointer,artifact_kind=kind,enabled=True)],defaults={})
        d=event(None,'resume');d=event(d['state'],'service_connections');return event(d['state'],'service_save',dict(service_config=json.dumps(cfg)))['state']
    def review(st,file=True):
        d=event(st,'service_request');d=event(d['state'],'service_review',dict(service_route='media',service_file=file,service_input=json.dumps({'language':'en'})));assert d['state'].get('pending'),d['state'].get('notice');return d
    def complete(d):
        e=d['effect'];assert e,d['state'].get('notice');wire=base64.b64decode(e['wire_base64'])if'wire_base64'in e else e['wire'].encode()
        with socket.create_connection((e['host'],e['port']),timeout=5)as s:
            s.sendall(wire);upload(op,e,s);reply_fields=response(op,e,s)
        return event(d['state'],'network_result',dict(reply_fields,purpose=e['purpose']))
    try:
        d=pick(configure(),audio,'recording.wav');record=d['state']['media_input'];file=root/record['path'];check('selected audio stored as actual owned file',file.read_bytes()==audio and record['sha256']==hashlib.sha256(audio).hexdigest());check('input bytes excluded from saved conversation JSON',base64.b64encode(audio).decode()not in json.dumps(d['state']))
        pending=review(d['state']);check('file disclosure is explicit before dispatch',pending['state']['pending']['file_disclosure']['sha256']==record['sha256']);done=complete(event(pending['state'],'approve'));headers,body=received[-1][1:];check('multipart transfer contains exact selected bytes',audio in body and 'multipart/form-data; boundary='in headers['Content-Type']);check('multipart fields mapped from typed input',b'name="language"\r\n\r\nen'in body and b'name="audio"'in body);check('actual mapped transcription returned',done['state']['service_result']['value']=='fixture transcription')
        d=pick(configure('binary'),audio,'recording.wav');done=complete(event(review(d['state'])['state'],'approve'));check('raw transfer preserves all bytes',received[-1][2]==audio)
        st=pick(configure('binary'),audio,'get-source.wav')['state'];st['service_connections']['routes'][0]['method']='GET';path=root/st['media_input']['path'];before=len(received)
        d=event(st,'service_request');refused=event(d['state'],'service_review',dict(service_route='media',service_file=True,service_input='{"language":"en"}'))
        check('GET file request refuses before approval or transport',refused['effect']is None and refused['state'].get('notice_error') and not refused['state'].get('pending'))
        check('GET refusal retains selected file and local bytes',refused['state']['media_input']==st['media_input'] and path.read_bytes()==audio and len(received)==before)
        (root/'local.txt').write_text('local operation retained');local=op(dict(op='tool',tool='read',root=str(root),args={'path':'local.txt'}));check('GET refusal preserves unrelated local operation',local=='local operation retained')
        st['service_connections']['routes'][0].update(request_format='json',path='/status/{language}');done=complete(event(review(st,file=False)['state'],'approve'));check('GET without file retains path variables and sends no body',received[-1][0]=='/status/en' and received[-1][2]==b'' and done['state']['service_result']['value']=='fixture transcription')
        st['service_connections']['routes'][0].update(method='PUT',request_format='binary',path='/transcribe');done=complete(event(review(st)['state'],'approve'));check('PUT retains actual reviewed file upload',received[-1][2]==audio)
        d=pick(configure('json',request={'audio':{'$file':'base64'},'name':{'$file':'name'}}),audio,'selected.wav');done=complete(event(review(d['state'])['state'],'approve'));data=json.loads(received[-1][2]);check('explicit JSON file mapping preserves all bytes',base64.b64decode(data['audio'])==audio and data['name']=='selected.wav')
        d=pick(configure(),audio,'original.wav');pending=review(d['state']);(root/d['state']['media_input']['path']).write_bytes(b'changed');refused=event(pending['state'],'approve');check('changed file is withheld before upload',refused['effect']is None and refused['state'].get('notice_error'))
        d=event(configure(),'media_pick');cancelled=event(d['state'],'cancel');late=event(cancelled['state'],'media_input_result',dict(media_ticket=d['effect']['media_ticket'],data=base64.b64encode(audio).decode(),name='late.wav',draft='late injected draft'));check('cancelled native input callback is exact state no-op',late['state']==cancelled['state'])
        d=event(configure(),'media_pick');wrong=event(d['state'],'media_input_result',dict(media_ticket='wrong',draft='injected'));check('wrong native capture owner is exact state no-op',wrong['state']==d['state']);event(d['state'],'cancel')
        d=pick(configure('json','/audio','binary','',{'text':'fixture audio'},'audio'),audio,'source.wav');done=complete(event(review(d['state'],file=False)['state'],'approve'));artifact=done['state'].get('service_artifact');check('binary result stored outside encrypted JSON',artifact and (root/artifact['path']).read_bytes()==audio and base64.b64encode(audio).decode()not in json.dumps(done['state']));check('returned artifact SHA matches actual bytes',artifact['sha256']==hashlib.sha256(audio).hexdigest());check('returned file appears in current conversation',any(r['path']==artifact['path']for r in done['ui']['service_artifacts']));export_state=copy.deepcopy(done['state']);export_state.setdefault('drive',{})['phone_default']=True;export_state['phone_uri']='content://fixture/tree/owned';exported=event(export_state,'media_export');check('media export keeps actual detected extension and MIME',exported['effect']['prepare']['name'].endswith('.wav') and exported['effect']['prepare']['mime']=='audio/wav');record=op(dict(op='project_chunks',action='record',prepare=exported['effect']['prepare'],bytes=str(artifact['bytes'])));check('media export metadata passes existing chunk transfer contract',record.get('mime')=='audio/wav');preview=event(done['state'],'media_preview');check('validated audio hands off to native decoder',preview['effect']['kind']=='media_preview' and preview['effect']['media_kind']=='audio');(root/artifact['path']).write_bytes(b'changed');blocked=event(done['state'],'media_preview');check('changed returned file cannot reach decoder',blocked['effect']is None and blocked['state'].get('notice_error'))
        for kind in ['audio','video']:
            fixture=(PROJECT/'verification/fixtures'/f'track-{kind}.mp4').read_bytes();picked=pick(configure(),fixture,'generic.mp4');record=picked['state']['media_input'];check(f'generic MP4 {kind} classification uses actual track handler',record['kind']==kind and (root/record['path']).read_bytes()==fixture)
        fake=b'\0\0\0\x18ftypisom\0\0\0\0isommp42'+b'\0\0\0\x40mdatvide';picked=pick(configure(),fake,'fake.mp4');check('malformed MP4 and embedded handler text cannot assert video',picked['state']['media_input']['kind']=='data')
        d=pick(configure(),audio,'audio.wav');pending=review(d['state'],file=False);refused=event(pending['state'],'approve');check('multipart cannot silently upload without file consent',refused['effect']is None and refused['state'].get('notice_error'))
        d=pick(configure(),audio,'keep.wav');record=d['state']['media_input'];path=root/record['path'];d=event(d['state'],'media_files');check('media manager lists actual retained file',record['path'].split('/')[-1] in json.dumps(d['ui']['modal']))
        d=event(d['state'],'media_file:'+path.name);refused=event(d['state'],'media_file_remove_confirm');check('file cannot be removed without its review',path.exists() and refused['state'].get('notice_error'))
        review_delete=event(d['state'],'media_file_remove');kept=event(review_delete['state'],'media_files');check('leaving removal review retains file',path.exists())
        changed=event(review_delete['state'],'media_files');changed=event(changed['state'],'media_file:'+path.name);changed=event(changed['state'],'media_file_remove');path.write_bytes(audio+b'changed');blocked=event(changed['state'],'media_file_remove_confirm');check('changed file is withheld from old removal approval',path.exists() and blocked['state'].get('notice_error'))
        d=event(blocked['state'],'media_file:'+path.name);d=event(d['state'],'media_file_remove');done=event(d['state'],'media_file_remove_confirm');check('specific reviewed file removal clears selected input only',not path.exists() and done['state']['media_input'] is None and len(list((root/'service-files').iterdir()))>0)
        png=b'\x89PNG\r\n\x1a\n'
        for kind,body in [(b'IHDR',struct.pack('>IIBBBBB',1,1,8,0,0,0,0)),(b'IDAT',zlib.compress(b'\0\0')),(b'IEND',b'')]:png+=struct.pack('>I',len(body))+kind+body+struct.pack('>I',zlib.crc32(kind+body))
        d=pick(configure(),png,'image.png');d=event(d['state'],'media_attach_input');current=next(c for c in d['state']['chats']if c['id']==d['state']['active']);check('selected image attaches exact bytes to draft without sending',current['draft_attachments'][0]['data']==base64.b64encode(png).decode() and not current.get('messages') and d['effect']is None)
    finally:p.stdin.close();p.wait(timeout=10);server.shutdown();server.server_close()
report=dict(passed=len(passed),checks=passed,scope='Owner file/HTTP fixture checks. No physical microphone, installed Android decoding, live provider or independent acceptance claimed.');(PROJECT/'output/checks/media.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
