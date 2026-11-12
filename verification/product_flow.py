"""Product identities and known bits through assembled flow, queries, UI and exact export."""
import base64,copy,hashlib,json,os,pathlib,re,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
def check(name,value):assert value,name;passed.append(name)
def strings(v):
 if isinstance(v,str):yield v
 elif isinstance(v,dict):
  for x in v.values():yield from strings(x)
 elif isinstance(v,list):
  for x in v:yield from strings(x)
def spec_ids(report):
 ids=[]
 if not isinstance(report,str):return None
 for line in report.splitlines():
  if line.startswith('spec ') and ': known:' in line:ids.append(line[5:line.index(': known:')])
 return ids
def statements_for(ids):
 if not ids:return None
 out=[]
 for ident in ids:
  if ident.startswith('edge '):
   rest=ident[5:]
   if ' ' not in rest:return None
   form,spec=rest.split(' ',1)
   if form=='require':out.append(f'require("{spec}");')
   elif form=='import':out.append(f'import "{spec}";')
   elif form=='import()':out.append(f'import("{spec}");')
   elif form=='export' and spec=='local':out.append('export function rebuilt() {}')
   elif form=='export':out.append(f'export {{}} from "{spec}";')
   else:return None
  elif ident.startswith('site '):
   rest=ident[5:]
   if ' ' not in rest:return None
   kind,value=rest.split(' ',1)
   out.append(f'{kind}("{value}", function () {{}});')
  else:return None
 return out
def observed_ids(text):
 if not isinstance(text,str) or 'JavaScript/Electron observation\n' not in text:return None
 ids=[];section=''
 for line in text.split('JavaScript/Electron observation\n',1)[1].splitlines():
  if line.startswith('Module graph: ') and line.endswith(' edges.'):section='graph';continue
  if line.startswith('Unknowns: '):section='';continue
  if line.startswith('String census: ') and line.endswith(' sites.'):section='census';continue
  if line.startswith('Source maps:'):section='';continue
  if section=='graph':
   if ' at byte ' not in line or ' ' not in line.split(' at byte ',1)[1]:return None
   form,rest=line.split(' at byte ',1);tail=rest.split(' ',1)[1]
   if tail=='local form, no external specifier.':ids.append(f'edge {form} local')
   elif tail.startswith('specifier "') and tail.endswith('".'):ids.append('edge '+form+' '+tail[len('specifier "'):-2])
   else:return None
  elif section=='census':
   if ' at byte ' not in line or ' ' not in line.split(' at byte ',1)[1]:return None
   kind,rest=line.split(' at byte ',1);tail=rest.split(' ',1)[1]
   if tail.startswith('literal "') and tail.endswith('".'):ids.append('site '+kind+' '+tail[len('literal "'):-2])
   else:return None
 return ids
