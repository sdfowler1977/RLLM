import tempfile
"""Public JSONL event checks: controlled WebDAV replies, real Rust transitions."""
import base64, json, pathlib, subprocess, hashlib

PROJECT=pathlib.Path(__file__).resolve().parents[1]
(PROJECT/'output').mkdir(exist_ok=True)
testdir=tempfile.TemporaryDirectory(prefix='checks-',dir=PROJECT/'output')
RESULTS=PROJECT/'output'/'checks';RESULTS.mkdir(exist_ok=True)
root = testdir.name
p = subprocess.Popen([str(PROJECT/'output/headless/RLLM'), '--jsonl'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
passed = []
def event(st, action, inputs=None):
    p.stdin.write(json.dumps(dict(op='event', state=st, action=action, inputs=inputs or {}, root=root))+'\n'); p.stdin.flush()
    r = json.loads(p.stdout.readline()); assert 'value' in r, r
    return r['value']
def reply(d, body, status=207):
    b = body.encode()
    data = base64.b64encode(f'HTTP/1.1 {status} Result\r\nContent-Length: {len(b)}\r\n\r\n'.encode()+b).decode()
    e = d['effect']; inputs = {k:e[k] for k in ['purpose','child_ticket','child_owner','model_ticket','drive_ticket'] if k in e}
    inputs['data'] = data
    return event(d['state'], 'network_result', inputs)
def xml(hrefs):
    return '<d:multistatus xmlns:d="DAV:">'+''.join('<d:response><d:href>'+h+'</d:href><d:propstat><d:prop><d:displayname>Observed file</d:displayname></d:prop></d:propstat></d:response>' for h in hrefs)+'</d:multistatus>'
def route(e, method, path):
    assert e['host']=='100.64.0.2' and e['port']==5006 and e['tls'] is True, e
    assert e['tls_host']=='nas.example.test', e
    assert e['wire'].startswith(method+' '+path+' HTTP/1.1\r\n'), e
    assert '\r\nHost: nas.example.test:5006\r\n' in e['wire'], e
try:
    st = event(None,'resume')['state']; st['dialog']=None
    st['drive'] = dict(url='https://100.64.0.2:5006/share/', tls_name='nas.example.test', user='fixture', password='fixture', plain=False)
    d = event(st,'drive_list'); route(d['effect'],'PROPFIND','/share/'); passed.append('manual PROPFIND route and certificate identity')
    allowed = ['https://nas.example.test:5006/share/a%20b.txt','https://100.64.0.2:5006/share/ip.txt','/share/relative.txt']
    forbidden = ['https://evil.test:5006/share/x','https://nas.example.test:5007/share/x','http://nas.example.test:5006/share/x','https://nas.example.test:5006/share2/x','https://nas.example.test:5006/secret/x','/share/%2e%2e/x','/share/%252e%252e/x','/share/%5cx','/share/%00x','https://nas.example.test:5006/share/']
    d = reply(d,xml(allowed+forbidden)); rows=d['state']['remote']; assert len(rows)==3, rows
    assert rows[0]['url']=='https://100.64.0.2:5006/share/a%20b.txt',rows
    passed.extend(['explicit certificate alias accepted and rewritten','configured route and relative href accepted','unrelated hostname refused','different port refused','HTTP downgrade refused','sibling and outside roots refused','decoded traversal refused','double escapes refused','backslash and control bytes refused','aliased collection self entry omitted'])
    opened = event(d['state'],'remote:0'); route(opened['effect'],'GET','/share/a%20b.txt')
    read = reply(opened,'Actual controlled WebDAV file text.',200)
    assert 'Actual controlled WebDAV file text.' in json.dumps(read['ui']),read
    passed.append('manual file open retains VPN route and displays returned bytes')
    no_alias=json.loads(json.dumps(st)); no_alias['drive']['tls_name']=''
    refused=reply(event(no_alias,'drive_list'),xml([allowed[0]])); assert refused['state']['remote']==[]
    passed.append('unconfigured alias refused')
    commented='<?xml version="1.0"?><!-- <tags> & ignored -->'+xml([allowed[0]]).replace('<d:response>','<d:response><!--property comment-->')
    listed=reply(event(st,'drive_list'),commented);assert len(listed['state']['remote'])==1;passed.append('WebDAV listing accepts comments without interpreting their markup or entities')
    character_data='<d:multistatus xmlns:d="DAV:"><d:response><d:href><![CDATA[/share/file.txt]]></d:href><d:propstat><d:prop><d:displayname><![CDATA[File & <notes> &amp;]]></d:displayname></d:prop></d:propstat></d:response></d:multistatus>'
    listed=reply(event(st,'drive_list'),character_data);assert listed['state']['remote'][0]['name']=='File & <notes> &amp;' and listed['state']['remote'][0]['url'].endswith('/share/file.txt');passed.append('CDATA preserves literal property text and usable contained hrefs')
    for label,body in [('DOCTYPE','<!DOCTYPE x SYSTEM "https://evil.test/dtd">'+xml([allowed[0]])),('entity declaration','<!ENTITY x "bad">'+xml([allowed[0]])),('unfinished comment','<multistatus><!--unfinished'),('illegal comment','<multistatus><!--illegal--text--></multistatus>'),('trailing hyphen comment','<multistatus><!--illegal---></multistatus>'),('unfinished CDATA','<multistatus><![CDATA[unfinished'),('invalid character',xml([allowed[0]])+'\x00'),('invalid character reference',xml([allowed[0]]).replace('Observed file','&#0;'))]:
        refused=reply(event(st,'drive_list'),body);assert refused['effect'] is None and refused['state'].get('notice_error') and not refused['state'].get('remote'),(label,refused)
        passed.append('malformed or external XML is refused locally: '+label)
    st['profiles']=[dict(name='Parent connection',protocol='chat',endpoint='http://parent.test/v1/chat/completions',model='parent',plain=True,stream=False,tool_format='structured'),dict(name='Child connection',protocol='chat',endpoint='http://child.test/v1/chat/completions',model='child',plain=True,stream=False,tool_format='structured')]
    st['agents']=[dict(name='Parent',model='0',tools='delegate,drive_list',subagents=[1]),dict(name='Child',model='1',tools='drive_list',subagents=[])]
    def model(d,tool=None,args=None,text='Answer'):
        m={'content':text}
        if tool:m['tool_calls']=[{'id':'call_one','type':'function','function':{'name':tool,'arguments':json.dumps(args or {})}}]
        return reply(d,json.dumps({'choices':[{'index':0,'message':m,'finish_reason':'tool_calls' if tool else 'stop'}]}),200)
    d=event(st,'send',{'draft':'Use a child to inspect the remote folder.'})
    d=model(d,'delegate',{'agent':1,'task':'List the folder and report its actual files.'},'')
    assert d['state']['dialog']=='approval',d
    d=event(d['state'],'approve');assert d['state']['request_index']==1
    d=model(d,'drive_list',{'path':''},'');assert d['state']['dialog']=='approval',d
    d=event(d['state'],'approve');route(d['effect'],'PROPFIND','/share/')
    d=reply(d,xml([allowed[0]]));assert d['state']['request_index']==1 and 'a%20b.txt' in d['effect']['wire'],d
    evidence=[e for c in d['state']['inquiry_records']['items'] for e in c['evidence'] if e['tool']=='drive_list']
    assert len(evidence)==1 and '100.64.0.2' in evidence[0]['output'],evidence
    passed.append('delegated listing normalizes URLs and retains actual returned evidence')
    d=model(d,text='The observed file is a b.txt.');assert d['state']['request_index']==0 and 'The observed file is a b.txt.' in d['effect']['wire'],d
    passed.append('network evidence child returns to captured parent connection')
    d=model(d,text='Reported the observed file.');assert not d['state']['busy'] and d['effect'] is None,d
    passed.append('parent completes after actual child storage result')
    def create(state,content):
        state=json.loads(json.dumps(state));state['dialog']='drive_editor';state['edit']={}
        reviewed=event(state,'drive_create_review',{'path':'new file.txt','content':content})
        assert reviewed['state']['dialog']=='approval',reviewed
        return event(reviewed['state'],'approve')
    for label,content in [('text','Original network contents.'),('empty',''),('Unicode','UTF-8: \u65e5\u672c\u8a9e\nnext line'),('maximum text size','x'*(1024*1024))]:
        created=create(st,content);route(created['effect'],'PUT','/share/new%20file.txt')
        assert '\r\nIf-None-Match: *\r\n' in created['effect']['wire']
        acknowledged=reply(created,'',201);route(acknowledged['effect'],'GET','/share/new%20file.txt')
        assert acknowledged['effect']['limit']>=len(content.encode())+256
        assert acknowledged['state']['drive_creation']['phase']=='verifying' and 'saved successfully' not in json.dumps(acknowledged['ui'])
        verified=reply(acknowledged,content,200)
        assert verified['effect'] is None and verified['state'].get('drive_creation') is None and 'readback verified' in json.dumps(verified['ui']),verified
        assert hashlib.sha256(content.encode()).hexdigest() in json.dumps(verified['ui'])
        passed.append('manual create and actual readback comparison: '+label)
        duplicate=reply(dict(state=verified['state'],effect=acknowledged['effect']),content,200)
        assert duplicate['state']==verified['state'] and duplicate['effect'] is None
        passed.append('completed create rejects duplicate verification: '+str(len(content.encode())))
    created=create(st,'Expected');acknowledged=reply(created,'',204)
    wrong=reply(acknowledged,'Different',200);assert wrong['effect'] is None and 'unconfirmed' in wrong['state']['notice'].lower() and not wrong['state']['busy'];passed.append('mismatched readback never reports save success or replays PUT')
    created=create(st,'Expected');acknowledged=reply(created,'',201)
    missing=reply(acknowledged,'Missing',404);assert missing['effect'] is None and 'unconfirmed' in missing['state']['notice'].lower();passed.append('missing readback remains unconfirmed')
    created=create(st,'Expected');acknowledged=reply(created,'',201)
    broken=event(acknowledged['state'],'failure',{'purpose':'drive_create_verify','error':'Controlled verification transport failure','sent':False,'drive_ticket':acknowledged['effect']['drive_ticket']});assert broken['effect'] is None and 'unconfirmed' in broken['state']['notice'].lower() and not broken['state']['busy'];passed.append('verification transport failure leaves no false success or write replay')
    recovered=create(broken['state'],'Separate reviewed contents');assert recovered['effect']['wire'].startswith('PUT ');passed.append('unconfirmed prior creation does not block another reviewed create')
    created=create(st,'Expected');cancelled=event(created['state'],'cancel')
    late=reply(dict(state=cancelled['state'],effect=created['effect']),'',201);assert late['effect'] is None and late['state']==cancelled['state'] and 'unconfirmed' in late['state']['notice'];passed.append('cancelled write ignores late acknowledgement and reports unknown outcome')
    created=create(st,'Expected');acknowledged=reply(created,'',201);cancelled=event(acknowledged['state'],'cancel')
    late=reply(dict(state=cancelled['state'],effect=acknowledged['effect']),'Expected',200);assert late['effect'] is None and late['state']==cancelled['state'];passed.append('cancelled readback cannot certify a saved file')
    created=create(st,'Expected');acknowledged=reply(created,'',201)
    changed=json.loads(json.dumps(acknowledged['state']));changed['drive']['url']='https://other.test/share/'
    refused=reply(dict(state=changed,effect=acknowledged['effect']),'Expected',200);assert refused['effect'] is None and refused['state']==changed;cancelled=event(refused['state'],'cancel');assert 'unconfirmed' in cancelled['state']['notice'].lower();passed.append('changed drive identity cannot certify prior destination')
    st['agents'][0]['tools']='delegate,drive_list,drive_create';st['agents'][1]['tools']='drive_list,drive_create'
    d=event(st,'send',{'draft':'Have the child save a network file and verify it.'});d=model(d,'delegate',{'agent':1,'task':'Create new file.txt with Expected and verify it.'},'');d=event(d['state'],'approve')
    d=model(d,'drive_create',{'path':'new file.txt','content':'Expected'},'');d=event(d['state'],'approve');write=d
    d=reply(d,'',201);assert d['state']['request_index']==1 and d['effect']['purpose']=='drive_create_verify';passed.append('child waits for readback rather than returning on write acknowledgement')
    stale=reply(dict(state=d['state'],effect=write['effect']),'',201);assert stale['state']==d['state'] and stale['effect'] is None;passed.append('stale child write cannot replace current readback')
    d=reply(d,'Expected',200);assert d['state']['request_index']==1 and 'readback verified' in d['effect']['wire']
    evidence=[e for c in d['state']['inquiry_records']['items'] for e in c['evidence'] if e['tool']=='drive_create'];assert len(evidence)==1 and 'readback verified' in evidence[0]['output'];passed.append('child receives and retains only verified create result')
    d=model(d,text='The file was saved and its readback matched.');assert d['state']['request_index']==0 and 'readback matched' in d['effect']['wire'];d=model(d,text='The verified save completed.');assert not d['state']['busy'];passed.append('verified child create returns through captured parent')
    d=event(st,'send',{'draft':'Have the child save a network file.'});d=model(d,'delegate',{'agent':1,'task':'Create new file.txt with Expected.'},'');d=event(d['state'],'approve');d=model(d,'drive_create',{'path':'new file.txt','content':'Expected'},'');d=event(d['state'],'approve');d=reply(d,'',201);d=reply(d,'Wrong',200)
    assert d['state']['request_index']==0 and d['effect']['purpose']=='model' and 'unconfirmed' in d['effect']['wire'];passed.append('child readback failure returns once to parent without a false result')
    (RESULTS/'network-results.json').write_text(json.dumps({'passed':passed,'scope':'Controlled HTTP replies through real Rust event API; no live NAS or Android TLS handshake'},indent=2))
    print(len(passed),'connected WebDAV checks passed')
finally:
    p.stdin.close();p.wait();testdir.cleanup()
