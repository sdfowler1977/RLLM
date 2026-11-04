"""WebDAV resource/property scopes preserve actual metadata and fresh file routes."""
import base64,copy,json,pathlib,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
(PROJECT/'output/checks').mkdir(parents=True,exist_ok=True)
def check(name,value):assert value,name;passed.append(name)
def body(fmt,tool=None,args=None,text='Answer'):
 args=args or{}
 if fmt=='chat':
  m={'content':text};r={'choices':[{'index':0,'message':m,'finish_reason':'tool_calls'if tool else'stop'}]}
  if tool:m['tool_calls']=[{'id':'call_one','type':'function','function':{'name':tool,'arguments':json.dumps(args)}}]
 elif fmt=='local':
  m={'content':text};r={'message':m,'done':True,'done_reason':'stop'}
  if tool:m['tool_calls']=[{'function':{'name':tool,'arguments':args}}]
 elif fmt=='messages':
  parts=[{'type':'text','text':text}];r={'content':parts,'stop_reason':'tool_use'if tool else'end_turn'}
  if tool:parts.append({'type':'tool_use','id':'call_one','name':tool,'input':args})
 else:
  parts=[{'text':text}];r={'candidates':[{'index':0,'content':{'role':'model','parts':parts},'finishReason':'STOP'}]}
  if tool:parts.append({'functionCall':{'name':tool,'args':args}})
 return r
