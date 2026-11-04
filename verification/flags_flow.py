"""Individually known flags through assembled flow, queries, UI and exact export."""
import base64,copy,json,os,pathlib,re,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
def check(name,value):assert value,name;passed.append(name)
def strings(v):
 if isinstance(v,str):yield v
 elif isinstance(v,dict):
  for x in v.values():yield from strings(x)
 elif isinstance(v,list):
  for x in v:yield from strings(x)
with tempfile.TemporaryDirectory(prefix='flags-flow-',dir=PROJECT/'output')as tmp:
 root=pathlib.Path(tmp);asm=root/'flags.S';elf=root/'flags.elf';ndk=pathlib.Path(os.environ['CLIENT_NDK'])/'toolchains/llvm/prebuilt/linux-x86_64/bin'
 asm.write_text('''.text
.global entry
entry:
 mov x4,#42
 mov x5,#7
 ands xzr,x0,#0xff
flag_adc: adc x1,xzr,xzr
flag_sbc: sbc x2,xzr,xzr
flag_cc: csel x6,x4,x5,cc
flag_vc: csel x7,x4,x5,vc
flag_pl: csel x8,x4,x5,pl
flag_eq: csel x9,x4,x5,eq
flag_hi: csel x10,x4,x5,hi
flag_ls: csel x11,x4,x5,ls
 ret
unknown_sign_entry:
 mov x4,#42
 mov x5,#7
 ands xzr,x0,x0
unknown_sign_vc: csel x12,x4,x5,vc
unknown_sign_pl: csel x13,x4,x5,pl
 ret
arithmetic_entry:
 mov x4,#42
 mov x5,#7
 and x12,x0,#0xfffffffffffffff0
 adds x13,x12,#1
arithmetic_adc: adc x14,xzr,xzr
arithmetic_ne: csel x15,x4,x5,ne
arithmetic_eq: csel x16,x4,x5,eq
arithmetic_vc: csel x17,x4,x5,vc
arithmetic_pl: csel x18,x4,x5,pl
 ret
word_entry:
 mov w4,#42
 mov w5,#7
 ands wzr,w0,#0xff
word_adc: adc w1,wzr,wzr
word_cc: csel w2,w4,w5,cc
 ret
join_entry:
 cbz x0,join_left
 mov x4,#42
 mov x5,#7
 mov x1,#0
 ands xzr,x1,x1
 b join_target
join_left:
 mov x4,#42
 mov x5,#7
 mov x1,#1
 ands xzr,x1,x1
 b join_target
join_target:
join_adc: adc x2,xzr,xzr
join_vc: csel x3,x4,x5,vc
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
  d,graph=inspect(p);nodes={n['encoding']['address']:n for n in graph['nodes'][1:]};check('independent assembler retains actual flag consumers','adc'in disassembly and'csel'in disassembly and'tst'in disassembly)
  for name,register,value in [('flag_adc','X1',0),('flag_sbc','X2',0xffffffffffffffff),('flag_cc','X6',42),('flag_vc','X7',42),('flag_pl','X8',42),('flag_hi','X10',7),('flag_ls','X11',42),('unknown_sign_vc','X12',42),('arithmetic_adc','X14',0),('arithmetic_ne','X15',42),('arithmetic_eq','X16',7),('arithmetic_vc','X17',42),('word_adc','W1',0),('word_cc','W2',42),('join_adc','X2',0),('join_vc','X3',42)]:
   check(name+' uses the determining known flags',register+' <- encoded integer 0x'+format(value,'x')in nodes[symbols[name]]['content'])
  for name,register in [('flag_eq','X9'),('unknown_sign_pl','X13'),('arithmetic_pl','X18')]:check(name+' retains its actual unresolved condition',register+' <- partial integer bits0x2, known mask0xffffffffffffffd2'in nodes[symbols[name]]['content'])
  check('logical carry is independently available to ADC','incoming carry 0'in nodes[symbols['flag_adc']]['content']);check('arithmetic carry survives an unknown result sign','incoming carry 0'in nodes[symbols['arithmetic_adc']]['content']);check('joined flag display labels its unknown Z directly','NZCV N=0 Z=unknown C=0 V=0'in nodes[symbols['join_adc']]['content'])
  record=d['state']['investigation_records']['items'][-1];node=nodes[symbols['flag_adc']];d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(node['id']));check('native view retains the actual known-carry result','incoming carry 0; X1 <- encoded integer 0x0'in'\n'.join(strings(d['ui'])))
  st=d['state'];st['profiles']=[dict(name='Controlled',model='controlled',endpoint='http://controlled.test/model',plain=True,stream=False,protocol='chat')];d=event(st,'send',{'draft':'Read the retained independently known carry.'});d=reply(d,json.dumps(dict(tool='observations',args=dict(id=record['id'],snapshot=0,node=node['id']))));d=event(d['state'],'approve');check('ordinary model query receives the actual known-carry result',d['effect']is not None and'incoming carry 0; X1 <- encoded integer 0x0'in d['effect']['wire']);d=reply(d,'Known-carry result received.')
  d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(node['id']));d=event(d['state'],'reconstruction_new');d=event(d['state'],'reconstruction_prepare',dict(record=record['id'],snapshot='0',node=str(node['id']),mode='representation',output='flags-result.txt'));d=event(d['state'],'approve');e=d['effect'];out=op(dict(e,op='tool',root=str(root)));check('reviewed export retains the exact flag-derived representation',(root/'flags-result.txt').read_bytes()==node['content'].encode()and'read back exactly'in out)
  old=subprocess.Popen([str(PROJECT/'output/published-controller'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
  try:prior,prior_graph=inspect(old)
  finally:old.stdin.close();old.wait(timeout=10)
  prior_node=next(n for n in prior_graph['nodes'][1:]if n['encoding']['address']==symbols['flag_adc']);check('actual published capture retains the same flag-consumer encoding',prior_node['encoding']['bits']==node['encoding']['bits']and prior_node['encoding']['mnemonic']=='ADC');st=event(None,'resume')['state'];st['investigation_records']=copy.deepcopy(prior['state']['investigation_records']);prior_record=st['investigation_records']['items'][-1];d=event(st,'investigation_record:'+prior_record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(prior_node['id']));check('current view preserves the actual prior flag capture',prior_node['content'].strip()in'\n'.join(strings(d['ui'])))
 finally:p.stdin.close();p.wait(timeout=10)
result=dict(passed=passed,scope='Owner current shared-core checks with original LLVM-assembled AArch64 flag-producing and consuming instructions, independent disassembly, retained observations, native UI descriptions, controlled provider query, exact export and genuine immediately-published core capture compatibility.');(PROJECT/'output/checks/flags-flow.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
