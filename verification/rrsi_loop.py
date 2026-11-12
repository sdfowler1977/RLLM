"""Connected self-improvement loop through the product binary: workspace, journal, critic, resume, tamper, and non-refine."""
import base64,hashlib,json,os,pathlib,subprocess,tempfile,traceback
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
TASK='Write the bounded note.'
CHECKS='REQUIRED-ALPHA\nREQUIRED-BETA'
CONTROL='CONTROL-RULES'
REJECTED='REJECTED-REVISION'
REFUSAL='refused because component prompt with that hypothesis was already rejected in round 1'
PROFILE=dict(name='Controlled',endpoint='http://model.test/v1/chat/completions',model='controlled',protocol='chat',stream=False,plain=True,tool_format='structured')
AGENT=dict(name='General',tools='read',instructions='INCUMBENT RULES')
def canonical(value):
 return json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False)
def row_seal(row):
 body={key:row[key] for key in row if key!='revision'}
 digest=hashlib.sha256(canonical(body).encode()).hexdigest()
 revision=row.get('revision')
 return isinstance(revision,str) and revision==digest and isinstance(row.get('run_digest'),str) and len(row['run_digest'])==64
def read_rows(path):
 if not path.is_file():return []
 rows=[]
 for line in path.read_text().splitlines():
  if line:rows.append(json.loads(line))
 return rows
def sha256(path):
 return hashlib.sha256(path.read_bytes()).hexdigest()
def leg(name,body):
 print('leg '+name,flush=True)
 try:body()
 except Exception:
  traceback.print_exc()
  if name not in evaluated:check(name,False,traceback.format_exc()[-2000:])
