"""Connected owner checks: actual local HTTP and native shared-core gateway events."""
import concurrent.futures, copy, hashlib, http.client, json, os, pathlib, re, select, subprocess, tempfile, time
PROJECT=pathlib.Path(__file__).resolve().parents[1]
BIN=str(PROJECT/'output/headless/RLLM');passed=[]
def check(name,condition):
 assert condition,name;passed.append(name)
with tempfile.TemporaryDirectory(prefix='a2a-',dir=PROJECT/'output') as temp:
 root=pathlib.Path(temp)/'workspace';root.mkdir();(root/'one.txt').write_text('Actual original observation.\n');(root/'two.txt').write_text('Different actual observation.\n')
 outside=pathlib.Path(temp)/'outside.txt';outside.write_text('OUTSIDE PRIVATE DATA');(root/'escape').symlink_to(outside)
 token='local-test-secret-'+'z'*40;secret=pathlib.Path(temp)/'token';secret.write_text(token+'\n');secret.chmod(0o600)
 p=subprocess.Popen([BIN,'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def envelope(v):
  p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
 def op(v):
  r=envelope(v);assert 'value' in r,r;return r['value']
 def event(st,action,inputs=None):return op(dict(op='event',root=str(root),state=st,action=action,inputs=inputs or {}))
 def command(version='1.0',mid='message-one',tool='inspect',args=None,config=None):
  m={'messageId':mid,'role':'ROLE_USER' if version.startswith('1.') else 'user','parts':[{'data':{'tool':tool,'args':args or {'path':'one.txt'}}}]}
  if version.startswith('0.'):m['kind']='message';m['parts'][0]['kind']='data'
  return {'jsonrpc':'2.0','id':mid,'method':'SendMessage' if version.startswith('1.') else 'message/send','params':dict(message=m,**({'configuration':config} if config is not None else {}))}
 def wire(config,body=None,version='1.0',path='/a2a',method='POST',auth=token,extra=''):
  raw=json.dumps(body) if body is not None else ''
  h=f'{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {len(raw.encode())}\r\n'
  if version is not None:h+=f'A2A-Version: {version}\r\n'
  if auth is not None:h+=f'Authorization: Bearer {auth}\r\n'
  result=op(dict(op='gateway_incoming',config=config,header=h+extra+'\r\n',body=raw))['reply'];head,body=result.split('\r\n\r\n',1)
  return int(head.split()[1]),json.loads(body),head
 def rpc(config,method,params0=None,version='1.0'):
  return wire(config,{'jsonrpc':'2.0','id':123,'method':method,'params':params0 or {}},version)[1]
 # Native gateway effect binds the actual trusted workspace, not client inputs.
 st=event(None,'launch')['state'];started=event(st,'gateway_start',{'listen':'127.0.0.1','port':'8080','token':token,'a2a':True});config=started['effect']['config']
 check('native gateway captures selected root and identity',config['a2a_root']==str(root.resolve()) and len(config['a2a_identity'])==2)
 check('native settings retain explicit opt-in',started['state']['gateway']['a2a'] is True and 'a2a_identity' not in started['state']['gateway'])
 bad=envelope(dict(op='event',root=str(root),state=st,action='gateway_start',inputs={'listen':'0.0.0.0','port':'8080','token':token,'a2a':True,'lan':True}))
 check('native workspace LAN exposure refused','error' in bad or (bad['value']['effect'] is None and 'local-only' in bad['value']['state']['notice']))
 status,card,head=wire(config,version='1.0',method='GET',path='/.well-known/agent-card.json')
 check('v1 card versions and bearer scheme',status==200 and [i['protocolVersion'] for i in card['supportedInterfaces']]==['1.0','0.3'] and card['securityRequirements'][0]['schemes']['bearer']=={'list':[]})
 check('optional capabilities accurately disabled',card['capabilities']=={'streaming':False,'pushNotifications':False})
 status,body,head=wire(config,auth=None);check('authentication precedes method dispatch',status==401 and 'WWW-Authenticate: Bearer' in head)
 check('wrong bearer refused',wire(config,auth='wrong')[0]==401)
 first=wire(config,command())[1]['result']['task'];check('v1 real inspect task completes',first['status']['state']=='TASK_STATE_COMPLETED')
 output=first['artifacts'][0]['parts'][0]['data'];check('task contains actual shared engine output','one.txt' in output['output'] and output['returnedSha256']==hashlib.sha256(output['output'].encode()).hexdigest() and output['complete'])
 second=wire(config,command())[1]['result']['task'];check('identical message retry retains original task',second['id']==first['id'])
 changed=command(args={'path':'two.txt'});check('changed message ID reuse refused',wire(config,changed)[1]['error']['code']==-32602)
 check('get task omits history on request','history' not in rpc(config,'GetTask',{'id':first['id'],'historyLength':0})['result'])
 check('completed task cannot restart',wire(config,dict(command(mid='follow'),params={'message':dict(command(mid='follow')['params']['message'],taskId=first['id'])}))[1]['error']['code']==-32004)
 compare=wire(config,command(mid='comparison',tool='compare',args={'path':'one.txt','other':'two.txt','content':'complete_digest'}))[1]['result']['task'];check('actual complete file comparison',compare['status']['state']=='TASK_STATE_COMPLETED' and all(hashlib.sha256((root/name).read_bytes()).hexdigest() in compare['artifacts'][0]['parts'][0]['data']['output'] for name in ['one.txt','two.txt']))
 directory=wire(config,command(mid='directory',args={'path':'.'}))[1]['result']['task'];check('directory inspection shares working resource limits',directory['status']['state']=='TASK_STATE_COMPLETED' and 'one.txt' in directory['artifacts'][0]['parts'][0]['data']['output'])
 (root/'sub').mkdir();(root/'sub'/'one.txt').write_text('Actual smaller tree')
 directories=wire(config,command(mid='directories',tool='compare',args={'path':'.','other':'sub'}))[1]['result']['task'];check('directory comparison shares working resource limits',directories['status']['state']=='TASK_STATE_COMPLETED')
 legacy=wire(config,command('0.3','legacy'),version=None)[1]['result'];check('missing version defaults to legacy',legacy['kind']=='task' and legacy['status']['state']=='completed')
 check('legacy get task',rpc(config,'tasks/get',{'id':legacy['id']},'0.3')['result']['id']==legacy['id'])
 check('cross-version cached task representation',rpc(config,'GetTask',{'id':legacy['id']})['result']['status']['state']=='TASK_STATE_COMPLETED')
 check('patch version ignored',wire(config,command(mid='patch'),version='1.0.99')[1]['result']['task']['status']['state']=='TASK_STATE_COMPLETED')
 query_reply=wire(config,command(mid='query'),version='1.0.2',path='/a2a?A2A-Version=1.0.7')[1];assert 'result'in query_reply,query_reply
 check('equivalent query and header patches',query_reply['result']['task']['status']['state']=='TASK_STATE_COMPLETED')
 check('conflicting version selection refused',wire(config,command(),path='/a2a?A2A-Version=0.3')[0]==400)
 check('unsupported protocol error',wire(config,command(),version='2.0')[1]['error']['code']==-32009)
 for path in ['../outside.txt','/etc/passwd']:
  check('escaping path refused '+path, 'error' in wire(config,command(mid='escape-'+path,args={'path':path}))[1])
 symlink=wire(config,command(mid='symlink',args={'path':'escape'}))[1]['result']['task'];check('escaping symlink never reveals private data','OUTSIDE PRIVATE DATA' not in json.dumps(symlink))
 check('write operation is not exposed',wire(config,command(mid='write',tool='write'))[1]['error']['code']==-32004)
 check('client cannot choose root',wire(config,command(mid='root-override',args={'path':'one.txt','root':str(outside.parent)}))[1]['error']['code']==-32602)
 check('non-data part rejected',wire(config,{'jsonrpc':'2.0','id':1,'method':'SendMessage','params':{'message':{'messageId':'text','role':'ROLE_USER','parts':[{'text':'read files'}]}}})[1]['error']['code']==-32005)
 check('push notification request refused',wire(config,command(mid='push',config={'taskPushNotificationConfig':{'url':'http://attacker.invalid'}}))[1]['error']['code']==-32003)
 check('stream capability unavailable',rpc(config,'SendStreamingMessage')['error']['code']==-32004)
 check('unknown method refused',rpc(config,'execute_shell')['error']['code']==-32601)
 page=rpc(config,'ListTasks',{'pageSize':2,'historyLength':0})['result'];check('list pages have bounded rows and counts',len(page['tasks'])==2 and page['totalSize']>=5 and page['nextPageToken'])
 check('list omits history and artifacts by default',all('history' not in t and 'artifacts' not in t for t in page['tasks']))
 times=[t['status']['timestamp'] for t in page['tasks']];check('list orders latest status first',times==sorted(times,reverse=True))
 check('changed page filters rejected',rpc(config,'ListTasks',{'pageSize':1,'historyLength':0,'pageToken':page['nextPageToken']})['error']['code']==-32602)
 following=rpc(config,'ListTasks',{'pageSize':2,'historyLength':0,'pageToken':page['nextPageToken']})['result'];check('cursor advances immutable snapshot',not {t['id'] for t in page['tasks']} & {t['id'] for t in following['tasks']})
 check('invalid timestamp rejected',rpc(config,'ListTasks',{'statusTimestampAfter':'2026-01-01T-1:00:00Z'})['error']['code']==-32602)
 check('future timestamp filters older work',rpc(config,'ListTasks',{'statusTimestampAfter':'9999-01-01T00:00:00Z'})['result']['tasks']==[])
 check('invalid page size rejected',rpc(config,'ListTasks',{'pageSize':0})['error']['code']==-32602)
 check('terminal cancel returns protocol error',rpc(config,'CancelTask',{'id':legacy['id']})['error']['code']==-32002)
 check('unknown task uses scoped not-found',rpc(config,'GetTask',{'id':'unknown'})['error']['code']==-32001)
 # Live app policy publication must revoke both new work and previously cached data.
 policy={'schema':1,'rules':[{'id':'current-rule','text':'Do not inspect this workspace','scope':'tool','tool':'inspect','decision':'deny','enabled':True}]}
 op({'op':'harness_policy_commit','state':dict(started['state'],harness_policy=policy)})
 check('cached task unavailable after rule revocation',rpc(config,'GetTask',{'id':legacy['id']})['error']['code']==-32001)
 check('new denied task refused', 'error' in wire(config,command(mid='denied'))[1])
 check('unrelated health remains available',wire(config,method='GET',path='/health')[0]==200)
 # This state is the current policy epoch1, not an old saved rollback.
 op({'op':'harness_policy_commit','state':dict(started['state'],harness_policy=None,harness_policy_epoch=1)})
 op({'op':'a2a_close','config':config});check('stopped interface retires task memory','error' in rpc(config,'GetTask',{'id':legacy['id']}))
 config=event(started['state'],'gateway_start',{'listen':'127.0.0.1','port':'8080','token':token,'a2a':True})['effect']['config']
 # Pinned root identity prevents replacement with a different directory.
 root.rename(root.with_name('held'));root.mkdir();check('root replacement refused',wire(config,method='GET',path='/.well-known/agent-card.json')[0]==503)
 p.stdin.close();p.wait(timeout=10)
 # Real socket listener (not a synthetic success response), both protocol versions.
 server=subprocess.Popen([BIN,'--a2a','--root',str(root.with_name('held')),'--token-file',str(secret),'--port','0'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
 try:
  assert select.select([server.stderr],[],[],10)[0],'listener startup timed out'
  startup=server.stderr.readline();port=int(re.search(r':(\d+)/a2a',startup)[1]);check('headless listener starts without a model',port>0)
  def request_http(body,version='1.0',auth=token,path='/a2a',method='POST'):
   conn=http.client.HTTPConnection('127.0.0.1',port,timeout=40);headers={'Content-Type':'application/json','A2A-Version':version}
   if auth is not None:headers['Authorization']='Bearer '+auth
   conn.request(method,path,None if body is None else json.dumps(body),headers);r=conn.getresponse();data=r.read();conn.close();return r.status,json.loads(data)
  check('actual HTTP authentication enforced',request_http(command(),auth=None)[0]==401)
  t=request_http(command(mid='socket'))[1]['result']['task'];check('actual HTTP shared inspection completes',t['status']['state']=='TASK_STATE_COMPLETED')
  check('actual HTTP legacy binding works',request_http(command('0.3','socket-old'),'0.3')[1]['result']['kind']=='task')
  check('actual HTTP card advertised local URL',request_http(None,path='/.well-known/agent-card.json',method='GET')[1]['supportedInterfaces'][0]['url']==f'http://127.0.0.1:{port}/a2a')
  large=root.with_name('held')/'large.bin'
  with large.open('wb') as f:f.truncate(128*1024*1024)
  # Actual independent file work must overlap under the discovered host default.
  # Query the shared controller policy rather than assuming host CPU inventory.
  probe=subprocess.Popen([BIN,'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
  probe.stdin.write(json.dumps(dict(op='event',root=str(root.with_name('held')),state=None,action='resume',inputs={}))+'\n');probe.stdin.flush();policy=json.loads(probe.stdout.readline())['value']['state']['policy'];probe.stdin.close();probe.wait(timeout=10)
  count=policy['workers'];check('absent device metadata discovers bounded host workers',1<=count<=4)
  jobs=[]
  for n in range(count):
   item=request_http(command(mid='parallel-'+str(n),args={'path':'large.bin','content':'complete_digest'},config={'returnImmediately':True}))[1];assert 'result'in item,item;jobs.append(item['result']['task'])
  states=[request_http({'jsonrpc':'2.0','id':300+n,'method':'GetTask','params':{'id':job['id']}})[1]['result']for n,job in enumerate(jobs)]
  check('discovered default admits independent actual scans concurrently',all(job['status']['state']in ['TASK_STATE_SUBMITTED','TASK_STATE_WORKING']for job in states)and len({job['id']for job in states})==count)
  overload=request_http(command(mid='parallel-overload',args={'path':'large.bin','content':'complete_digest'},config={'returnImmediately':True}))[1];check('global task capacity refuses only the excess request','error'in overload and overload['error']['code']==-32000)
  cancel=request_http({'jsonrpc':'2.0','id':310,'method':'CancelTask','params':{'id':jobs[0]['id']}})[1]['result'];check('one concurrent scan can be cancelled independently',cancel['status']['state']=='TASK_STATE_CANCELED'and not cancel['artifacts'])
  deadline=time.monotonic()+35;finished=[]
  while len(finished)<len(jobs)-1:
   finished=[request_http({'jsonrpc':'2.0','id':320+n,'method':'GetTask','params':{'id':job['id']}})[1]['result']for n,job in enumerate(jobs[1:])]
   assert all(x['status']['state']not in ['TASK_STATE_FAILED','TASK_STATE_CANCELED']for x in finished),finished
   if all(x['status']['state']=='TASK_STATE_COMPLETED'for x in finished):break
   assert time.monotonic()<deadline;finished=[];time.sleep(.05)
  check('other concurrent scans complete after scoped cancellation',len(finished)==count-1 and all(x['status']['state']=='TASK_STATE_COMPLETED'for x in finished))
  source_digest=hashlib.sha256()
  with large.open('rb')as stream:
   while chunk:=stream.read(1024*1024):source_digest.update(chunk)
  expected=source_digest.hexdigest();check('concurrent task results retain exact source and output identity',all(expected in x['artifacts'][0]['parts'][0]['data']['output']and x['artifacts'][0]['parts'][0]['data']['returnedSha256']==hashlib.sha256(x['artifacts'][0]['parts'][0]['data']['output'].encode()).hexdigest()for x in finished))
  running=request_http(command(mid='cancel-running',args={'path':'large.bin','content':'complete_digest'},config={'returnImmediately':True}))[1]['result']['task']
  check('nonblocking task returns before large scan completes',running['status']['state'] in ['TASK_STATE_SUBMITTED','TASK_STATE_WORKING'])
  cancelled=request_http({'jsonrpc':'2.0','id':200,'method':'CancelTask','params':{'id':running['id']}})[1]['result']
  check('active HTTP task cancellation accepted',cancelled['status']['state']=='TASK_STATE_CANCELED' and cancelled['artifacts']==[])
  again=request_http({'jsonrpc':'2.0','id':201,'method':'CancelTask','params':{'id':running['id']}})[1]['result']
  check('v1 cancellation is idempotent',again['status']['state']=='TASK_STATE_CANCELED')
  deadline=time.monotonic()+5
  while True:
   following=request_http(command(mid='after-cancel'))[1]
   if 'result' in following:break
   assert following['error']['code']==-32000 and time.monotonic()<deadline,following
   time.sleep(.02)
  check('cancelled scan does not cancel another task',following['result']['task']['status']['state']=='TASK_STATE_COMPLETED')
  check('cancelled task publishes no late artifact',request_http({'jsonrpc':'2.0','id':202,'method':'GetTask','params':{'id':running['id']}})[1]['result']['artifacts']==[])
  active=request_http(command(mid='blocking-owned',args={'path':'large.bin','content':'complete_digest'},config={'returnImmediately':True}))[1]['result']['task']
  with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
   waiting=executor.submit(request_http,command(mid='blocking-owned',args={'path':'large.bin','content':'complete_digest'}))
   time.sleep(.05)
   probe=command(mid='blocked-admission');probe['params']['message']['contextId']='blocked-context'
   overload=request_http(probe)[1]
   check('blocking wait admission leaves control capacity',overload['error']['code']==-32000)
   began=time.monotonic();control=request_http({'jsonrpc':'2.0','id':203,'method':'GetTask','params':{'id':active['id']}})[1]
   check('status remains responsive during blocking request',time.monotonic()-began<2 and control['result']['id']==active['id'])
   cancel=request_http({'jsonrpc':'2.0','id':204,'method':'CancelTask','params':{'id':active['id']}})[1]['result']
   check('cancellation remains available during blocking request',cancel['status']['state']=='TASK_STATE_CANCELED')
   finished=waiting.result(timeout=5)[1]['result']['task'];check('blocked retry receives terminal cancellation',finished['status']['state']=='TASK_STATE_CANCELED')
  rejected=request_http({'jsonrpc':'2.0','id':205,'method':'ListTasks','params':{'contextId':'blocked-context'}})[1]['result']
  check('overloaded blocking call creates no hidden task',rejected['tasks']==[])
  fresh=request_http(command(mid='after-blocking'))[1]
  check('completion releases blocking admission',fresh['result']['task']['status']['state']=='TASK_STATE_COMPLETED')
  conn=http.client.HTTPConnection('127.0.0.1',port,timeout=10);conn.request('POST','/a2a','{"jsonrpc":"2.0","id":1,"id":2}',{'Authorization':'Bearer '+token,'Content-Type':'application/json','A2A-Version':'1.0'});r=conn.getresponse();check('duplicate JSON fields rejected on wire',json.loads(r.read())['error']['code']==-32700);conn.close()
 finally:
  server.terminate();server.wait(timeout=10)
 check('SIGTERM stops local listener cleanly',server.returncode==0)
 secret.chmod(0o644);failed=subprocess.run([BIN,'--a2a','--root',str(root),'--token-file',str(secret),'--port','0'],capture_output=True,text=True,timeout=10)
 check('publicly readable token file refused',failed.returncode!=0 and 'private regular file' in failed.stderr)
print(json.dumps({'passed':len(passed),'checks':passed,'observed_host_workers':count,'scope':'Owner connected checks: actual host sockets, controller operations, bounded task lifecycle and file observations.'},indent=2))
(PROJECT/'output/checks').mkdir(exist_ok=True)
(PROJECT/'output/checks/a2a.json').write_text(json.dumps({'passed':passed,'observed_host_workers':count},indent=2)+'\n')
