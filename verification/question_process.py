import tempfile
"""Exercise the manual built-in question/design workflow without a model."""
import copy,json,pathlib,subprocess
PROJECT=pathlib.Path(__file__).resolve().parents[1]
(PROJECT/'output').mkdir(exist_ok=True)
testdir=tempfile.TemporaryDirectory(prefix='checks-',dir=PROJECT/'output')
RESULTS=PROJECT/'output'/'checks';RESULTS.mkdir(exist_ok=True)
root=pathlib.Path(testdir.name)
p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
passed=[]
def op(v):
 p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
def event(st,a,i=None):
 r=op(dict(op='event',state=st,action=a,inputs=i or {},root=str(root)));assert 'value' in r,r;return r['value']
def edit(d,open_action,save_action,inputs):
 d=event(d['state'],open_action);d=event(d['state'],save_action,inputs);assert d['state']['dialog']=='inquiry_view',d;return d
try:
 d=event(None,'resume');d=event(d['state'],'inquiry_new');d=event(d['state'],'inquiry_start',{'question':'Design an original file index from observed requirements.'});assert d['effect'] is None;passed.append('manual question without model or external MCP server')
 d=edit(d,'inquiry_plan','inquiry_plan_save',{'questions':'Which names must be indexed?','constraints':'Original implementation','requirements':'Preserve exact filenames'})
 d=edit(d,'inquiry_note','inquiry_note_save',{'kind':'unknown','text':'Filename encoding needs observation.','citations':'','resolves':''})
 d=edit(d,'inquiry_note','inquiry_note_save',{'kind':'design','text':'Use original code to preserve observed filename bytes.','citations':'','resolves':''})
 passed.append('manual plan, unresolved finding and original design')
 d=edit(d,'inquiry_assessment','inquiry_assessment_save',{'item':'questions','index':'1','status':'partial','explanation':'The requirement is recorded; filename observations remain missing.','citations':'','checks':''})
 c=d['state']['inquiry_records']['items'][-1];assert c['assessments'][-1]['status']=='partial';old_plan=c['plan'];passed.append('partial item assessment retains missing evidence')
 d=edit(d,'inquiry_plan','inquiry_plan_save',{'questions':'Which names and encodings must be indexed?','constraints':'Original implementation','requirements':'Preserve exact filenames'})
 c=d['state']['inquiry_records']['items'][-1];assert len(c['assessments'])==1 and any(n.get('plan')==old_plan for n in c['notes']);passed.append('plan revision preserves previous assessment and exact prior plan')
 d=event(d['state'],'inquiry_section:review');view=json.dumps(d['ui']);assert 'unreviewed' in view,view;passed.append('revised plan does not inherit answered or partial status')
 d=event(d['state'],'inquiry_export');d=event(d['state'],'inquiry_export_review',{'output':'question.json'});assert d['state']['dialog']=='approval';d=event(d['state'],'approve');e=d['effect'];assert e['kind']=='tool' and e['tool']=='write',d
 r=op(dict(e,op='tool',root=str(root)));assert 'value' in r,r
 exported=json.loads((root/'question.json').read_text());assert exported['type']=='question_process';c=exported['data'];assert c['question'].startswith('Design an original') and c['item_review']['unreviewed']==2 and c['item_review']['authored_answered']==0;assert c['evidence']==[];passed.append('reviewed export writes actual journal, unresolved state and original design')
 i={k:v for k,v in e.items() if k in ['purpose','tool_ticket','tool_binding']};i['output']=r['value'];d=event(d['state'],'tool_result',i)
 d=event(d['state'],'inquiry_export');d=event(d['state'],'inquiry_export_review',{'output':'cancelled.json'});d=event(d['state'],'approve');e=d['effect'];d=event(d['state'],'cancel');r=op(dict(e,op='tool',root=str(root)));assert 'error' in r and not (root/'cancelled.json').exists(),r;passed.append('cancelled export cannot execute a stale approved write')
 d=event(d['state'],'inquiry_export');d=event(d['state'],'inquiry_export_review',{'output':'design-brief.md','format':'design_brief'});assert d['state']['dialog']=='approval'and not(root/'design-brief.md').exists();assert d['state']['pending']['inquiry_export']['format']=='design_brief';passed.append('design brief has explicit contents and ordinary write review')
 d=event(d['state'],'approve');e=d['effect'];altered=copy.deepcopy(e);altered['inquiry_export']['format']='journal';refused=op(dict(altered,op='tool',root=str(root)));assert'error'in refused and not(root/'design-brief.md').exists();passed.append('changed export format cannot borrow an approved write')
 r=op(dict(e,op='tool',root=str(root)));assert'value'in r,r;brief=(root/'design-brief.md').read_text();assert brief.startswith('# Design brief')and'Use original code to preserve observed filename bytes.'in brief and'## Returned-evidence references'in brief and'Unknown entries without an authored resolution: 1.'in brief,brief;passed.append('actual no-model design brief preserves authored design and unresolved work')
 values={k:v for k,v in e.items()if k in['purpose','tool_ticket','tool_binding']};values['output']=r['value'];d=event(d['state'],'tool_result',values)
 d=event(d['state'],'inquiry_export');bad=event(d['state'],'inquiry_export_review',{'output':'bad-format.md','format':'unknown'});assert bad['effect']is None and bad['state'].get('notice_error')and not(root/'bad-format.md').exists();passed.append('unknown brief mode refuses only that export')
 d=event(bad['state'],'inquiry_export');d=event(d['state'],'inquiry_export_review',{'output':'cancelled-brief.md','format':'design_brief'});d=event(d['state'],'approve');e=d['effect'];cancelled=event(d['state'],'cancel');r=op(dict(e,op='tool',root=str(root)));assert'error'in r and not(root/'cancelled-brief.md').exists();passed.append('cancelled design brief cannot execute a stale approved write');d=cancelled
 def confirmation(value):
  if isinstance(value,dict):
   if value.get('event','').startswith('inquiry_delete_confirm:'):return value['event']
   for child in value.values():
    found=confirmation(child)
    if found:return found
  elif isinstance(value,list):
   for child in value:
    found=confirmation(child)
    if found:return found
  return None
 d=event(d['state'],'inquiry_new');d=event(d['state'],'inquiry_start',{'question':'Question A to retain'});a=d['state']['inquiry_selection'];d=event(d['state'],'inquiry_delete');old_a=confirmation(d['ui']);assert old_a and 'Question A to retain' in json.dumps(d['ui']);passed.append('deletion review displays its exact question and captures its identity')
 d=event(d['state'],'inquiry_new');d=event(d['state'],'inquiry_start',{'question':'Question B to review'});b=d['state']['inquiry_selection'];d=event(d['state'],'inquiry_delete');fresh_b=confirmation(d['ui']);before=d['state']['inquiry_records']
 stale=event(d['state'],old_a);assert stale['state']['inquiry_records']==before and 'no journal was deleted' in stale['state']['notice'];passed.append('stale Question A confirmation cannot delete Question B')
 legacy=event(stale['state'],'inquiry_delete_confirm');assert legacy['state']['inquiry_records']==before;passed.append('legacy unbound deletion callback cannot delete a journal')
 kept=event(legacy['state'],'inquiry');late=event(kept['state'],fresh_b);assert late['state']['inquiry_records']==before;passed.append('leaving deletion review prevents its captured callback from deleting')
 d=event(late['state'],'inquiry_select:'+b);d=event(d['state'],'inquiry_delete');old_b=confirmation(d['ui'])
 d=edit(d,'inquiry_plan','inquiry_plan_save',{'questions':'A changed question plan','constraints':'','requirements':''});d=event(d['state'],'inquiry_delete');fresh_b=confirmation(d['ui']);before=d['state']['inquiry_records']
 changed=event(d['state'],old_b);assert changed['state']['inquiry_records']==before and fresh_b!=old_b;passed.append('changed journal requires a fresh exact-content deletion confirmation')
 chats=[{k:v for k,v in c.items() if k!='inquiry_id'} for c in changed['state']['chats']];d=event(changed['state'],fresh_b);ids=[c['id'] for c in d['state']['inquiry_records']['items']];remaining_chats=[{k:v for k,v in c.items() if k!='inquiry_id'} for c in d['state']['chats']];assert a in ids and b not in ids and remaining_chats==chats and all(c.get('inquiry_id')!=b for c in d['state']['chats']) and (root/'question.json').exists();passed.append('fresh confirmation deletes only its reviewed journal and preserves chat and exported files')
 repeated=event(d['state'],fresh_b);assert repeated['state']['inquiry_records']==d['state']['inquiry_records'];passed.append('completed deletion cannot replay against another question')
 (RESULTS/'process-results.json').write_text(json.dumps({'passed':passed,'scope':'Native manual controls and real guarded file worker; no model prose validation'},indent=2));print(len(passed),'manual question-process checks passed')
finally:
 p.stdin.close();p.wait();testdir.cleanup()
