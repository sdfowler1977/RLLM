"""Opt-in live public search through reviewed child and parent core routes."""
import argparse,base64,hashlib,json,pathlib,subprocess,tempfile

parser=argparse.ArgumentParser()
parser.add_argument('--live',action='store_true',required=True)
parser.parse_args()
PROJECT=pathlib.Path(__file__).resolve().parents[1]
passed=[]
def check(name,value):
    assert value,name
    passed.append(name)

with tempfile.TemporaryDirectory(prefix='live-web-',dir=PROJECT/'output') as directory:
    root=pathlib.Path(directory)
    process=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def event(state,action,inputs=None):
        process.stdin.write(json.dumps(dict(op='event',state=state,action=action,inputs=inputs or{},root=str(root)))+'\n')
        process.stdin.flush()
        result=json.loads(process.stdout.readline())
        assert 'value'in result,result
        return result['value']
    def reply(result,content):
        body=json.dumps({'choices':[{'message':{'content':content},'finish_reason':'stop'}]}).encode()
        raw=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(body)).encode()+b'\r\n\r\n'+body
        inputs=dict(purpose='model',model_ticket=result['state']['model_expected'],data=base64.b64encode(raw).decode())
        for key in ['child_ticket','child_owner']:
            if key in result['effect']:inputs[key]=result['effect'][key]
        return event(result['state'],'network_result',inputs)
    observations=[]
    try:
        state=event(None,'resume')['state']
        state['profiles']=[dict(name=name,model=name,protocol='chat',endpoint='http://'+name+'.test/v1/chat/completions',plain=True,stream=False)for name in ['parent','child']]
        state['agents']=[dict(name='Parent',tools='delegate,web_search,web_read',subagents=[1],model='0',investigation_preferences=False),dict(name='Child',tools='web_search,web_read',model='1',investigation_preferences=False)]
        state['investigation_preferences']={'enabled':False}
        result=event(state,'send',{'draft':'Use the callable child to search for the Rust programming language.'})
        check('intrinsic command process is present with optional guidance off','Investigation is intrinsic to following commands'in result['effect']['wire'])
        result=reply(result,json.dumps(dict(tool='delegate',args=dict(agent=1,task='Search for Rust programming language and return actual source evidence.'))))
        check('child selection enters review',result['state']['dialog']=='approval'and result['effect']is None)
        result=event(result['state'],'approve')
        check('approved child uses its assigned model',result['state']['request_index']==1)
        result=reply(result,json.dumps(dict(tool='web_search',args=dict(query='Rust programming language'))))
        check('child search enters its own review',result['state']['dialog']=='approval'and result['effect']is None)
        result=event(result['state'],'approve')
        for attempt in range(2):
            effect=result['effect']
            assert effect['purpose']=='web_tool',result['state'].get('notice')
            target=effect['wire'].split(' ',2)[1]
            url=('https'if effect['tls']else'http')+'://'+effect['host']+target
            assert effect['host']in ['api.mwmbl.org','html.duckduckgo.com']
            raw=subprocess.check_output(['curl','--fail-with-body','--silent','--show-error','--http1.1','--include','--suppress-connect-headers','--connect-timeout','10','--max-time','30','--max-filesize',str(effect['limit']),'--header','Accept-Encoding: identity',url],timeout=35)
            observations.append(dict(url=url,bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest()))
            inputs=dict(purpose='web_tool',web_ticket=effect['web_ticket'],data=base64.b64encode(raw).decode())
            for key in ['child_ticket','child_owner']:
                if key in effect:inputs[key]=effect[key]
            result=event(result['state'],'network_result',inputs)
            if result['effect']and result['effect'].get('purpose')=='model':break
        check('actual live search evidence returns to the assigned child',result['effect']is not None and result['effect']['purpose']=='model'and result['state']['request_index']==1 and'Search results for: Rust programming language'in result['effect']['wire'])
        check('retained results include source URLs and actual snippets','Returned results'in result['effect']['wire']and'snippet'in result['effect']['wire']and'http'in result['effect']['wire'])
        records=result['state']['inquiry_records']['items']
        check('live returned evidence reaches the integrated journal',any('Search results for: Rust programming language'in e.get('output','')for record in records for e in record.get('evidence',[])))
        result=reply(result,'Child received the live source results for Rust programming language.')
        check('actual child completion returns to the captured parent',result['effect']is not None and result['state']['request_index']==0 and'Child received the live source results'in result['effect']['wire'])
        result=reply(result,'The child returned the search evidence.')
        check('parent completes the ordinary command',not result['state']['busy']and result['effect']is None)
    finally:
        process.stdin.close()
        process.wait(timeout=10)

record=dict(passed=passed,transport_observations=observations,scope='Live public HTTPS search responses transported by curl through actual core review, owned callbacks, evidence retention and child/parent continuation. Model replies are controlled protocol fixtures; configured model inference and installed Android transport require their own execution.')
(PROJECT/'output/checks/live-web.json').write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps(record,indent=2))
