"""Real chunked picker import and HTTP upload, with scoped ownership failures."""
import base64,copy,hashlib,json,pathlib,socket,subprocess,tempfile,threading
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from email.parser import BytesParser
from email.policy import default
from transport import upload
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[];received=[]
def check(name,value):assert value,name;passed.append(name)
class Peer(BaseHTTPRequestHandler):
    def log_message(self,*a):pass
    def do_POST(self):
        payload=self.rfile.read(int(self.headers['Content-Length']));received.append((dict(self.headers),payload));body=b'{"received":true}'
        self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    do_PUT=do_POST
server=ThreadingHTTPServer(('127.0.0.1',0),Peer);threading.Thread(target=server.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='upload-',dir=PROJECT/'output')as temp:
    root=pathlib.Path(temp);p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def raw(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
    def op(v):r=raw(v);assert'value'in r,r;return r['value']
    def event(st,a,i=None,workspace=None):return op(dict(op='event',state=st,action=a,inputs=i or {},root=str(workspace or root)))
    def config(st=None,fmt='binary',request=None):
        cfg=dict(version=1,hosts=[dict(id='server',url=f'http://127.0.0.1:{server.server_port}',plain=True,enabled=True)],routes=[dict(id='upload',host='server',capability='document_processing',method='POST',path='/upload',request_format=fmt,request={}if request is None else request,file_field='source',response='json',response_path='',enabled=True)],defaults={})
        d=event(st,'resume')if st is None else dict(state=st);d=event(d['state'],'service_connections');saved=event(d['state'],'service_save',dict(service_config=json.dumps(cfg)));assert not saved['state'].get('notice_error'),saved['state'].get('notice');return saved['state']
    def opening(st,name='source.pdf',workspace=None):
        d=event(st,'media_pick',workspace=workspace);owner=d['effect']['media_ticket'];stream=op(dict(op='media_input_open',media_ticket=owner,name=name));return d,dict(media_ticket=owner,ticket=stream['ticket']),stream
    def selected(st,data,name='source.pdf'):
        d,o,_=opening(st,name)
        for i in range(0,len(data),8192):op(dict(o,op='media_input_feed',data=base64.b64encode(data[i:i+8192]).decode()))
        receipt=op(dict(o,op='media_input_finish'));done=event(d['state'],'media_input_result',dict(media_ticket=o['media_ticket'],media_receipt=receipt));assert done['state'].get('media_input'),done['state'].get('notice');return done,o,receipt
    def approved(st,input=None):
        d=event(st,'service_request');d=event(d['state'],'service_review',dict(service_route='upload',service_file=True,service_input=json.dumps(input or {})));assert d['state'].get('dialog')=='approval',d['state'].get('notice');d=event(d['state'],'approve');assert d['effect'],d['state'].get('notice');return d
    def exchange(d):
        e=d['effect'];check('file request envelope stays bounded '+str(len(passed)),len(json.dumps(e))<65536 and'media_upload'in e and'wire_base64'not in e)
        with socket.create_connection((e['host'],e['port']),timeout=20)as sock:
            sock.sendall(e['wire'].encode());upload(op,e,sock);response=b''
            while chunk:=sock.recv(8192):response+=chunk
        return event(d['state'],'network_result',dict(purpose=e['purpose'],data=base64.b64encode(response).decode()))
    try:
        # A syntactically complete PDF with a large, unreferenced stream object.
        objects=[b'<< /Type /Catalog /Pages 2 0 R >>',b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>']
        stream=b'% padding\n'*320000;objects.append(b'<< /Length '+str(len(stream)).encode()+b' >>\nstream\n'+stream+b'endstream');data=b'%PDF-1.7\n';offsets=[0]
        for i,body in enumerate(objects,1):offsets.append(len(data));data+=str(i).encode()+b' 0 obj\n'+body+b'\nendobj\n'
        xref=len(data);data+=b'xref\n0 5\n0000000000 65535 f \n'+b''.join(f'{n:010d} 00000 n \n'.encode()for n in offsets[1:])+b'trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n'+str(xref).encode()+b'\n%%EOF\n'
        d,o,receipt=selected(config(),data);record=d['state']['media_input'];check('PDF larger than inline limit imports exact bytes',len(data)>2*1024*1024 and(root/record['path']).read_bytes()==data and record['sha256']==hashlib.sha256(data).hexdigest()and record['name']=='source.pdf'and record['kind']=='document');check('large file bytes stay outside saved state',len(json.dumps(d['state']))<100000 and base64.b64encode(data[:100000]).decode()not in json.dumps(d['state']))
        repeated=event(d['state'],'media_input_result',dict(media_ticket=o['media_ticket'],media_receipt=receipt,draft='injected'));check('input completion is single-use exact no-op',repeated['state']==d['state'])
        done=exchange(approved(d['state']));check('binary HTTP upload retains complete large PDF',received[-1][1]==data and received[-1][0]['Content-Type']=='application/pdf'and done['state']['service_result']['value']=={'received':True})
        st=config(d['state'],'json',{'source':{'$file':'base64'},'url':{'$file':'data_url'},'name':{'$file':'name'},'mime':{'$file':'mime'},'opaque':{'$input':'/opaque'}});done=exchange(approved(st,{'opaque':{'$file':'base64'}}));value=json.loads(received[-1][1]);check('JSON streams repeated base64 and data URL without padding gaps',base64.b64decode(value['source'])==data and base64.b64decode(value['url'].split(',',1)[1])==data and value['name']=='source.pdf'and value['mime']=='application/pdf');check('input data cannot acquire template file authority',value['opaque']=={'$file':'base64'})
        st=config(d['state'],'multipart',{'language':{'$input':'/language'},'encoded':{'$file':'base64'},'detail':{'name':{'$file':'name'}}});done=exchange(approved(st,{'language':'en'}));headers,body=received[-1];message=BytesParser(policy=default).parsebytes(('Content-Type: '+headers['Content-Type']+'\r\nMIME-Version: 1.0\r\n\r\n').encode()+body);fields={part.get_param('name',header='content-disposition'):part.get_payload(decode=True)for part in message.iter_parts()};check('multipart streams actual file and mapped fields',fields['source']==data and fields['language']==b'en'and base64.b64decode(fields['encoded'])==data and json.loads(fields['detail'])=={'name':'source.pdf'})
        for length in [1,2,3,8191,8192,8193]:
            content=bytes(i%251 for i in range(length));small,_,_=selected(config(None,'json',{'source':{'$file':'base64'}}),content,'bytes.bin');exchange(approved(small['state']));check('base64 chunk boundary '+str(length),base64.b64decode(json.loads(received[-1][1])['source'])==content)
        d,o,_=opening(config());bad=raw(dict(o,op='media_input_feed',ticket='wrong',data=base64.b64encode(b'wrong').decode()));check('wrong import ticket cannot write','value'not in bad);bad=raw(dict(o,op='media_input_feed',data=base64.b64encode(b'x'*8193).decode()));check('oversized import chunk is refused','value'not in bad)
        op(dict(o,op='media_input_feed',data=base64.b64encode(b'partial retained').decode()));cancelled=event(d['state'],'cancel');bad=raw(dict(o,op='media_input_finish'));check('cancelled import cannot finish and retains partial file','value'not in bad and any(f.read_bytes()==b'partial retained'for f in(root/'service-files').iterdir()if f.stat().st_size<100));late=event(cancelled['state'],'media_input_result',dict(media_ticket=o['media_ticket'],media_receipt='late'));check('cancelled import callback cannot change state',late['state']==cancelled['state'])
        fresh,o,_=selected(config(),data);d=approved(fresh['state']);owner=dict(purpose=d['effect']['purpose'],ticket=d['effect']['media_upload']);bad=raw(dict(owner,op='media_upload_read',purpose='wrong'));check('wrong purpose cannot consume upload','value'not in bad);part=op(dict(owner,op='media_upload_read'));check('upload first chunk is bounded actual data',base64.b64decode(part['data'])==data[:8192]);path=root/fresh['state']['media_input']['path'];original=path.read_bytes();path.write_bytes(b'changed'+original[7:]);bad=raw(dict(owner,op='media_upload_read'));check('same-size file mutation interrupts further upload','value'not in bad);event(d['state'],'cancel');check('cancelled upload cannot resume','value'not in raw(dict(owner,op='media_upload_read')))
        # Independent selections compete for one actual workspace quota. Sparse
        # reservations count before any input body is received.
        quota=root/'quota';quota.mkdir();openings=[]
        for i in range(4):
            pending,o,stream=opening(None,workspace=quota);openings.append((pending,o));check('concurrent reservation '+str(i),stream['limit']==64*1024*1024)
        fifth=event(None,'media_pick',workspace=quota);bad=raw(dict(op='media_input_open',media_ticket=fifth['effect']['media_ticket'],name='fifth.bin'));check('fifth stream cannot overbook shared storage','value'not in bad and sum(f.stat().st_size for f in(quota/'service-files').iterdir())==256*1024*1024)
        child=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        def child_op(v):child.stdin.write(json.dumps(v)+'\n');child.stdin.flush();return json.loads(child.stdout.readline())
        try:
            child_pick=child_op(dict(op='event',action='media_pick',root=str(quota),inputs={}))['value'];refused=child_op(dict(op='media_input_open',media_ticket=child_pick['effect']['media_ticket'],name='other-process.bin'));check('shared quota also refuses an independent process','value'not in refused)
        finally:child.stdin.close();child.wait(timeout=10)
        pending,o=openings[0];op(dict(o,op='media_input_feed',data=base64.b64encode(b'quota partial').decode()));event(pending['state'],'cancel',workspace=quota);check('cancellation releases only unwritten reservation',sum(f.stat().st_size for f in(quota/'service-files').iterdir())==3*64*1024*1024+len(b'quota partial')and any(f.read_bytes()==b'quota partial'for f in(quota/'service-files').iterdir()if f.stat().st_size<100))
        stream=op(dict(op='media_input_open',media_ticket=fifth['effect']['media_ticket'],name='fifth.bin'));check('released quota can be used by next selection',stream['limit']==64*1024*1024-len(b'quota partial'))
        for pending,_ in openings[1:]:event(pending['state'],'cancel',workspace=quota)
        event(fifth['state'],'cancel',workspace=quota)
        capture_root=root/'capture-quota';capture_root.mkdir();recording=event(None,'media_record',workspace=capture_root);capture_path=pathlib.Path(recording['effect']['capture_path']);captures=[]
        for i in range(4):
            pending,o,stream=opening(None,workspace=capture_root);captures.append(pending);check('recording reservation is respected '+str(i),stream['limit']==(64 if i<3 else 62)*1024*1024)
        capture_path.write_bytes(b'r'*(2*1024*1024));check('native recording can fill its reserved bound without overbooking',sum(f.stat().st_size for f in(capture_root/'service-files').iterdir())==256*1024*1024)
        event(recording['state'],'cancel',workspace=capture_root)
        for pending in captures:event(pending['state'],'cancel',workspace=capture_root)
        check('recording cancellation retains recorded bytes',capture_path.stat().st_size==2*1024*1024)
        # Exercise the actual maximum, rather than only reserving its size.
        maximum=64*1024*1024;chunk=bytes(i%251 for i in range(8192));encoded=base64.b64encode(chunk).decode();digest=hashlib.sha256()
        d,o,_=opening(config(),name='maximum.bin')
        for _ in range(maximum//len(chunk)):op(dict(o,op='media_input_feed',data=encoded));digest.update(chunk)
        receipt=op(dict(o,op='media_input_finish'));ready=event(d['state'],'media_input_result',dict(media_ticket=o['media_ticket'],media_receipt=receipt));check('exact 64 MiB input is fully adopted',ready['state']['media_input']['bytes']==maximum and ready['state']['media_input']['sha256']==digest.hexdigest());exchange(approved(ready['state']));check('maximum binary HTTP upload has exact length and digest',len(received[-1][1])==maximum and hashlib.sha256(received[-1][1]).hexdigest()==digest.hexdigest())
        d,o,_=opening(config(),name='overflow.bin')
        for _ in range(maximum//len(chunk)):op(dict(o,op='media_input_feed',data=encoded))
        refused=raw(dict(o,op='media_input_feed',data=base64.b64encode(b'x').decode()));check('one byte beyond maximum cannot become a truncated success','value'not in refused and'value'not in raw(dict(o,op='media_input_finish')));event(d['state'],'cancel')
        (root/'local.txt').write_text('ordinary local result');check('artifact-free local operation survives scoped upload failure',op(dict(op='tool',tool='read',root=temp,args={'path':'local.txt'}))=='ordinary local result')
    finally:p.stdin.close();p.wait(timeout=10)
server.shutdown();server.server_close()
report=dict(passed=len(passed),checks=passed,scope='Owner checks using actual disk bytes, public shared-core events, chunk APIs and local HTTP sockets. Android SDK compilation is separate; physical picker/socket/lifecycle and live user servers remain unverified.');(PROJECT/'output/checks/media_upload.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