with tempfile.TemporaryDirectory(prefix='product-flow-',dir=PROJECT/'output')as tmp:
 root=pathlib.Path(tmp);asm=root/'product.S';elf=root/'product.elf';ndk=pathlib.Path(os.environ['CLIENT_NDK'])/'toolchains/llvm/prebuilt/linux-x86_64/bin'
 asm.write_text('''.text
.global entry
entry:
zero_product: madd x1,x0,xzr,xzr
 mov x2,#42
zero_addend: madd x3,x0,xzr,x2
zero_subtract: msub x4,x0,xzr,x2
 movk x0,#0x1234
 mov x6,#1
unit_product: madd x7,x0,x6,xzr
same_register_cancel: msub x8,x0,x6,x0
 mov x9,#2
shift_product: madd x10,x0,x9,xzr
commuted_product: madd x11,x9,x0,xzr
 movk x12,#0xabcd
low_product: madd x13,x0,x12,xzr
masked_product: and x14,x13,#0xffff
 and x15,x0,#0xffffffff00000000
forced_zero: madd x16,x15,x15,xzr
forced_accumulator: madd x17,x15,x15,x2
 mov x18,#3
 mov x19,#2
 movk x20,#0x1000
partial_addend: madd x21,x18,x19,x20
partial_subtract: msub x22,x18,x19,x20
masked_addend: and x23,x21,#0xffff
 ands xzr,xzr,xzr
flag_product: madd x24,x0,xzr,x2
preserved_flags: csel x25,x2,xzr,eq
 ret
word_entry:
 mov w2,#42
word_zero: madd w1,w0,wzr,wzr
word_accumulator: msub w3,w0,wzr,w2
 movk w0,#0x1234
 mov w6,#2
word_shift: madd w7,w0,w6,wzr
word_mask: and x8,x7,#0xffff
 ret
coordinate_entry:
 adr x1,entry
 mov x2,#1
coordinate_unit: madd x3,x1,x2,xzr
coordinate_accumulator: madd x4,x0,xzr,x1
coordinate_cancel: msub x5,x1,x2,x1
 mov x6,#2
coordinate_shift: madd x7,x1,x6,xzr
 ret
''')
 subprocess.run([str(ndk/'clang'),'--target=aarch64-linux-android26','-nostdlib','-static','-fuse-ld=lld','-Wl,-e,entry','-Wl,--emit-relocs',str(asm),'-o',str(elf)],check=True,capture_output=True,env=dict(os.environ,TMPDIR=str(root)))
 nm=subprocess.check_output([str(ndk/'llvm-nm'),'-n',str(elf)],text=True);symbols={m[3]:int(m[1],16)for line in nm.splitlines()if(m:=re.match(r'([0-9a-fA-F]+)\s+(\w)\s+(\S+)$',line))};disassembly=subprocess.check_output([str(ndk/'llvm-objdump'),'-d',str(elf)],text=True)
 p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def operation(process,value):process.stdin.write(json.dumps(value)+'\n');process.stdin.flush();r=json.loads(process.stdout.readline());assert'value'in r,r;return r['value']
 def op(value):return operation(p,value)
 def event(st,action,inputs=None):return op(dict(op='event',root=str(root),state=st,action=action,inputs=inputs or{}))
 def inspect(process):
  def event(st,action,inputs=None):return operation(process,dict(op='event',root=str(root),state=st,action=action,inputs=inputs or{}))
  d=event(None,'resume');d=event(d['state'],'investigation');d=event(d['state'],'inspect_file',{'path':elf.name});d=event(d['state'],'approve');e=d['effect'];output=operation(process,dict(e,op='tool',root=str(root)));inputs={k:e[k]for k in ['tool_ticket','tool_binding','editor','editor_dialog']if k in e};inputs.update(purpose='local_tool',output=output);return event(d['state'],'tool_result',inputs),json.loads(output)['snapshots'][0]
 def reply(d,text):
  b=json.dumps({'choices':[{'message':{'content':text},'finish_reason':'stop'}]}).encode();wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(b)).encode()+b'\r\n\r\n'+b;return event(d['state'],'network_result',dict(purpose='model',model_ticket=d['state']['model_expected'],data=base64.b64encode(wire).decode()))
 try:
  d,graph=inspect(p);nodes={n['encoding']['address']:n for n in graph['nodes'][1:]};check('independent assembler retains actual product instruction encodings','madd'in disassembly and'msub'in disassembly and'mul'in disassembly)
  for name,register,value in [('zero_product','X1',0),('zero_addend','X3',42),('zero_subtract','X4',42),('same_register_cancel','X8',0),('masked_product','X14',0x1234*0xabcd&0xffff),('forced_zero','X16',0),('forced_accumulator','X17',42),('masked_addend','X23',0x1006),('flag_product','X24',42),('preserved_flags','X25',42),('word_zero','W1',0),('word_accumulator','W3',42),('word_mask','X8',0x2468),('coordinate_cancel','X5',0)]:
   check(name+' retains its determined product or accumulator',register+' <- encoded integer 0x'+format(value,'x')in nodes[symbols[name]]['content'])
  for name,register,bits,known in [('unit_product','X7',0x1234,0xffff),('shift_product','X10',0x2468,0x1ffff),('commuted_product','X11',0x2468,0x1ffff),('low_product','X13',0x1234*0xabcd&0xffff,0xffff),('partial_addend','X21',0x1006,0xffff),('partial_subtract','X22',0xffa,0xffff),('word_shift','W7',0x2468,0xffffffff0001ffff),('coordinate_shift','X7',0,1)]:check(name+' retains each proved product bit',register+' <- partial integer bits0x'+format(bits,'x')+', known mask0x'+format(known,'x')in nodes[symbols[name]]['content'])
  for name in ['coordinate_unit','coordinate_accumulator']:check(name+' preserves source-coordinate identity','encoded source coordinate declared VA 'in nodes[symbols[name]]['content'])
  record=d['state']['investigation_records']['items'][-1];node=nodes[symbols['shift_product']];d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(node['id']));check('native view retains actual partial product bits','X10 <- partial integer bits0x2468, known mask0x1ffff'in'\n'.join(strings(d['ui'])))
  st=d['state'];st['profiles']=[dict(name='Controlled',model='controlled',endpoint='http://controlled.test/model',plain=True,stream=False,protocol='chat')];d=event(st,'send',{'draft':'Read the retained product bits.'});d=reply(d,json.dumps(dict(tool='observations',args=dict(id=record['id'],snapshot=0,node=node['id']))));d=event(d['state'],'approve');check('ordinary model query receives the actual product bits',d['effect']is not None and'X10 <- partial integer bits0x2468, known mask0x1ffff'in d['effect']['wire']);d=reply(d,'Product bits received.')
  d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(node['id']));d=event(d['state'],'reconstruction_new');d=event(d['state'],'reconstruction_prepare',dict(record=record['id'],snapshot='0',node=str(node['id']),mode='representation',output='product-result.txt'));d=event(d['state'],'approve');e=d['effect'];out=op(dict(e,op='tool',root=str(root)));check('reviewed export retains the exact product representation',(root/'product-result.txt').read_bytes()==node['content'].encode()and'read back exactly'in out)
  old=subprocess.Popen([str(PROJECT/'output/published-controller'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
  try:prior,prior_graph=inspect(old)
  finally:old.stdin.close();old.wait(timeout=10)
  prior_node=next(n for n in prior_graph['nodes'][1:]if n['encoding']['address']==symbols['shift_product']);check('actual published capture retains the same product encoding',prior_node['encoding']['bits']==node['encoding']['bits']and prior_node['encoding']['mnemonic']=='MADD');st=event(None,'resume')['state'];st['investigation_records']=copy.deepcopy(prior['state']['investigation_records']);prior_record=st['investigation_records']['items'][-1];d=event(st,'investigation_record:'+prior_record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(prior_node['id']));check('current view preserves the actual prior product capture',prior_node['content'].strip()in'\n'.join(strings(d['ui'])))
  js=root/'feature.js';js.write_text('const {app} = require("electron");\nconst helper = require("./helper.js");\nipcMain.on("window:open", function () {});\nrouter.get("/status", function () {});\nexport function openWindow() {}\n');(root/'held').mkdir();(root/'held'/'wide.js').write_bytes(b'const x = require("electron");\n'+b'x'*5000);(root/'oversized.js').write_bytes(b'x'*(131072+1))
  room=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
  try:
   def revent(st,action,inputs=None):return operation(room,dict(op='event',root=str(root),state=st,action=action,inputs=inputs or{}))
   def rraw(value):room.stdin.write(json.dumps(value)+'\n');room.stdin.flush();return json.loads(room.stdout.readline())
   def rsettle(d,e,out):return revent(d['state'],'tool_result',dict({k:e[k]for k in ['tool_ticket','tool_binding','editor','editor_dialog']if k in e},purpose='local_tool',output=out))
   d=revent(None,'resume');d=revent(d['state'],'investigation');d=revent(d['state'],'inspect_file',{'path':js.name,'content':'complete_digest'});d=revent(d['state'],'approve');e=d['effect'];output=operation(room,dict(e,op='tool',root=str(root)));graph=json.loads(output)['snapshots'][0];d=rsettle(d,e,output);record=d['state']['investigation_records']['items'][-1];check('clean-room subject retains the complete JavaScript file observation',graph['kind']=='complete_file' and graph['coverage']=='Complete' and graph['nodes'][0]['kind']=='File' and graph['nodes'][0]['sample']['complete_file'] is True and record['args']['path']==js.name and record['args']['content']=='complete_digest')
   d=revent(d['state'],'investigation_record:'+record['id']);d=revent(d['state'],'investigation_graph');d=revent(d['state'],'investigation_graph_node:0');d=revent(d['state'],'reconstruction_new');d=revent(d['state'],'reconstruction_prepare',dict(record=record['id'],snapshot='0',node='0',mode='clean_room',output='feature-rebuild.js'));d=revent(d['state'],'approve');e=d['effect'];out=operation(room,dict(e,op='tool',root=str(root)));generated=root/'feature-rebuild.js';check('clean-room reconstruction reports the spec, the verification and the generated file','Spec items:' in out and 'Verification matched' in out and 'Generated file: feature-rebuild.js' in out and 'matched edge require electron' in out and 'matched edge require ./helper.js' in out and 'matched site ipcMain.on window:open' in out and 'matched site router.get /status' in out and 'matched edge export local' in out);body=generated.read_bytes();sample=graph['nodes'][0]['sample'];subject=sample['sha256'];digest=hashlib.sha256(body).hexdigest();ids=spec_ids(out);named=statements_for(ids);lines=body.decode().splitlines() if body else [];check('clean-room generated file exists and its bytes differ from the reference',generated.is_file() and len(body)>0 and digest!=subject and sample.get('complete_file') is True and sample.get('method')=='SHA-256 of every byte read from the complete held regular file' and f'Output SHA-256: {digest}' in out and f'Reference SHA-256 {subject}' in out and named is not None and len(named)==len(ids) and all(line in lines for line in named));kept=body;d=rsettle(d,e,out)
   d=revent(d['state'],'investigation');d=revent(d['state'],'inspect_file',{'path':generated.name});d=revent(d['state'],'approve');e=d['effect'];seen=operation(room,dict(e,op='tool',root=str(root)));found=observed_ids(seen if isinstance(seen,str) else '');reported=re.search(r'Spec items: (\d+)\. Generated file: feature-rebuild\.js',out);check('clean-room re-inspection of the generated file matches the reported spec items',reported is not None and found is not None and int(reported.group(1))==len(ids)==len(found) and ids==found);d=rsettle(d,e,seen)
   d=revent(d['state'],'investigation_record:'+record['id']);d=revent(d['state'],'investigation_graph');d=revent(d['state'],'investigation_graph_node:0');d=revent(d['state'],'reconstruction_new');d=revent(d['state'],'reconstruction_prepare',dict(record=record['id'],snapshot='0',node='0',mode='clean_room',output='feature-rebuild.js'));d=revent(d['state'],'approve');e=d['effect'];answer=rraw(dict(e,op='tool',root=str(root)));check('exclusive create refuses an output name that already exists',answer.get('error')=='Cannot create new streaming output: File exists (os error 17)' and generated.read_bytes()==kept);d=revent(d['state'],'failure',dict(purpose='local_tool',tool_ticket=e.get('tool_ticket'),tool_binding=e.get('tool_binding'),error=answer.get('error')))
   d=revent(d['state'],'investigation');d=revent(d['state'],'inspect_file',{'path':'held'});d=revent(d['state'],'approve');e=d['effect'];output=operation(room,dict(e,op='tool',root=str(root)));nodes=json.loads(output)['snapshots'][0]['nodes'];file_node=next(n for n in nodes if n['kind']=='File');d=rsettle(d,e,output);partial=d['state']['investigation_records']['items'][-1];d=revent(d['state'],'investigation_record:'+partial['id']);d=revent(d['state'],'investigation_graph');d=revent(d['state'],'investigation_graph_node:'+str(file_node['id']));d=revent(d['state'],'reconstruction_new');d=revent(d['state'],'reconstruction_prepare',dict(record=partial['id'],snapshot='0',node=str(file_node['id']),mode='clean_room',output='wide-rebuild.js'));refused=d['state'].get('notice');prepared_effect=d.get('effect');prepared_pending=d['state'].get('pending');d=revent(d['state'],'approve');check('clean-room reconstruction refuses an observation that is not a complete file',file_node['kind']=='File' and file_node['sample']['complete_file'] is False and partial['args']['path']=='held' and refused=='Clean-room reconstruction requires a complete unchanged observed file; missing or prefix content is not reconstructed' and prepared_effect is None and prepared_pending is None and d.get('effect') is None and d['state'].get('notice')=='No current operation is awaiting approval' and not (root/'wide-rebuild.js').exists())
   d=revent(d['state'],'investigation');d=revent(d['state'],'inspect_file',{'path':'oversized.js','content':'complete_digest'});d=revent(d['state'],'approve');e=d['effect'];output=operation(room,dict(e,op='tool',root=str(root)));cap_graph=json.loads(output)['snapshots'][0];d=rsettle(d,e,output);cap=d['state']['investigation_records']['items'][-1];d=revent(d['state'],'investigation_record:'+cap['id']);d=revent(d['state'],'investigation_graph');d=revent(d['state'],'investigation_graph_node:0');d=revent(d['state'],'reconstruction_new');d=revent(d['state'],'reconstruction_prepare',dict(record=cap['id'],snapshot='0',node='0',mode='clean_room',output='oversized-rebuild.js'));d=revent(d['state'],'approve');e=d['effect'];answer=rraw(dict(e,op='tool',root=str(root)));check('clean-room reconstruction refuses a subject over the subject-byte cap',cap_graph.get('sampled_bytes')==131073 and cap['args']['path']=='oversized.js' and cap['args']['content']=='complete_digest' and answer.get('error')=='Refusal: clean-room subject-byte cap. 131073 exceeds 131072. No output was written.' and not (root/'oversized-rebuild.js').exists())
  finally:room.stdin.close();room.wait(timeout=10)
 finally:p.stdin.close();p.wait(timeout=10)
result=dict(passed=passed,scope='Owner current shared-core checks with original LLVM-assembled AArch64 product instruction encodings, independent disassembly, retained observations, native UI descriptions, controlled provider query, exact export and genuine immediately-published core capture compatibility. Connected clean-room reconstruction of one retained complete JavaScript file, the exclusive-create refusal, and named refusals for an incomplete file observation and a subject over the 131072-byte cap. The generated file is a non-empty skeleton whose digest differs from the retained subject observation and whose statements are the reported spec items. A fresh inspect of that file must observe the same spec items. An incomplete observation is refused by notice before any tool effect, and the output file is absent.');(PROJECT/'output/checks/product-flow.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
