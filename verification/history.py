"""Isolated shared-core history state checks; no real conversation data."""
import copy,json,pathlib,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1]
(PROJECT/'output').mkdir(exist_ok=True)
testdir=tempfile.TemporaryDirectory(prefix='history-checks-',dir=PROJECT/'output')
p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
passed=[]
def event(st,a):
 p.stdin.write(json.dumps(dict(op='event',state=st,action=a,inputs={},root=testdir.name))+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert 'value' in r,r;return r['value']
try:
 st=event(None,'resume')['state'];st['dialog']=None;st['trash']=[dict(id='deleted-one',title='Fixture one',messages=[dict(role='user',content='First')]),dict(id='deleted-two',title='Fixture two',messages=[])]
 refused=event(st,'trash_empty');assert refused['state']['trash']==st['trash'];passed.append('empty Trash requires current review')
 opened=event(st,'trash');d=event(opened['state'],'trash_empty_review');assert d['state']['dialog']=='trash_empty_review' and len(d['state']['trash_review'])==2
 kept=event(d['state'],'trash');assert kept['state']['trash']==st['trash'];passed.append('keeping reviewed conversations preserves all items')
 changed=copy.deepcopy(d['state']);changed['trash'][0]['messages'][0]['content']='Changed after review'
 refused=event(changed,'trash_empty');assert refused['state']['trash']==changed['trash'] and 'changed after review' in refused['state']['notice'];passed.append('changed message invalidates exact reviewed snapshot')
 added=copy.deepcopy(d['state']);added['trash'].append(dict(id='deleted-three',title='New fixture',messages=[]))
 refused=event(added,'trash_empty');assert refused['state']['trash']==added['trash'];passed.append('newly trashed conversation invalidates old confirmation')
 ambiguous=copy.deepcopy(st);ambiguous['trash'][1]['id']='deleted-one'
 opened=event(ambiguous,'trash');refused=event(opened['state'],'trash_empty_review');assert refused['state']['trash']==ambiguous['trash'] and refused['state']['dialog']!='trash_empty_review';passed.append('duplicate identities cannot be purged as a snapshot')
 emptied=event(d['state'],'trash_empty');assert emptied['state']['trash']==[] and emptied['state']['chats']==st['chats'] and emptied['state']['active']==st['active'];passed.append('confirmed exact Trash purge preserves active history')
 result=PROJECT/'output/checks/history-results.json';result.parent.mkdir(exist_ok=True);result.write_text(json.dumps({'passed':passed,'scope':'Isolated native event state transitions; no Android encrypted persistence or real data deletion'},indent=2));print(len(passed),'history checks passed')
finally:p.stdin.close();p.wait();testdir.cleanup()
