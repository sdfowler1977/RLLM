"""Connected self-build through the product binary: edit, sanctioned build, gate, journal, refusal, spawn and reap."""
import base64,hashlib,json,os,pathlib,shutil,stat,subprocess,tempfile,time,traceback
PROJECT=pathlib.Path(__file__).resolve().parents[1]
passed=[]
problems=[]
evaluated=[]
def check(name,value,detail=''):
 evaluated.append(name)
 if value:
  passed.append(name)
  return
 problems.append(name)
 print('FAIL '+name,flush=True)
 if detail:print(str(detail)[:2000],flush=True)
GATE='output/tools/gate'
UNITS='output/room-units'
RED_UNITS='output/red-units'
JOURNAL='revisions/self-build.jsonl'
ROLLBACK='revisions/previous-binary'
INHERITED_JOURNAL='revisions/inherited.jsonl'
INHERITED_ROLLBACK='revisions/inherited-rollback'
SOURCE='src/edited.rs'
MARKER='self-build-edit'
CALLER_JOB='lane-e6-caller'
CALLER_INSTRUCTION='continue the same self-build job'
CARRIED_JOB='lane-e6-inherited'
CARRIED_INSTRUCTION='carried instruction for the same job'
ORIGINAL='fn marker() {}\n'
MODES={'none','exec','spawn'}
SOURCES={'caller','inherited','none'}
def canonical(value):
 return json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False)
def fingerprint(path):
 info=path.stat()
 return (info.st_ino,info.st_size,info.st_mtime_ns,hashlib.sha256(path.read_bytes()).hexdigest())
def journal_rows(path):
 rows=[]
 if not path.is_file():return rows
 for line in path.read_text().splitlines():
  if not line:continue
  row=json.loads(line)
  body={key:row[key] for key in row if key!='revision'}
  digest=hashlib.sha256(canonical(body).encode()).hexdigest()
  rows.append((row,digest,canonical(row)==line))
 return rows
def proc_stat(pid):
 path=pathlib.Path('/proc')/str(pid)/'stat'
 for _ in range(25):
  if path.exists():
   text=path.read_text()
   fields=text[text.rfind(')')+2:].split()
   return fields[0],int(fields[1])
  time.sleep(0.02)
 return None
def compile_red(room,rustc):
 fixtures=room/'fixtures'
 fixtures.mkdir()
 source=fixtures/'red.rs'
 source.write_text('fn main() {}\n#[test]\nfn fails() { panic!("red-gate"); }\n')
 subprocess.run([rustc,'--test','--edition','2021','--crate-name','red_gate',str(source),'-o',str(room/RED_UNITS)],check=True)
def build_room(room):
 # The copied build-units.sh is the real sanctioned script. This room crate is one
 # include_str test so rustc and the real gate finish inside the product review window.
 (room/'core').mkdir()
 (room/'core'/'console.rs').write_text('fn main() {}\n#[test]\nfn edited_source_is_present() {\n    let source = include_str!("../src/edited.rs");\n    assert!(source.contains("'+MARKER+'"));\n}\n')
 (room/'src').mkdir()
 (room/'src'/'edited.rs').write_text(ORIGINAL)
 (room/'tools').mkdir()
 shutil.copy2(PROJECT/'tools'/'build-units.sh',room/'tools'/'build-units.sh')
 shutil.copy2(PROJECT/'tools'/'gate.rs',room/'tools'/'gate.rs')
 (room/'output'/'tools').mkdir(parents=True)
 shutil.copy2(PROJECT/'output'/'tools'/ 'gate',room/'output'/'tools'/'gate')
 (room/'output'/'tools'/'gate').chmod(0o755)
 successor=room/'successor.sh'
 successor.write_text('#!/bin/sh\nexit 0\n')
 successor.chmod(0o755)
 compile_red(room,os.environ['CLIENT_RUST']+'/bin/rustc')
def clean_env(extra=None):
 env=dict(os.environ)
 env.pop('RLLM_SELF_JOB',None)
 env.pop('RLLM_SELF_INSTRUCTION',None)
 if extra:env.update(extra)
 return env