with tempfile.TemporaryDirectory(prefix='rrsi-loop-',dir=PROJECT/'output') as tmp:
 room=pathlib.Path(tmp)
 data_home=room/'xdg'
 data_home.mkdir()
 default_root=data_home/'rllm'
 env=dict(os.environ)
 env['XDG_DATA_HOME']=str(data_home)
 env.pop('RLLM_SELF_JOB',None)
 env.pop('RLLM_SELF_INSTRUCTION',None)
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
 def open_product():
  global product,error_file
  close_product()
  error_file=open(room/'product.err','w')
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
 def fresh():
  state=event(None,'resume')['state']
  state['profiles']=[dict(PROFILE)]
  state['agents']=[dict(AGENT)]
  return state
 def run_of(pending):
  return pending['state'].get('task_run') or pending['state'].get('refinement_pending') or {}
 def outcome_text(pending):
  run=run_of(pending)
  return 'phase='+str(run.get('phase'))+' notice='+str(pending['state'].get('notice') or '')+' decision='+str(run.get('round_note') or '')
 def files_here():
  if not default_root.is_dir():return []
  return sorted(path.name for path in default_root.iterdir() if path.is_file())
 held={}
 def source_digest_of(workspace):
  # The refine arm records rrsi::source_digest when approval_root is non-empty.
  # A missing rllm-harness.txt is the digest "absent", not a missing field.
  path=pathlib.Path(workspace)/'rllm-harness.txt'
  if path.is_symlink() or not path.is_file():return 'absent'
  return hashlib.sha256(path.read_bytes()).hexdigest()
 def workspace_leg():
  opened=event(fresh(),'refine',{'task':TASK})
  run=opened['state'].get('task_run') or {}
  approval=opened['state'].get('approval_root')
  workspace=run.get('workspace')
  source=run.get('source_digest')
  expected=source_digest_of(approval) if isinstance(approval,str) and approval else ''
  held['after_entry']=opened
  check('refine entry copies approval_root onto task_run.workspace and records source_digest',
        isinstance(approval,str) and approval==str(room.resolve()) and workspace==approval and source==expected and expected!='',
        'approval_root='+str(approval)+' workspace='+str(workspace)+' source_digest='+str(source)+' expected='+str(expected)+' canonical='+str(room.resolve())+' keys='+str(sorted(run)))
 def non_refine_leg():
  event(held['after_entry']['state'],'cancel')
  before=files_here()
  delegated=event(fresh(),'delegate',{'task':'Read the notes.'})
  delegate_run=delegated['state'].get('task_run') or {}
  delegate_workspace=delegate_run.get('workspace')
  pooled=reply(delegated,'candidates: 1\nrewritten harness')
  delegate_note=pooled['state'].get('notice') or ''
  delegate_left=pooled['state'].get('task_run')
  state=pooled['state']
  chat=next(item for item in state['chats'] if item['id']==state['active'])
  while len(chat['messages'])<5:
   sent=event(state,'send',{'draft':'Extra line '+str(len(chat['messages']))})
   state=reply(sent,'Extra answer '+str(len(chat['messages'])))['state']
   chat=next(item for item in state['chats'] if item['id']==state['active'])
  compacted=event(state,'compact')
  compact_run=compacted['state'].get('task_run') or {}
  compact_workspace=compact_run.get('workspace')
  done=reply(compacted,'candidates: 2\nrewritten harness\n---\nanother rewrite')
  summary=next(item for item in done['state']['chats'] if item['id']==done['state']['active']).get('context_summary')
  after=files_here()
  grew=[name for name in after if name not in before]
  pool_text=''
  for name in after:
   pool_text+= (default_root/name).read_text()
  worktrees=(room/'.rllm-worktrees').exists() or (default_root/'.rllm-worktrees').exists()
  check('a non-refine run carries no workspace and does not attempt the pool',
        delegated['state'].get('approval_root')==str(room.resolve())
        and delegate_run.get('mode')=='delegate' and not delegate_workspace
        and delegate_left is None and delegate_note=='Agent task result saved'
        and compact_run.get('mode')=='compact' and compact_workspace in (None,'')
        and done['state'].get('task_run') is None and summary=='candidates: 2\nrewritten harness\n---\nanother rewrite'
        and not grew and 'pool_status' not in pool_text and not worktrees,
        'delegate_workspace='+str(delegate_workspace)+' delegate_note='+delegate_note
        +' compact_workspace='+str(compact_workspace)+' summary='+str(summary)
        +' grew='+str(grew)+' worktrees='+str(worktrees))
 def critic_leg():
  pending=event(fresh(),'refine',{'task':TASK,'response_checks':CHECKS,'refinement_rounds':'2'})
  pending=reply(pending,'Original reply.')
  control=reply(pending,CONTROL)
  control_text=outcome_text(control)
  pending=reply(control,'First partial answer REQUIRED-ALPHA.')
  pending=reply(pending,REJECTED)
  pending=reply(pending,'First replay missed everything.')
  treated=reply(pending,REJECTED)
  treated_text=outcome_text(treated)
  held['refusal']=treated
  held['critic_files']=files_here()
  check('a second round decision changes when the ledger already rejected that component',
        control['state']['task_run']['phase']=='probe' and REFUSAL not in control_text
        and REFUSAL in treated_text and control_text!=treated_text
        and treated['state'].get('task_run',{}).get('phase')!='probe',
        'control='+control_text+'\ntreated='+treated_text)
 def journal_leg():
  if held.get('refusal'):event(held['refusal']['state'],'cancel')
  pending=event(fresh(),'refine',{'task':TASK,'response_checks':CHECKS})
  for text in ['Partial report REQUIRED-ALPHA.','Candidate rules for the bounded report.','Completed report REQUIRED-ALPHA REQUIRED-BETA.','Revision rules for the bounded report.','Revised report missed the second phrase. REQUIRED-ALPHA.']:
   pending=reply(pending,text)
  settled=pending['state'].get('refinement_pending') or {}
  held['settled']=settled
  digest=settled.get('run_digest') if isinstance(settled.get('run_digest'),str) else ''
  ledger_name=digest+'.jsonl' if len(digest)==64 else ''
  ledger_path=default_root/ledger_name if ledger_name else None
  library_path=default_root/'experience.jsonl'
  ledger_rows=read_rows(ledger_path) if ledger_path else []
  library_rows=read_rows(library_path)
  held['ledger_name']=ledger_name
  held['ledger_rows']=ledger_rows
  linked=bool(ledger_rows) and all(row.get('run_digest')==digest and row_seal(row) for row in ledger_rows)
  chained=bool(ledger_rows) and ledger_rows[0].get('previous')=='' and all(ledger_rows[index].get('previous')==ledger_rows[index-1].get('revision') for index in range(1,len(ledger_rows)))
  this_library=[row for row in library_rows if row.get('run_digest')==digest]
  library_ok=bool(this_library) and all(row_seal(row) for row in this_library) and any(row.get('harness')==settled.get('composition') for row in this_library)
  named=settled.get('edit_ledger_root')==str(default_root) and settled.get('edit_ledger_path')==ledger_name and ledger_name!='edits.jsonl'
  no_fixed_name=not (default_root/'edits.jsonl').is_file()
  chain=[(row.get('previous')==('' if index==0 else ledger_rows[index-1].get('revision')), row.get('run_digest')==digest, row_seal(row), row.get('verdict'), (row.get('hypothesis') or '')[:64]) for index,row in enumerate(ledger_rows)]
  check('the digest-named ledger chains each revision to this run and experience.jsonl seals the settled row',
        named and linked and chained and library_ok and no_fixed_name and settled.get('phase')=='review',
        'root='+str(settled.get('edit_ledger_root'))+' path='+str(settled.get('edit_ledger_path'))
        +' digest='+digest+' phase='+str(settled.get('phase'))+' files='+str(files_here())
        +' edits.jsonl='+str((default_root/'edits.jsonl').is_file())
        +' rows='+str(len(ledger_rows))+' linked='+str(linked)+' chained='+str(chained)
        +' library_ok='+str(library_ok)+' chain='+str(chain)
        +' library_error='+str(settled.get('library_error')))
 def resume_leg():
  close_product()
  open_product()
  settled=held.get('settled') or {}
  digest=settled.get('run_digest') if isinstance(settled.get('run_digest'),str) else ''
  ledger_name=digest+'.jsonl' if len(digest)==64 else ''
  rows=read_rows(default_root/ledger_name) if ledger_name else []
  rejected=[row for row in rows if row.get('verdict')=='rejected' and row.get('run_digest')==digest and isinstance(row.get('hypothesis'),str) and row.get('hypothesis')]
  chosen=rejected[0] if rejected else {}
  hypothesis=chosen.get('hypothesis') or ''
  component=chosen.get('component') if isinstance(chosen.get('component'),str) else ''
  round_no=chosen.get('round')
  round_text=str(int(round_no)) if isinstance(round_no,int) or (isinstance(round_no,str) and round_no.isdigit()) else ''
  refusal='refused because component '+component+' with that hypothesis was already rejected in round '+round_text
  harness=settled.get('composition') or ''
  pending=event(fresh(),'refine',{'task':TASK,'response_checks':CHECKS,'refinement_rounds':'2'})
  pending=reply(pending,'Original reply.')
  wire=(pending.get('effect') or {}).get('wire') or ''
  history_at=wire.find('EDIT HISTORY')
  library_at=wire.find('HISTORICAL EXPERIENCE')
  history=wire[history_at:library_at if library_at>history_at else None] if history_at>=0 else ''
  library=wire[library_at:] if library_at>=0 else ''
  # The library section is historical_experience. The ledger section is prompt_document,
  # which only resume_run rebuilds into a fresh process. A lesson may quote a hypothesis;
  # that quote is not the ledger prompt.
  library_read='Adopted harness (historical)' in library and bool(harness) and harness in library
  ledger_rebuilt='Rejected hypotheses (do not propose these again)' in history and bool(hypothesis) and hypothesis in history
  proposed=reply(pending,hypothesis or REJECTED)
  proposed_run=proposed['state'].get('task_run') or {}
  decision=proposed_run.get('round_note') or proposed['state'].get('notice') or ''
  phase=str(proposed_run.get('phase'))
  decision_resumed=bool(hypothesis) and bool(component) and bool(round_text) and refusal in decision and phase!='probe'
  files_alone=bool(ledger_name) and (default_root/ledger_name).is_file() and not (default_root/'edits.jsonl').is_file()
  if library_read and not ledger_rebuilt:
   gap='rrsi::resume_run is not reachable from the JSON-lines route; its only callers are the two unit tests in core/rrsi.rs. Library rows are read into the propose prompt. The ledger prompt is not rebuilt from '+ledger_name+'.'
  elif ledger_rebuilt and not decision_resumed:
   gap='The ledger prompt was rebuilt from '+ledger_name+' but the resumed decision did not refuse the recorded rejection. resume_run returns that ledger and its run digest; the JSON-lines route has not installed them on the run.'
  elif not library_read and not ledger_rebuilt:
   gap='The propose prompt has neither the library section nor a ledger prompt rebuilt from '+ledger_name+'.'
  else:
   gap='The resumed decision did not refuse the hypothesis recorded in '+ledger_name+'.'
  check('files alone rebuild the resumed phase and decision',
        library_read and ledger_rebuilt and decision_resumed and files_alone,
        gap+'\nlibrary_read='+str(library_read)+' ledger_rebuilt='+str(ledger_rebuilt)
        +' decision_resumed='+str(decision_resumed)+' files_alone='+str(files_alone)
        +'\ndecision='+decision+' phase='+phase+' ledger='+ledger_name
        +' hypothesis='+hypothesis[:180]+'\nedit_history='+str(history_at>=0)
        +' historical='+str(library_at>=0)+' files='+str(files_here())
        +'\nprompt_tail='+wire[-700:])
 def tamper_leg():
  library=default_root/'experience.jsonl'
  if not library.is_file():
   check('a tampered library entry is refused and its bytes stay unchanged',False,'experience.jsonl was not written')
   return
  original=library.read_bytes()
  token=b'Candidate rules for the bounded report.'
  at=original.find(token)
  if at<0:
   check('a tampered library entry is refused and its bytes stay unchanged',False,'sealed harness text was not in experience.jsonl')
   return
  tampered=bytearray(original)
  tampered[at]=ord('X') if tampered[at]!=ord('X') else ord('Y')
  library.write_bytes(tampered)
  before=sha256(library)
  close_product()
  open_product()
  pending=event(fresh(),'refine',{'task':TASK,'response_checks':CHECKS})
  pending=reply(pending,'Original reply.')
  wire=(pending.get('effect') or {}).get('wire') or ''
  after=sha256(library)
  refused='library record digest does not match its body; existing history was left unchanged' in wire
  adopted='Adopted harness (historical)' in wire
  check('a tampered library entry is refused and its bytes stay unchanged',
        refused and not adopted and before==after and library.read_bytes()==bytes(tampered),
        'refused='+str(refused)+' adopted_line='+str(adopted)+' before='+before+' after='+after)
 names=['refine entry copies approval_root onto task_run.workspace and records source_digest','a non-refine run carries no workspace and does not attempt the pool','a second round decision changes when the ledger already rejected that component','the digest-named ledger chains each revision to this run and experience.jsonl seals the settled row','files alone rebuild the resumed phase and decision','a tampered library entry is refused and its bytes stay unchanged']
 bodies=[workspace_leg,non_refine_leg,critic_leg,journal_leg,resume_leg,tamper_leg]
 try:
  open_product()
 except Exception:
  traceback.print_exc()
  for name in names:
   if name not in evaluated:check(name,False,'product setup failed\n'+traceback.format_exc()[-1500:])
 else:
  for name,body in zip(names,bodies):leg(name,body)
 finally:
  err_path=room/'product.err'
  close_product()
  err_text=err_path.read_text()[-4000:] if err_path.exists() else ''
  if err_text and problems:print(err_text,flush=True)
 record=dict(checks=evaluated,count=len(evaluated),failed=problems,scope='Owner checks through the product JSON-lines route: refine copies approval_root onto task_run.workspace and records source_digest, the digest-named ledger chains revisions for that run and experience.jsonl seals the settled row, a second round reads a recorded rejection, resume rebuilds the ledger prompt and decision from those files alone, a tampered library entry is refused without a rewrite, and delegate or compact carries no workspace and does not run the pool.')
 if not problems:record['passed']=passed
 out=PROJECT/'output/checks'
 out.mkdir(parents=True,exist_ok=True)
 (out/'rrsi-loop.json').write_text(json.dumps(record,indent=2)+'\n')
 if not problems:print(str(len(passed))+' rrsi-loop checks passed',flush=True)
 print(json.dumps(record,indent=2),flush=True)
 if problems:raise SystemExit(1)
