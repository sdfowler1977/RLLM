"""Request history -> selected statistics, saved range and live continuation."""
import base64,copy,json,pathlib,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
def check(name,value):
    assert value,name
    passed.append(name)
with tempfile.TemporaryDirectory(prefix='diagnostics-',dir=PROJECT/'output') as tmp:
    root=pathlib.Path(tmp);(root/'input.txt').write_text('Local work after range validation.')
    p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def op(value):
        p.stdin.write(json.dumps(value)+'\n');p.stdin.flush();out=json.loads(p.stdout.readline());assert 'value'in out,out;return out['value']
    def event(st,action,inputs=None):return op(dict(op='event',root=str(root),state=st,action=action,inputs=inputs or{}))
    def finish(d,content,ms,first=None):
        body=json.dumps({'choices':[{'message':{'content':content},'finish_reason':'stop'}]}).encode();wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(body)).encode()+b'\r\n\r\n'+body
        inputs=dict(purpose='model',model_ticket=d['state']['model_expected'],data=base64.b64encode(wire).decode(),elapsed_ms=ms)
        if first is not None:inputs['first_text_ms']=first
        return event(d['state'],'network_result',inputs)
    try:
        d=event(None,'resume');check('startup keeps phone permissions under explicit Setup control',d['effect']is None and d['ui'].get('modal')is None);st=d['state'];st['profiles']=[dict(name='First',model='first',endpoint='http://first.test/v1/chat/completions',protocol='chat',plain=True,stream=False),dict(name='Second',model='second',endpoint='http://second.test/v1/chat/completions',protocol='chat',plain=True,stream=False)]
        st['agents']=[dict(name='General',tools='list,read')];st['phone_access_requested']=True
        d=event(st,'send',dict(draft='First measurement'));d=finish(d,'a'*400,100,20)
        d=event(d['state'],'send',dict(draft='Second measurement'));d=finish(d,'b'*400,900,180)
        d=event(d['state'],'monitor:window:2');g=d['ui']['monitor']['gauges']
        check('actual completed requests produce weighted token average',g[0]['value']=='≈200.0 tok/s')
        check('response and first-text gauges use selected means',g[1]['value']=='500 ms'and g[2]['value']=='100 ms')
        check('selected gauge sample count is visible','2 completed runs'in g[0]['caption'])
        d=event(d['state'],'monitor:range');check('exact count control is exposed',d['ui']['modal']['title']=='Averaging range')
        d=event(d['state'],'capture',dict(runs='7'));d=event(d['state'],'resume');check('unapplied count survives form capture and recovery',d['state']['dialog']=='monitor_range'and next(f['value']for f in d['ui']['modal']['fields']if f['key']=='runs')=='7')
        snapshot=copy.deepcopy(d['state']);d=event(snapshot,'monitor:range_save',dict(runs='0'))
        check('invalid count retains prior range and review',d['state']['monitor_window']==2 and d['state']['dialog']=='monitor_range'and d['effect']is None)
        d=event(d['state'],'monitor:range_save',dict(runs='1'));check('exact count applies to actual completed history',d['ui']['monitor']['gauges'][1]['value']=='900 ms')
        d=event(json.loads(json.dumps(d['state'])),'resume');d=event(d['state'],'diagnostics');check('range survives saved-state recovery',d['ui']['monitor']['window']==1)
        d=event(d['state'],'send',dict(draft='Keep live separate'));ticket=d['state']['model_expected'];d=event(d['state'],'model_progress',dict(model_ticket=ticket,stream_text='abcd',elapsed_ms=1000));d=event(d['state'],'diagnostics',dict(draft='Retained draft'))
        check('live estimate has its own card',d['ui']['monitor']['live']['value']=='≈1.0 tok/s')
        check('live request preserves completed-run average',d['ui']['monitor']['gauges'][1]['value']=='900 ms')
        d=event(d['state'],'monitor:range_save',dict(runs='2'));check('range changes preserve active callback and draft',d['state']['model_expected']==ticket and d['state']['busy']and d['state']['draft']=='Retained draft')
        d=event(d['state'],'cancel');before=copy.deepcopy(d['state'])
        for action,inputs in [('rate_tick',dict(model_ticket=ticket,elapsed_ms=9999,draft='STALE')),('model_progress',dict(model_ticket=ticket,elapsed_ms=9999,stream_text='STALE')),('failure',dict(purpose='model',model_ticket=ticket,error='STALE'))]:
            late=event(before,action,inputs);check('stale model delivery preserves exact state: '+action,late['state']==before and late['effect']is None)
        (root/'AGENTS.md').write_text('User project instructions changed after the cancelled request.\n')
        for action,inputs in [('document_result',dict(document_ticket='retired',content='STALE_DOCUMENT')),('image_result',dict(document_ticket='retired')),('document_cancel',dict(document_ticket='retired')),('document_failure',dict(document_ticket='retired',error='STALE_DOCUMENT_ERROR')),('apps_result',dict(ticket='retired',package='app.retired',data=[]))]:
            late=event(before,action,inputs);check('stale platform delivery preserves exact state before project refresh: '+action,late['state']==before and late['effect']is None)
        refreshed=event(before,'diagnostics');check('explicit navigation still refreshes current project instructions','User project instructions changed'in json.dumps(refreshed['state']['harness_project']))
        d=event(d['state'],'monitor:window:2');g=d['ui']['monitor']['gauges'];check('cancelled run is excluded from successful averages',g[1]['value']=='900 ms'and g[3]['value']=='0.0 %')
        st=d['state'];st['selected']=1;st['routing']={'primary':'1'};d=event(st,'send',dict(draft='Other model'));d=finish(d,'c'*40,400,80);record=d['state']['requests'][-1]
        d=event(d['state'],'monitor:model:'+record['id']);check('model filter precedes selected count',d['ui']['monitor']['gauges'][1]['value']=='400 ms'and '1 recorded requests'in d['ui']['monitor']['subtitle'])
        d=event(d['state'],'monitor:models');check('model statistics share selected history',any('1 requests'in f.get('value','')for f in d['ui']['modal']['fields']))
        d=event(d['state'],'monitor:details');check('request list shares selected history',len(d['ui']['modal']['buttons'])==1)
        d=event(d['state'],'monitor:all');d=event(d['state'],'monitor:window:128');check('retention ceiling remains explicit',d['ui']['monitor']['window']==128 and '128 runs'in d['ui']['monitor']['range_label'])
        for card in d['ui']['monitor']['gauges']:
            visual=card['visual'];check('gauge arcs stay within their drawing bounds: '+card['title'],all(s.get('kind')!='arc'or(0<=s['x']-s['width']/2 and s['x2']+s['width']/2<=visual['width']and 0<=s['y']-s['width']/2 and s['y2']+s['width']/2<=visual['height'])for s in visual['shapes']))
        d=event(d['state'],'monitor:range_save',dict(runs='129'));d=event(d['state'],'files');e=d['effect'];result=op(dict(e,op='tool',root=str(root)));check('range failure leaves ordinary local tools available','input.txt'in result)
        pathlib.Path(PROJECT/'output/checks/diagnostic-view.json').write_text(json.dumps(d['ui'],indent=2)+'\n')
    finally:p.stdin.close();p.wait(timeout=10)
result=dict(passed=passed,scope='Owner connected core execution with actual completed requests, saved-state round trip, active callback preservation, selected model/range, ordinary artifact-free local worker and scoped range refusal. Geometry checks describe drawing bounds; installed Android visual acceptance is separate.')
(PROJECT/'output/checks/diagnostics.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