with tempfile.TemporaryDirectory(prefix='self-rebuild-',dir=PROJECT/'output') as tmp:
 room=pathlib.Path(tmp)
 build_room(room)
 source=room/SOURCE
 product=None
 error_file=None
 def close_product():
  global product,error_file
  if product is not None:
   try:product.stdin.close()
   except Exception:pass
   try:product.wait(timeout=10)
   except Exception:
    product.kill()
    product.wait(timeout=5)
   product=None
  if error_file is not None:
   error_file.close()
   error_file=None
 def open_product(env):
  global product,error_file
  close_product()
  error_path=room/'product.err'
  error_file=open(error_path,'w')
  product=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=error_file,text=True,env=env)
 def raw(value):
  product.stdin.write(json.dumps(value)+'\n')
  product.stdin.flush()
  line=product.stdout.readline()
  if not line:
   tail=(room/'product.err').read_text()[-2000:]
   raise RuntimeError('product closed stdout '+tail)
  return json.loads(line)
 def op(value):
  reply=raw(value)
  if 'value' not in reply:raise AssertionError(reply.get('error') or reply)
  return reply['value']
 def event(state,action,inputs=None):
  return op(dict(op='event',state=state,action=action,inputs=inputs or {},root=str(room)))
 def reply(pending,text):
  body=json.dumps({'choices':[{'message':{'content':text},'finish_reason':'stop'}]}).encode()
  wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(body)).encode()+b'\r\n\r\n'+body
  return event(pending['state'],'network_result',dict(purpose='model',model_ticket=pending['state']['model_expected'],data=base64.b64encode(wire).decode()))
 def finish(pending):
  effect=pending.get('effect') or {}
  if effect.get('purpose')=='model':return reply(pending,'Recorded the self-build result.')
  return pending
 def propose(state,args):
  opened=event(state,'send',{'draft':'Investigate the workspace and run the reviewed self-build.'})
  if not (opened.get('effect') and opened['effect'].get('purpose')=='model'):
   raise AssertionError(opened.get('state',{}).get('notice') or 'no model request')
  return reply(opened,json.dumps({'tool':'self_build','args':args}))
 def approve_and_run(pending):
  if pending['state'].get('dialog')!='approval':
   raise AssertionError(pending['state'].get('notice') or 'self-build was not held for review')
  approved=event(pending['state'],'approve')
  effect=approved.get('effect') or {}
  if effect.get('kind')!='tool' or effect.get('tool')!='self_build':
   raise AssertionError(approved['state'].get('notice') or 'approval did not dispatch self-build')
  answer=raw(dict(effect,op='tool',root=str(room)))
  return approved,effect,answer
 def settle(approved,effect,answer):
  if 'error' in answer:
   done=event(approved['state'],'failure',dict(purpose='local_tool',tool_ticket=effect.get('tool_ticket'),tool_binding=effect.get('tool_binding'),error=answer['error']))
   return finish(done),answer['error']
  if 'value' not in answer:raise AssertionError(answer)
  done=event(approved['state'],'tool_result',dict({key:effect[key] for key in ['purpose','tool_ticket','tool_binding','child_ticket','child_owner'] if key in effect},output=answer['value']))
  return finish(done),answer['value']
 def base_args(**extra):
  args=dict(path=SOURCE,binary='successor.sh',timeout_seconds=90)
  args.update(extra)
  return args
 held={'state':None,'parent':None,'pid':0}
 def leg(name,body):
  print('leg '+name,flush=True)
  try:body()
  except Exception:
   traceback.print_exc()
   if name not in evaluated:check(name,False,traceback.format_exc()[-2000:])
 def adopt(done):
  held['state']=done['state']
 def over_cap():
  before=fingerprint(source)
  over=propose(held['state'],base_args(content='fn marker() { let _ = "over-cap"; }\n',job='j'*257,instruction='must not edit',reload='',journal=JOURNAL,rollback=ROLLBACK,parent=''))
  try:
   over_text=json.dumps(over.get('effect'))+' '+json.dumps(over.get('state'))
   check('a 257-byte job is refused before the source edit','job exceeds 256 bytes' in over_text and fingerprint(source)==before,over.get('state',{}).get('notice') or over_text[:1500])
  finally:adopt(finish(over))
 def unsanctioned():
  before=fingerprint(source)
  refused=propose(held['state'],base_args(content='fn marker() { let _ = "unsanctioned"; }\n',builder='bash',builder_args=['echo','unsanctioned'],job='lane-e6-refuse',instruction='do not run this builder',reload=''))
  approved,effect,answer=approve_and_run(refused)
  try:
   after=fingerprint(source)
   detail='error='+str(answer.get('error',answer))+' before='+str(before)+' after='+str(after)
   check('an unsanctioned builder is refused and the source bytes stay unchanged','error' in answer and 'builder is not one of the sanctioned commands' in answer.get('error','') and after==before,detail)
  finally:adopt(settle(approved,effect,answer)[0])
 def red_gate():
  red=propose(held['state'],base_args(content='fn marker() { let _ = "red-gate"; }\n',gate=GATE,gate_args=[RED_UNITS],journal=JOURNAL,rollback=ROLLBACK,parent='',reload='spawn',job='lane-e6-red',instruction='red gate must refuse',timeout_seconds=60))
  approved,effect,answer=approve_and_run(red)
  try:
   red_error=answer.get('error','')
   check('a failing unit gate refuses reload with the unit gate is not green wording','unit gate is not green' in red_error and '0/1' in red_error,red_error or answer)
  finally:adopt(settle(approved,effect,answer)[0])
 def green_journal():
  green_content='fn marker() { let _ = "'+MARKER+'"; }\n'
  built=propose(held['state'],base_args(content=green_content,builder='bash',builder_args=['tools/build-units.sh',UNITS],gate=GATE,gate_args=[UNITS],journal=JOURNAL,rollback=ROLLBACK,parent='',reload='',job=CALLER_JOB,instruction=CALLER_INSTRUCTION))
  approved,effect,answer=approve_and_run(built)
  try:
   receipt=answer.get('value',answer.get('error',answer))
   rows=journal_rows(room/JOURNAL)
   binary=room/UNITS
   if not rows:check('empty reload journals the edited source under a recomputed digest whose parent is empty',False,receipt)
   else:
    first,first_digest,first_compact=rows[0]
    held['parent']=rows[-1][1]
    check('empty reload journals the edited source under a recomputed digest whose parent is empty',
          len(rows)==1 and first_compact and first['parent']=='' and first['revision']==first_digest and isinstance(receipt,str) and first_digest in receipt
          and 'Rust cases passed: 1/1; isolated application cases: 0' in receipt
          and source.read_text()==green_content and binary.is_file() and binary.read_bytes()[:4]==b'\x7fELF'
          and first['successor']['mode']=='none' and first['successor']['job_source']=='caller'
          and first['successor']['job']==CALLER_JOB and first['successor']['pid']==product.pid,
          receipt)
  finally:adopt(settle(approved,effect,answer)[0])
 def spawn_child():
  parent=held['parent'] if isinstance(held['parent'],str) else ''
  spawned=propose(held['state'],base_args(content='fn marker() { let _ = "'+MARKER+'"; let _ = "spawn"; }\n',gate=GATE,gate_args=[UNITS],journal=JOURNAL,rollback=ROLLBACK,parent=parent,reload='spawn',job=CALLER_JOB,instruction=CALLER_INSTRUCTION,timeout_seconds=60))
  approved,effect,answer=approve_and_run(spawned)
  try:
   spawn_receipt=answer.get('value','') if isinstance(answer.get('value'),str) else ''
   pid_text=spawn_receipt.rsplit('successor pid ',1)[-1].split(' ',1)[0] if 'successor pid ' in spawn_receipt else ''
   pid=int(pid_text) if pid_text.isdigit() else 0
   held['pid']=pid
   child=proc_stat(pid) if pid else None
   rows=journal_rows(room/JOURNAL)
   spawn_row,spawn_digest,spawn_compact=rows[-1] if rows else ({},'',False)
   check('spawn records a successor pid that is a child of this product process',
         child is not None and child[1]==product.pid and 'successor pid '+str(pid) in spawn_receipt
         and spawn_compact and spawn_row.get('revision')==spawn_digest and isinstance(held['parent'],str) and spawn_row.get('parent')==held['parent']
         and spawn_row.get('successor',{}).get('mode')=='spawn' and spawn_row.get('successor',{}).get('pid')==pid
         and spawn_row.get('successor',{}).get('job_source')=='caller'
         and spawn_row.get('successor',{}).get('env')==['RLLM_SELF_JOB','RLLM_SELF_INSTRUCTION'],
         'receipt='+spawn_receipt+' stat='+str(child)+' product='+str(product.pid)+' parent='+str(held['parent']))
  finally:adopt(settle(approved,effect,answer)[0])
 def reap_child():
  reaped=propose(held['state'],base_args(content='fn marker() { let _ = "'+MARKER+'"; let _ = "reap"; }\n',reload='',job='lane-e6-reap',instruction='reap the successor',timeout_seconds=60))
  approved,effect,answer=approve_and_run(reaped)
  try:
   reap_receipt=answer.get('value',answer.get('error',answer))
   gone=held['pid']>0 and not pathlib.Path('/proc',str(held['pid'])).exists()
   check('the next self-build reaps that successor and the pid is gone',isinstance(reap_receipt,str) and ' reaped 1 successor(s)' in reap_receipt and gone,reap_receipt)
  finally:adopt(settle(approved,effect,answer)[0])
 def inherited_job():
  close_product()
  open_product(clean_env({'RLLM_SELF_JOB':CARRIED_JOB,'RLLM_SELF_INSTRUCTION':CARRIED_INSTRUCTION}))
  state=event(None,'resume')['state']
  state['profiles']=[dict(name='Controlled',endpoint='http://model.test/v1/chat/completions',model='controlled',protocol='chat',stream=False,plain=True)]
  state['agents']=[dict(name='General',tools='self_build,write')]
  held['state']=event(state,'answer_mode:investigate')['state']
  carried=propose(held['state'],dict(path=SOURCE,content='fn marker() { let _ = "'+MARKER+'"; let _ = "inherited"; }\n',binary='successor.sh',gate=GATE,gate_args=[UNITS],journal=INHERITED_JOURNAL,rollback=INHERITED_ROLLBACK,parent='',reload='',timeout_seconds=60))
  approved,effect,answer=approve_and_run(carried)
  try:
   carried_receipt=answer.get('value',answer.get('error',answer))
   inherited_rows=journal_rows(room/INHERITED_JOURNAL)
   if not inherited_rows:check('an omitted job records the carried job as inherited',False,carried_receipt)
   else:
    inherited,inherited_digest,inherited_compact=inherited_rows[0]
    successor=inherited['successor']
    check('an omitted job records the carried job as inherited',
          len(inherited_rows)==1 and inherited_compact and inherited['revision']==inherited_digest and inherited['parent']==''
          and successor['job_source']=='inherited' and successor['job']==CARRIED_JOB
          and successor['instruction']==CARRIED_INSTRUCTION and successor['mode']=='none'
          and successor['pid']==product.pid and isinstance(carried_receipt,str) and inherited_digest in carried_receipt,
          carried_receipt)
  finally:adopt(settle(approved,effect,answer)[0])
 def known_rows():
  every=[]
  for path in (room/JOURNAL,room/INHERITED_JOURNAL):
   for row,digest,compact in journal_rows(path):
    successor=row.get('successor') or {}
    every.append(compact and row.get('revision')==digest and successor.get('mode') in MODES and successor.get('job_source') in SOURCES)
  check('every revision row names a known successor mode and job source',bool(every) and all(every),'rows='+str(len(every)))
 names=['a 257-byte job is refused before the source edit','an unsanctioned builder is refused and the source bytes stay unchanged','a failing unit gate refuses reload with the unit gate is not green wording','empty reload journals the edited source under a recomputed digest whose parent is empty','spawn records a successor pid that is a child of this product process','the next self-build reaps that successor and the pid is gone','an omitted job records the carried job as inherited','every revision row names a known successor mode and job source']
 try:
  open_product(clean_env())
  state=event(None,'resume')['state']
  state['profiles']=[dict(name='Controlled',endpoint='http://model.test/v1/chat/completions',model='controlled',protocol='chat',stream=False,plain=True)]
  state['agents']=[dict(name='General',tools='self_build,write')]
  held['state']=event(state,'answer_mode:investigate')['state']
 except Exception:
  traceback.print_exc()
  for name in names:
   if name not in evaluated:check(name,False,'product setup failed\n'+traceback.format_exc()[-1500:])
 else:
  for name,body in zip(names,[over_cap,unsanctioned,red_gate,green_journal,spawn_child,reap_child,inherited_job,known_rows]):leg(name,body)
 finally:
  err_path=room/'product.err'
  close_product()
  err_text=err_path.read_text()[-4000:] if err_path.exists() else ''
  if err_text and problems:print(err_text,flush=True)
  record=dict(checks=evaluated,count=len(evaluated),failed=problems,scope='Owner checks through the product JSON-lines route: a room-local sanctioned build and unit gate, content-addressed journal digests, refusal before an over-cap job, a real red gate, spawn parentage, reap, and an inherited job.')
  if not problems:record['passed']=passed
  out=PROJECT/'output/checks'
  out.mkdir(parents=True,exist_ok=True)
  (out/'self-rebuild.json').write_text(json.dumps(record,indent=2)+'\n')
  if not problems:print(str(len(passed))+' self-rebuild checks passed',flush=True)
  print(json.dumps(record,indent=2),flush=True)
 if problems:raise SystemExit(1)
