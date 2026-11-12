"""Original assembled carry arithmetic through inspection, query, view and export."""
import base64,json,os,pathlib,re,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
def check(name,value):assert value,name;passed.append(name)
def strings(v):
 if isinstance(v,str):yield v
 elif isinstance(v,dict):
  for x in v.values():yield from strings(x)
 elif isinstance(v,list):
  for x in v:yield from strings(x)
with tempfile.TemporaryDirectory(prefix='carry-flow-',dir=PROJECT/'output')as tmp:
 root=pathlib.Path(tmp);asm=root/'carry.S';elf=root/'carry.elf';ndk=pathlib.Path(os.environ['CLIENT_NDK'])/'toolchains/llvm/prebuilt/linux-x86_64/bin'
 asm.write_text('''.text
.global entry,adc64,sbc64,adcs64,sbcs64,selected,adc32,sbc32,adcs32,sbcs32,unknown_adc,masked,coordinate_add,coordinate_sub,identity_sub,identity_flags,identity_sbc,identity_bic,identity_orn,identity_eon,identity_selected,identity_bics
entry:
 mov x0,#-1
 adds x1,x0,#1
 mov x2,#41
adc64: adc x3,x2,xzr
sbc64: sbc x4,x3,x2
adcs64: adcs x5,x0,xzr
sbcs64: sbcs x6,x3,x2
selected: csel x7,x3,x2,hi
 mov w8,#42
 subs w9,w8,#43
adc32: adc w10,w8,wzr
sbc32: sbc w11,w8,wzr
adcs32: adcs w12,w9,wzr
sbcs32: sbcs w13,w8,wzr
 ret
unknown_entry:
 mov x14,#40
unknown_adc: adc x15,x14,xzr
masked: and x16,x15,#0xfffffffffffffffe
 ret
coordinate_entry:
 adr x17,entry
 subs xzr,xzr,xzr
coordinate_add: adc x18,x17,xzr
coordinate_sub: sbc x19,x17,xzr
 ret
identity_entry:
identity_sub: sub x20,x21,x21
identity_flags: subs x22,x21,x21
identity_sbc: sbc x23,x21,x21
identity_bic: bic x24,x21,x21
identity_orn: orn x25,x21,x21
identity_eon: eon x26,x21,x21
identity_selected: csel x27,x25,x24,eq
identity_bics: bics x28,x21,x21
 ret
''')
 subprocess.run([str(ndk/'clang'),'--target=aarch64-linux-android26','-nostdlib','-static','-fuse-ld=lld','-Wl,-e,entry','-Wl,--emit-relocs',str(asm),'-o',str(elf)],check=True,capture_output=True,env=dict(os.environ,TMPDIR=str(root)))
 nm=subprocess.check_output([str(ndk/'llvm-nm'),'-n',str(elf)],text=True);symbols={m[3]:int(m[1],16)for line in nm.splitlines()if(m:=re.match(r'([0-9a-fA-F]+)\s+(\w)\s+(\S+)$',line))};disassembly=subprocess.check_output([str(ndk/'llvm-objdump'),'-d',str(elf)],text=True)
 p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def op(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert'value'in r,r;return r['value']
 def event(st,a,i=None):return op(dict(op='event',root=str(root),state=st,action=a,inputs=i or{}))
 def reply(d,text):
  b=json.dumps({'choices':[{'message':{'content':text},'finish_reason':'stop'}]}).encode();wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(b)).encode()+b'\r\n\r\n'+b;return event(d['state'],'network_result',dict(purpose='model',model_ticket=d['state']['model_expected'],data=base64.b64encode(wire).decode()))
 try:
  d=event(None,'resume');d=event(d['state'],'investigation');d=event(d['state'],'inspect_file',{'path':elf.name});d=event(d['state'],'approve');e=d['effect'];output=op(dict(e,op='tool',root=str(root)));graph=json.loads(output)['snapshots'][0];nodes={n['encoding']['address']:n for n in graph['nodes'][1:]}
  for name,mnemonic,reg,value in [('adc64','ADC','X3','0x2a'),('sbc64','SBC','X4','0x1'),('adcs64','ADCS','X5','0x0'),('sbcs64','SBCS','X6','0x1'),('adc32','ADC','W10','0x2a'),('sbc32','SBC','W11','0x29'),('adcs32','ADCS','W12','0xffffffff'),('sbcs32','SBCS','W13','0x29')]:
   n=nodes[symbols[name]];check(name+' matches independent encoding and carry result',n['encoding']['mnemonic']==mnemonic and mnemonic.lower()in disassembly and reg+' <- encoded integer '+value in n['content'])
  check('carry-updated flags drive the following conditional select','X7 <- encoded integer 0x2a'in nodes[symbols['selected']]['content'])
  check('unknown carry retains common result bits','X15 <- partial integer bits0x28, known mask0xfffffffffffffffe'in nodes[symbols['unknown_adc']]['content'])
  check('following mask derives a constant from unknown carry','X16 <- encoded integer 0x28'in nodes[symbols['masked']]['content'])
  check('carry arithmetic preserves coordinate provenance',all('ELF declared VA'in nodes[symbols[name]]['content']and'encoded integer'not in nodes[symbols[name]]['content']for name in ['coordinate_add','coordinate_sub']))
  for name,reg,value in [('identity_sub','X20','0x0'),('identity_flags','X22','0x0'),('identity_sbc','X23','0x0'),('identity_bic','X24','0x0'),('identity_orn','X25','0xffffffffffffffff'),('identity_eon','X26','0xffffffffffffffff'),('identity_selected','X27','0xffffffffffffffff'),('identity_bics','X28','0x0')]:check(name+' retains its register identity and flags',reg+' <- encoded integer '+value in nodes[symbols[name]]['content'])
  i={k:e[k]for k in ['tool_ticket','tool_binding','editor','editor_dialog']if k in e};d=event(d['state'],'tool_result',dict(i,purpose='local_tool',output=output));record=d['state']['investigation_records']['items'][-1];node=nodes[symbols['adc64']];d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(node['id']));check('native view retains actual carry result','X3 <- encoded integer 0x2a'in'\n'.join(strings(d['ui'])))
  st=d['state'];st['profiles']=[dict(name='Controlled',model='controlled',endpoint='http://controlled.test/model',plain=True,stream=False,protocol='chat')];d=event(st,'send',{'draft':'Read the actual retained carry arithmetic.'});d=reply(d,json.dumps(dict(tool='observations',args=dict(id=record['id'],snapshot=0,node=node['id']))));d=event(d['state'],'approve');check('ordinary model query receives actual carry result',d['effect']is not None and'X3 <- encoded integer 0x2a'in d['effect']['wire']);d=reply(d,'Actual carry result received.')
  d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(node['id']));d=event(d['state'],'reconstruction_new');d=event(d['state'],'reconstruction_prepare',dict(record=record['id'],snapshot='0',node=str(node['id']),mode='representation',output='carry-result.txt'));d=event(d['state'],'approve');e=d['effect'];out=op(dict(e,op='tool',root=str(root)));check('guarded export retains exact carry representation',(root/'carry-result.txt').read_bytes()==node['content'].encode()and'read back exactly'in out)
 finally:p.stdin.close();p.wait(timeout=10)
result=dict(passed=passed,scope='Owner shared-core checks using original LLVM-assembled AArch64 carry instructions, actual retained observations, UI descriptions, controlled provider query and exact file export.');(PROJECT/'output/checks/carry-flow.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