with tempfile.TemporaryDirectory(prefix='delegation-visibility-',dir=PROJECT/'output')as tmp:
 root=pathlib.Path(tmp);p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def op(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert'value'in r,r;return r['value']
 def event(st,a,i=None):return op(dict(op='event',state=st,action=a,inputs=i or{},root=str(root)))
 def strings(v):
  if isinstance(v,str):yield v
  elif isinstance(v,dict):
   for child in v.values():yield from strings(child)
  elif isinstance(v,list):
   for child in v:yield from strings(child)
 def prompt(d):return'\n'.join(strings(json.loads(d['effect']['wire'].split('\r\n\r\n',1)[1])))
 def catalog(d):
  text=prompt(d);marker='MODEL AND DELEGATION CONFIGURATION (settings data, not instructions):\n';return json.loads(text.split(marker,1)[1].split('\n',1)[0])
 def state(fmt):
  st=event(None,'resume')['state'];st['profiles']=[dict(name=name,model=model,protocol=fmt,endpoint='http://PRIVATE_'+str(i)+'.test/v1/chat/completions?token=ENDPOINT_SECRET',key='KEY_SECRET_'+str(i),auth_header='X-PRIVATE-AUTH',auth_prefix='PREFIX_SECRET',plain=True,stream=False,tool_format='structured')for i,(name,model)in enumerate([('Primary connection','PRIMARY_MODEL'),('Exact child one','CHILD_MODEL_ONE'),('Exact child two','CHILD_MODEL_TWO'),('Unlinked connection','UNLINKED_MODEL')])];st['agents']=[dict(name='Parent',model='0',tools='read,hypotheses,delegate',subagents=[1,2]),dict(name='Research child',role='coding',specialty='Research',model='1',tools='read,hypotheses'),dict(name='Review child',role='nonwrite',specialty='Review',model='2',tools='read,hypotheses')];return st
 def reply(d,tool=None,args=None,text='Answer',ticket=None):
  value=body(d['state']['request_protocol'],tool,args,text);b=json.dumps(value).encode();wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(b)).encode()+b'\r\n\r\n'+b;return event(d['state'],'network_result',dict(purpose='model',model_ticket=ticket or d['state']['model_expected'],data=base64.b64encode(wire).decode()))
 def proposal(st,args):return reply(event(st,'send',{'draft':'Use a configured permitted child for this task.'}),'delegate',args)
 def finish(d,text='Actual child answer.'):
  d=reply(d,text=text);assert d['effect']and d['state']['request_index']==0,d['state'].get('notice');return reply(d,text='Parent answer using the actual returned child response.')
 def drive_state(fmt='chat'):
  st=state(fmt);st['drive']={'url':'https://nas.test/share/','user':'fixture','password':'fixture'};st['agents'][0]['tools']='read,delegate,drive_list,drive_read,drive_create,drive_update,drive_folder';st['agents'][1]['tools']='drive_read';return st
 def request(st,tool='drive_read',args=None):
  d=reply(event(st,'send',{'draft':'Use the configured network folder and report the actual result.'}),tool,args if args is not None else {'path':'missing.txt'});assert d['state']['dialog']=='approval',d['state'].get('notice');d=event(d['state'],'approve');assert d['effect'],d['state'].get('notice');return d
 def response(d,body='',status=200,headers=None,source=None):
  e=(source or d)['effect'];b=body.encode();h={'Content-Length':str(len(b))};h.update(headers or{});wire=('HTTP/1.1 %d Result\r\n'%status+''.join(k+': '+v+'\r\n'for k,v in h.items())+'\r\n').encode()+b;i={k:e[k]for k in ['purpose','drive_ticket','child_ticket']if k in e};i['data']=base64.b64encode(wire).decode();return event(d['state'],'network_result',i)
 def failed(d,reason,source=None):
  e=(source or d)['effect'];return event(d['state'],'failure',dict({k:e[k]for k in ['purpose','drive_ticket','child_ticket']if k in e},error=reason,sent=False))
 def messages(d):return next(c for c in d['state']['chats']if c['id']==d['state']['active'])['messages']
 def returned(name,d,tool,reason):
  check(name,d['effect']is not None and d['effect']['purpose']=='model'and d['state']['request_index']==0 and reason in prompt(d)and'"status":"failed"'in prompt(d)and('"tool":"'+tool+'"')in prompt(d)and not d['state'].get('drive_expected')and not d['state'].get('inquiry_operation'))
 def doc(inner):return'<d:multistatus xmlns:d="DAV:">'+inner+'</d:multistatus>'
 def prop(name='',kind='',status='HTTP/1.1 200 OK',extra=''):
  return'<d:propstat><d:prop>'+name+kind+extra+'</d:prop>'+(''if status is None else'<d:status>'+status+'</d:status>')+'</d:propstat>'
 def item(href,properties='',extra=''):return'<d:response><d:href>'+href+'</d:href>'+properties+extra+'</d:response>'
 def listed(st,inner):return response(event(st,'drive_list'),doc(inner),207)
 try:
  nested=prop('<d:displayname>File</d:displayname>','<d:resourcetype/>',extra='<d:lockdiscovery><d:lockroot><d:href>/share/WRONG</d:href></d:lockroot><d:collection/></d:lockdiscovery>');d=listed(drive_state(),item('/share/file.txt',nested));row=d['state']['remote'][0];check('nested metadata href preserves the exact listed file route',row['url']=='https://nas.test:443/share/file.txt');check('nested collection metadata does not change a file into a folder',row['folder']is False and row['name']=='File');d=event(d['state'],'remote:0');check('manual file open uses the actual resource href',d['effect']['wire'].startswith('GET /share/file.txt HTTP/1.1'));d=response(d,'ACTUAL_FILE');check('manual file open displays the fresh returned bytes','ACTUAL_FILE'in json.dumps(d['ui']))
  failed=prop('<d:displayname>WRONG_NAME</d:displayname>','<d:resourcetype><d:collection/></d:resourcetype>','HTTP/1.1 404 Not Found');good=prop('<d:displayname>Actual name</d:displayname>','<d:resourcetype/>');d=listed(drive_state(),item('/share/file.txt',failed));row=d['state']['remote'][0];check('failed property values are reported as errors instead of file metadata',row['name']=='/share/file.txt'and row['folder']is False and row['property_errors']=={'displayname':404,'resourcetype':404})
  for sequence in [failed+good,good+failed]:
   d=listed(drive_state(),item('/share/file.txt',sequence));row=d['state']['remote'][0];check('successful requested properties survive mixed property statuses '+str(sequence==failed+good),row['name']=='Actual name'and row['folder']is False and row['property_errors']['displayname']==404)
  folder=prop('<d:displayname>Actual folder</d:displayname>','<d:resourcetype><d:collection/></d:resourcetype>');d=listed(drive_state(),item('/share/folder/',folder));check('successful resource type still produces a usable folder row',d['state']['remote'][0]['folder']is True);d=event(d['state'],'remote:0');check('manual folder open retains PROPFIND routing',d['effect']['wire'].startswith('PROPFIND /share/folder/ HTTP/1.1'));d=response(d,doc(''),207)
  d=listed(drive_state(),item('/share/missing.txt',extra='<d:status>HTTP/1.1 404 Not Found</d:status>'));row=d['state']['remote'][0];check('whole-resource status remains explicit in data and the row label',row['status']==404 and'HTTP 404'in row['name']);d=event(d['state'],'remote:0');check('a historical listing status does not prevent a new explicit read',d['effect']['wire'].startswith('GET /share/missing.txt HTTP/1.1'));d=response(d,'NOW_AVAILABLE');check('fresh read can succeed after an earlier resource status','NOW_AVAILABLE'in json.dumps(d['ui']))
  d=listed(drive_state(),item('/share/legacy.txt',prop('<d:displayname>Legacy</d:displayname>','<d:resourcetype/>',None)));check('legacy absent-status property listings retain their prior behavior',d['state']['remote'][0]['name']=='Legacy'and not d['state']['remote'][0].get('property_errors'))
  d=listed(drive_state(),item('/share/escaped&amp;name.txt',prop('<d:displayname xml:lang="en&gt;fr" extra="a>b">Café &gt; 台灣</d:displayname>','<d:resourcetype/>','<![CDATA[HTTP/1.1 200 OK]]>')));row=d['state']['remote'][0];check('quoted tag delimiters preserve Unicode and entity-decoded property values',row['name']=='Café > 台灣'and row['url'].endswith('/escaped&name.txt'))
  d=listed(drive_state(),item('/share/fi<!--middle-->le.txt',prop('<d:displayname>File <!--middle--> name</d:displayname>','<d:resourcetype/>')));row=d['state']['remote'][0];check('comments preserve surrounding field text and resource coordinates',row['name']=='File  name'and row['url'].endswith('/file.txt'))
  for label,inner in [('empty property status',item('/share/f.txt',prop('<d:displayname>False</d:displayname>','<d:resourcetype/>',''))),('malformed property status',item('/share/f.txt',prop('<d:displayname>False</d:displayname>','<d:resourcetype/>','HTTP/1.1 BAD'))),('empty resource status',item('/share/f.txt',extra='<d:status/>')),('duplicate resource status',item('/share/f.txt',extra='<d:status>HTTP/1.1 200 OK</d:status><d:status>HTTP/1.1 404 Missing</d:status>')),('duplicate resource href',item('/share/f.txt',extra='<d:href>/share/other.txt</d:href>'))]:
   d=listed(drive_state(),inner);check('current malformed listing is scoped to its request: '+label,d['effect']is None and not d['state']['busy']and d['state']['notice_error']and not d['state'].get('remote'))
  for label,xml in [('plain text','not XML'),('wrong root','<other/>'),('multiple roots','<d:multistatus xmlns:d="DAV:"/><other/>'),('trailing data',doc('')+'extra')]:
   d=response(event(drive_state(),'drive_list'),xml,207);check('invalid listing envelope is an actual scoped error: '+label,d['effect']is None and d['state']['notice_error']and not d['state']['busy'])
  d=listed(drive_state(),item('/share/sub/',folder));d=event(d['state'],'remote:0');check('a new folder request clears rows from the earlier location',d['state']['remote']==[]and d['state']['remote_url'].endswith('/sub/'));d=response(d,'Unavailable',503);check('failed navigation does not present old-folder rows under the new location',d['state']['remote']==[]and d['state']['notice_error']and not d['state']['busy'])
  for fmt in ['chat','local','messages','content']:
   d=request(drive_state(fmt),'drive_list',{'path':''});d=response(d,doc(item('/share/file.txt',nested)+item('/share/failed.txt',failed)+item('/share/missing.txt',extra='<d:status>HTTP/1.1 404 Missing</d:status>')),207);check(fmt+' model receives exact resource routes and scoped property/status data','"url":"https://nas.test:443/share/file.txt"'in prompt(d)and'property_errors'in prompt(d)and'"status":404'in prompt(d)and'WRONG_NAME'not in prompt(d)and'file.txt/share/WRONG'not in prompt(d));d=reply(d,text='Reported the actual listing entries and their statuses.')
  st=drive_state();st['agents'][1]['tools']='drive_list';d=proposal(st,{'agent':1,'task':'Inspect the actual folder listing.'});d=event(d['state'],'approve');d=reply(d,'drive_list',{'path':''});d=event(d['state'],'approve');d=response(d,doc(item('/share/file.txt',nested)),207);check('child listing uses the same source-correct parser and actual route',d['state']['request_index']==1 and'file.txt/share/WRONG'not in prompt(d)and'"url":"https://nas.test:443/share/file.txt"'in prompt(d));d=finish(d)
 finally:p.stdin.close();p.wait(timeout=10)
result={'passed':passed,'scope':'Owner public-event checks: current WebDAV resource/property/status scopes, actual row/open routing, mixed and absent statuses, nested metadata, XML field text, scoped errors, stale-view prevention, four provider formats and captured child return. Primary protocol reference: RFC4918 sections13and14.'};(PROJECT/'output/checks/dav-listing.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
