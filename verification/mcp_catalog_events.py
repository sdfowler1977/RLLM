"""Drive MCP catalog notifications through the real loopback and shared stream paths."""
import base64
import copy
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading

project = Path(__file__).resolve().parents[1]
checks, requests = [], []
refresh = 'original'
stream_case = 'normal'
post_case = 'json'
LENGTH_TAIL = b': framed tail\n\n'
CHUNK_TAIL = b'0\r\nX-Trace: framing\r\n\r\n'
BAD_CHUNK_TAIL = b'0\r\nContent-Length: 0\r\n\r\n'
OVERLONG_CHUNK_HEADER = b'1;' + b'x' * 8191
LIMIT_CHUNK_TAIL = b'0;' + b'x' * 8190 + b'\r\n\r\n'
CONTROL_TRAILERS = {
    'control-trailer-forbidden': BAD_CHUNK_TAIL,
    'control-trailer-line': BAD_CHUNK_TAIL[:-2],
    'control-trailer-malformed': b'0\r\nBad Name: value\r\n\r\n',
    'control-trailer-extra': b'0\r\n\r\nextra',
    'control-trailer-valid': CHUNK_TAIL,
    'control-chunk-limit': LIMIT_CHUNK_TAIL,
    'control-chunk-overlong': OVERLONG_CHUNK_HEADER,
}


def check(label, value):
    assert value, label
    checks.append(label)


def tool(name):
    return dict(name=name, inputSchema={'type':'object'})


def notice(method, params=None):
    result = dict(jsonrpc='2.0', method=method)
    if params is not None:
        result['params'] = params
    return result


def sse(records):
    return ''.join(f'id: event-{i}\ndata: {json.dumps(record)}\n\n'
                   for i, record in enumerate(records, 1)).encode()


class Peer(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def send(self, status, body, mime='application/json'):
        self.send_response(status)
        self.send_header('Content-Type', mime)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def send_framed(self, case, body):
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        if case in ('framed-length','truncated-length'):
            self.send_header('Content-Length',str(len(body)+len(LENGTH_TAIL)))
            if case=='framed-length':
                body+=LENGTH_TAIL
        elif case in ('framed-chunk','truncated-chunk','bad-trailer'):
            self.send_header('Transfer-Encoding','chunked')
            body=f'{len(body):x}\r\n'.encode()+body+b'\r\n'
            if case!='truncated-chunk':
                body+=BAD_CHUNK_TAIL if case=='bad-trailer' else CHUNK_TAIL
        self.end_headers()
        self.wfile.write(body)

    def send_control_stream(self, case, reply):
        ping=dict(jsonrpc='2.0', id='server-ping', method='ping')
        body=sse([ping,reply])
        head=b'HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n'
        if case=='control-header-limit':
            interim=b'HTTP/1.1 103 Early Hints\r\nLink: </resource>\r\n\r\n'
            head+=f'Content-Length: {len(body)}\r\nX-Pad: '.encode()
            self.wfile.write(interim+head.ljust(65536,b'x')+b'\r\n\r\n'+body)
            return
        if case in CONTROL_TRAILERS:
            self.wfile.write(head+b'Transfer-Encoding: chunked\r\n\r\n'
                             +f'{len(body):x}\r\n'.encode()+body+b'\r\n'+CONTROL_TRAILERS[case])
            return
        if case in ('control-boundary-valid','control-boundary-invalid'):
            first,second=body[:17],body[17:]
            wire=(head+b'Transfer-Encoding: chunked\r\n\r\n'
                  +f'{len(first):x}\r\n'.encode()+first+b'\r\n'
                  +f'{len(second):x}\r\n'.encode()+second)
            tail=b'\r\n0\r\n\r\n' if case=='control-boundary-valid' else b'\rX'
            self.wfile.write(wire+tail)
            return
        prefix=b''
        if case=='control-bad-status':
            head=head.replace(b' 200',b'  200',1)
        elif case=='control-bad-name':
            head+=b'Bad Name: value\r\n'
        elif case=='control-bad-value':
            head+=b'X-Trace: bad\x00value\r\n'
        elif case=='control-bad-interim':
            prefix=b'HTTP/1.1 103 Early Hints\r\nBad Name: value\r\n\r\n'
        elif case=='control-switching':
            prefix=b'HTTP/1.1 101 Switching Protocols\r\n\r\n'
        elif case=='control-valid':
            prefix=b'HTTP/1.1 103 Early Hints\r\nLink: </resource>\r\n\r\n'
        if case in ('control-bad-chunk','control-bad-last-chunk','control-valid'):
            extension=b';=bad' if case=='control-bad-chunk' else b' \t; flag; key="quoted; value"'
            last=b';=bad' if case=='control-bad-last-chunk' else b';done'
            head+=b'Transfer-Encoding: chunked\r\n'
            body=f'{len(body):x}'.encode()+extension+b'\r\n'+body+b'\r\n0'+last+b'\r\n\r\n'
        else:
            length=f'+{len(body)}' if case=='control-bad-length' else str(len(body))
            head+=f'Content-Length: {length}\r\n'.encode()
        self.wfile.write(prefix+head+b'\r\n'+body)

    def do_POST(self):
        assert self.path in ('/alpha','/beta','/march') and self.headers['Authorization'] == 'Bearer catalog'+self.path
        call = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if 'method' not in call:
            assert call==dict(jsonrpc='2.0',id='server-ping',result={})
            requests.append((self.path,'control-reply',call))
            if post_case=='control-valid':
                self.wfile.write(b'HTTP/1.1 103 Early Hints\r\nLink: </resource>\r\n\r\n'
                                 b'HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n')
            else:
                self.send(202,b'')
            return
        method, args = call['method'], call.get('params', {})
        requests.append((self.path, method, args))
        if method == 'notifications/initialized':
            self.send(202, b'')
            return
        if method == 'initialize':
            version = '2025-03-26' if self.path == '/march' else '2025-11-25'
            result = dict(protocolVersion=version, capabilities={'tools':{'listChanged':self.path != '/beta'}},
                          serverInfo={'name':'Catalog peer'+self.path,'version':'1'})
        elif method == 'tools/list':
            if post_case in ('split-list', 'failed-list'):
                if not args:
                    result = dict(tools=[tool('first')], nextCursor='next')
                elif post_case == 'failed-list':
                    result = dict(tools=[dict(name='bad', inputSchema=False)])
                else:
                    result = dict(tools=[tool('second')])
            elif refresh == 'bad':
                result = dict(tools=[tool('staged')], nextCursor='next') if not args else dict(tools=[dict(name='bad',inputSchema=False)])
            elif refresh == 'empty':
                result = dict(tools=[])
            else:
                result = dict(tools=[tool('original')])
        elif method == 'tools/call':
            assert args == dict(name='original',arguments={})
            result = dict(content=[dict(type='text',text='Reviewed original ran')])
        else:
            raise AssertionError(method)
        reply = dict(jsonrpc='2.0', id=call['id'], result=result)
        changed = notice('notifications/tools/list_changed', {'untrusted':'POST_SERVER_PAYLOAD'})
        if method == 'tools/call' and post_case != 'json':
            if post_case.startswith('control-'):
                self.send_control_stream(post_case,reply)
                return
            if post_case in ('framed-length','truncated-length','framed-chunk','truncated-chunk','bad-trailer','close-delimited'):
                self.send_framed(post_case,sse([changed,reply]))
                return
            if post_case == 'json-spoof':
                self.send(200, json.dumps(dict(reply, mcp_catalog_changed=True)).encode())
                return
            if post_case == 'wrong-id':
                reply['id'] = 'unrelated-request'
            if post_case == 'invalid-result':
                reply['result'] = dict(content=False)
            if post_case == 'bad-march':
                records = [[changed, notice('notifications/custom', [])], reply]
            elif post_case == 'march':
                records = [[notice('notifications/resources/list_changed'), changed], [reply]]
            elif post_case == 'late':
                records = [reply, changed]
            elif post_case == 'duplicate-terminal':
                duplicate = dict(reply, result=dict(content=[dict(type='text',text='Conflicting terminal')]))
                records = [changed, reply, duplicate]
            elif post_case == 'duplicate':
                records = [changed, reply, reply]
            elif post_case == 'batch-then-single':
                error = dict(jsonrpc='2.0', id=reply['id'], error=dict(code=-32603, message='Conflicting terminal'))
                records = [changed, [reply], error]
            elif post_case == 'single-then-batch':
                records = [changed, reply, [reply]]
            elif post_case == 'unrelated-after-terminal':
                records = [changed, reply, dict(reply,id='unrelated-request')]
            else:
                records = [changed, reply]
            self.send(200, sse(records), 'text/plain' if post_case == 'wrong-mime' else 'text/event-stream')
            return
        if method == 'tools/list' and post_case in ('split-list', 'failed-list') and not args:
            self.send(200, sse([changed, reply]), 'text/event-stream')
            return
        self.send(200, json.dumps(reply).encode())

    def do_GET(self):
        assert self.path in ('/alpha','/beta','/march') and self.headers['Authorization'] == 'Bearer catalog'+self.path
        assert self.headers['Accept'] == 'text/event-stream'
        last = self.headers.get('Last-Event-ID')
        requests.append((self.path,'GET',last))
        other = notice('notifications/resources/list_changed')
        changed = notice('notifications/tools/list_changed', {'untrusted':'SERVER_PAYLOAD'})
        if stream_case in ('trailer-observation','chunk-limit-observation'):
            self.send_response(200)
            self.send_header('Content-Type','text/event-stream')
            self.send_header('Transfer-Encoding','chunked')
            self.end_headers()
            ping=dict(jsonrpc='2.0',id='server-ping',method='ping')
            for body in (sse([other]),sse([changed,ping])):
                self.wfile.write(f'{len(body):x}\r\n'.encode()+body+b'\r\n')
            self.wfile.write(BAD_CHUNK_TAIL if stream_case=='trailer-observation' else OVERLONG_CHUNK_HEADER)
            return
        if self.path == '/march':
            records = [[changed, notice('notifications/custom', [])]] if stream_case == 'bad-batch' else [[other, changed]]
        elif stream_case == 'resume' and last is None:
            records = [other]
        else:
            records = [other, changed]
        self.send(200,sse(records),'text/event-stream')


with tempfile.TemporaryDirectory() as folder, http.server.ThreadingHTTPServer(('127.0.0.1', 0), Peer) as peer:
    worker = threading.Thread(target=peer.serve_forever, daemon=True)
    worker.start()
    controller = os.environ.get('RLLM_CONTROLLER', str(project/'output/headless/RLLM'))
    proc = subprocess.Popen([controller,'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)

    def ask_reply(value):
        proc.stdin.write(json.dumps(value)+'\n')
        proc.stdin.flush()
        return json.loads(proc.stdout.readline())

    def ask(value):
        reply = ask_reply(value)
        assert 'value' in reply, reply
        return reply['value']

    def event(st, action, inputs=None):
        return ask(dict(op='event',root=folder,state=st,action=action,inputs=inputs or {}))

    def network(effect):
        assert effect['kind']=='network',effect
        with socket.create_connection((effect['host'],effect['port']), timeout=5) as sock:
            sock.sendall(effect['wire'].encode())
            return b''.join(iter(lambda:sock.recv(65536),b''))

    def exchange(d):
        e=d['effect']
        return event(d['state'],'network_result',dict(purpose=e['purpose'],data=base64.b64encode(network(e)).decode()))

    def post_stream(d, feed_error=False, finish_error=False, whole=False, split_terminal=False, read_size=37):
        effect=d['effect']
        stream=ask(dict(op='stream_open',protocol=effect['stream_protocol'],mcp_control=effect['mcp_control']))
        raw=network(effect)
        # Exercise the real parser across header and event boundaries.
        size=len(raw) if whole else read_size
        if split_terminal:
            boundary=raw.index(b'id: event-2')
            fragments=(raw[:boundary],raw[boundary:])
        else:
            fragments=(raw[offset:offset+size] for offset in range(0,len(raw),size))
        for index, fragment in enumerate(fragments):
            part=ask(dict(op='stream_feed',id=stream,data=base64.b64encode(fragment).decode()))
            if split_terminal and index==0:
                check('first correlated reply does not end a partially received declared body',not part['complete'] and 'error' not in part)
            if 'error' in part:
                assert feed_error, part
                assert 'error' in ask_reply(dict(op='stream_finish',id=stream,eof=True))
                ask(dict(op='stream_close',id=stream))
                return part
        assert not feed_error
        args=dict(op='stream_finish',id=stream,eof=True)
        if finish_error:
            reply=ask_reply(args)
            assert 'error' in reply, reply
            ask(dict(op='stream_close',id=stream))
            return reply
        return ask(args)

    def post_deliver(d, result, purpose=None):
        callback=dict(result,purpose=purpose or d['effect']['purpose'])
        return event(d['state'],'network_result',callback)

    def call_original(st):
        chosen=event(event(st,'mcp_tools')['state'],'mcp_tool:0')
        reviewed=event(chosen['state'],'mcp_client_review',{'arguments':'{}'})
        assert reviewed['effect'] is None
        return exchange(exchange(event(reviewed['state'],'mcp_client_approve')))

    def register(st, path, initial=False):
        d=event(st,'mcp_client_settings' if initial else 'mcp_client_new')
        return event(d['state'],'mcp_client_save',dict(endpoint=f'http://127.0.0.1:{peer.server_port}'+path,
                     key='catalog'+path,access=True))

    def negotiate(st, action):
        return exchange(exchange(event(st,action)))

    def discover(st):
        return exchange(negotiate(st,'mcp_client_list'))

    def names(st):
        return [row['name'] for row in st.get('mcp_client_catalog',[])]

    def changed(st):
        d=event(st,'mcp_tools')
        return any(b['label']=='Refresh changed catalog' and b['event']=='mcp_client_list'
                   for b in d['ui']['modal']['buttons'])

    def open_get(d):
        e=d['effect']
        raw=network(e)
        stream=ask(dict(op='stream_open',protocol='mcp_server',mcp_control=e['mcp_control']))
        data=ask(dict(op='stream_feed',id=stream,data=base64.b64encode(raw).decode()))
        assert 'error' not in data,data
        return stream,data['observation']

    def progress(d, obs, purpose=None):
        return event(d['state'],'mcp_client_server_progress',dict(purpose=purpose or d['effect']['purpose'],counts=obs))

    def finish(d, stream, suspend=False):
        op='stream_suspend' if suspend else 'stream_finish'
        summary=ask(dict(op=op,id=stream,eof=True,owner=d['effect']['purpose']))
        return event(d['state'],'network_result',dict(purpose=d['effect']['purpose'],data=summary['data']))

    def select(st, index):
        return event(event(st,'mcp_client_connections')['state'],f'mcp_client_select:{index}')

    def select_path(st, path):
        index=next(i for i, config in enumerate(st['mcp_saved_connections']) if config['endpoint'].endswith(path))
        return select(st,index)

    try:
        a=register(event(None,'resume')['state'],'/alpha',True)
        a=discover(a['state'])
        check('baseline complete catalog from the real peer',names(a['state'])==['original'])
        listening=negotiate(a['state'],'mcp_client_listen')
        sid,obs=open_get(listening)
        check('negotiated standalone stream distinguishes tool-list changes',obs.get('tool_changes')==1 and obs['notifications']==2)
        wrong=progress(listening,obs,'mcp_client_server:stale')
        check('stale progress cannot mark the current connection',not wrong['state'].get('mcp_catalog_changes'))
        replaced=copy.deepcopy(listening)
        replaced['state']['mcp']['key']='replacement'
        rejected=progress(replaced,obs)
        check('changed connection credentials invalidate old observations',not rejected['state'].get('mcp_catalog_changes'))
        revoked=copy.deepcopy(listening)
        revoked['state']['storage_access']['mcp']=False
        rejected=progress(revoked,obs)
        check('revoked server access prevents adoption of live observations',not rejected['state'].get('mcp_catalog_changes'))
        malformed=progress(listening,dict(obs,tool_changes=3))
        check('impossible counts cannot mark a catalog',not malformed['state'].get('mcp_catalog_changes'))
        step=progress(listening,obs)
        check('live owned observation marks a refresh without auto dispatch',step['effect'] is None and 'refresh recommended' in step['state']['notice']
              and names(step['state'])==['original'] and 'SERVER_PAYLOAD' not in json.dumps(step))
        regress=progress(dict(listening,state=step['state']),dict(obs,tool_changes=0))
        check('regressing progress cannot clear the observation',regress['state']['mcp_client_observation']['tool_changes']==1)
        closed=finish(dict(listening,state=step['state']),sid)
        check('completed stream leaves the old catalog and offers manual refresh',changed(closed['state']) and names(closed['state'])==['original'])
        observed=event(closed['state'],'mcp_client_observation')
        check('the stream observation offers the same explicit refresh action',
              any(b['event']=='mcp_client_list' for b in observed['ui']['modal']['buttons']))
        count=len(requests)
        chosen=event(event(closed['state'],'mcp_tools')['state'],'mcp_tool:0')
        reviewed=event(chosen['state'],'mcp_client_review',{'arguments':'{}'})
        check('dirty catalog preserves explicit review and sends no call before approval',reviewed['effect'] is None and len(requests)==count)
        done=exchange(exchange(exchange(event(reviewed['state'],'mcp_client_approve'))))
        check('the reviewed tool can complete while the catalog is dirty','Reviewed original ran' in done['state']['output'] and changed(done['state']))
        refresh='bad'
        bad=exchange(exchange(negotiate(done['state'],'mcp_client_list')))
        check('failed later page retains the old list and refresh indication',bad['state']['notice_error']
              and names(bad['state'])==['original'] and changed(bad['state']))
        beta=register(bad['state'],'/beta')
        listen_beta=negotiate(beta['state'],'mcp_client_listen')
        sid,obs=open_get(listen_beta)
        check('unadvertised tool-change event contributes only generic counts',obs['notifications']==2 and obs['tool_changes']==0)
        beta=finish(listen_beta,sid)
        check('other connection has no inherited refresh indication',not changed(beta['state']))
        a=select(beta['state'],0)
        check('returning to the original saved connection restores its own indication',changed(a['state']))
        a=event(a['state'],'resume')
        check('saved indication survives controller resume',changed(a['state']))
        refresh='empty'
        a=discover(a['state'])
        check('terminal empty discovery clears the indication with the catalog',not names(a['state']) and not changed(a['state']))
        stream_case='resume'
        paused=negotiate(a['state'],'mcp_client_listen_resume')
        sid,obs=open_get(paused)
        check('first resumable segment has no tool-change event',obs['tool_changes']==0)
        paused=finish(paused,sid,True)
        resumed=event(paused['state'],'mcp_client_resume')
        sid,obs=open_get(resumed)
        check('owned resume retains negotiated change capability and cursor',obs['tool_changes']==1 and requests[-1]==('/alpha','GET','event-1'))
        resumed=finish(resumed,sid)
        check('final-only resumed observation marks the correct connection',changed(resumed['state']))
        resumed=event(resumed['state'],'mcp_client_resume_close')
        march=register(resumed['state'],'/march')
        listening=negotiate(march['state'],'mcp_client_listen')
        sid,obs=open_get(listening)
        check('negotiated March SSE batch counts only the tool-list notification',obs['tool_changes']==1 and obs['notifications']==2)
        closed=finish(listening,sid)
        check('March terminal callback also exposes the refresh action',changed(closed['state']))
        clean=discover(closed['state'])
        stream_case='bad-batch'
        listening=negotiate(clean['state'],'mcp_client_listen')
        effect=listening['effect']
        sid=ask(dict(op='stream_open',protocol='mcp_server',mcp_control=effect['mcp_control']))
        bad=ask(dict(op='stream_feed',id=sid,data=base64.b64encode(network(effect)).decode()))
        observed=progress(listening,bad['observation'])
        check('a malformed March batch commits no tool-change count or indication', 'error' in bad
              and bad['observation']['tool_changes']==0 and observed['state'].get('mcp_catalog_changes')==clean['state'].get('mcp_catalog_changes'))
        ask(dict(op='stream_close',id=sid))
        check('no notification automatically dispatches discovery or calls',sum(m=='tools/list' for _,m,_ in requests)==5
              and sum(m=='tools/call' for _,m,_ in requests)==1)

        refresh='original'
        a=discover(select_path(clean['state'],'/alpha')['state'])
        pending=call_original(a['state'])
        post_case='single'
        result=post_stream(pending)
        payload=json.loads(base64.b64decode(result['data']).split(b'\r\n\r\n',1)[1])
        check('negotiated POST SSE emits a separate typed catalog observation',result.get('mcp_catalog_changed') is True)
        check('correlated terminal result is unchanged and excludes the notice payload',
              payload==dict(jsonrpc='2.0',id=pending['effect']['mcp_control']['request_id'],
                            result=dict(content=[dict(type='text',text='Reviewed original ran')])))
        stale=post_deliver(pending,result,'mcp_client_result:stale')
        check('stale POST completion cannot mark the catalog',not stale['state'].get('mcp_catalog_changes'))
        revoked=copy.deepcopy(pending)
        revoked['state']['storage_access']['mcp']=False
        check('revoked connection rejects the POST observation',not post_deliver(revoked,result)['state'].get('mcp_catalog_changes'))
        replaced=copy.deepcopy(pending)
        replaced['state']['mcp']['key']='replacement'
        check('changed credentials reject the POST observation',not post_deliver(replaced,result)['state'].get('mcp_catalog_changes'))
        done=post_deliver(pending,result)
        check('approved call adopts its result and offers manual refresh without dispatch',
              done['effect'] is None and changed(done['state']) and names(done['state'])==['original']
              and 'Reviewed original ran' in done['state']['output'] and 'POST_SERVER_PAYLOAD' not in json.dumps(done))
        check('replayed completion cannot restore a cleared catalog indication',
              not changed(post_deliver(dict(pending,state=discover(done['state'])['state']),result)['state']))

        post_case='duplicate-terminal'
        pending=call_original(a['state'])
        bad=post_stream(pending,feed_error=True,whole=True)
        check('two correlated single responses in one fragment fail without publishing either result',
              'Duplicate correlated MCP terminal response' in bad.get('error','') and not bad['complete']
              and 'data' not in bad and 'mcp_catalog_changed' not in bad)
        another=call_original(a['state'])
        bad_split=post_stream(another,feed_error=True,split_terminal=True)
        check('duplicate terminal in the next read fails before returning a response',
              'Duplicate correlated MCP terminal response' in bad_split.get('error','') and 'data' not in bad_split)
        failed=event(pending['state'],'failure',dict(purpose=pending['effect']['purpose'],error=bad['error']))
        check('scoped duplicate failure preserves the last catalog without adopting the notice',
              names(failed['state'])==['original'] and not changed(failed['state']) and failed['state']['notice_error'])
        post_case='single'
        retry=call_original(failed['state'])
        check('a fresh reviewed call still succeeds after duplicate refusal',
              'Reviewed original ran' in post_deliver(retry,post_stream(retry))['state']['output'])

        for case in ('late','json-spoof','wrong-mime','wrong-id','invalid-result'):
            pending=call_original(a['state'])
            post_case=case
            result=post_stream(pending,finish_error=case=='wrong-id')
            if case=='wrong-id':
                check('uncorrelated SSE reply exports no catalog observation','mcp_catalog_changed' not in result)
            else:
                done=post_deliver(pending,result)
                check(f'{case} cannot dirty the catalog',not changed(done['state']))
                if case=='invalid-result':
                    check('invalid tool result still reports its validation failure',done['state']['notice_error'])
                else:
                    check(f'{case} preserves the valid call result','Reviewed original ran' in done['state']['output'])

        beta=discover(select_path(a['state'],'/beta')['state'])
        post_case='single'
        pending=call_original(beta['state'])
        result=post_stream(pending)
        check('unadvertised POST notification leaves the other connection clean',
              'mcp_catalog_changed' not in result and not changed(post_deliver(pending,result)['state']))

        march=discover(select_path(beta['state'],'/march')['state'])
        pending=call_original(march['state'])
        post_case='march'
        result=post_stream(pending)
        done=post_deliver(pending,result)
        check('March request-direction batch marks the catalog without changing the correlated batch result',
              changed(done['state']) and 'Reviewed original ran' in done['state']['output'])
        post_case='bad-march'
        pending=call_original(march['state'])
        bad=post_stream(pending,feed_error=True)
        check('invalid March batch exposes neither a completion nor a POST observation',
              'data' not in bad and 'mcp_catalog_changed' not in bad)

        for case in ('split-list','failed-list'):
            post_case=case
            pending=negotiate(a['state'],'mcp_client_list')
            page=post_deliver(pending,post_stream(pending))
            check(f'{case} keeps the last complete catalog while paging',
                  names(page['state'])==['original'] and any(page['state'].get('mcp_catalog_changes',{}).values()))
            final=post_deliver(page,post_stream(page))
            if case=='split-list':
                check('later clean page retains the indication for the assembled catalog',
                      names(final['state'])==['first','second'] and changed(final['state']))
            else:
                check('invalid later page retains both the old catalog and the observed change',
                      final['state']['notice_error'] and names(final['state'])==['original'] and changed(final['state']))
        post_case='json'
        check('clean discovery after an interleaved notice clears the indication',not changed(discover(final['state'])['state']))

        for case,connection in (('duplicate',a),('batch-then-single',march),('single-then-batch',march)):
            pending=call_original(connection['state'])
            post_case=case
            rejected=post_stream(pending,feed_error=True,read_size=8192)
            check(f'{case} rejects a second correlated reply in the same transport read',
                  'Duplicate correlated MCP terminal response' in rejected['error']
                  and not rejected['complete'] and 'data' not in rejected and 'mcp_catalog_changed' not in rejected)
            failed=event(pending['state'],'failure',dict(purpose=pending['effect']['purpose'],error=rejected['error'],sent=True))
            check(f'{case} keeps the last complete catalog and yields a scoped error without replay',
                  names(failed['state'])==['original'] and failed['state']['notice_error']
                  and not changed(failed['state']) and failed['effect'] is None)
        evidence=Path(folder)/'evidence.txt'
        evidence.write_text('local evidence')
        check('local read works after the MCP transport refusal',
              ask(dict(op='tool',tool='read',root=folder,args=dict(path=evidence.name)))=='local evidence')
        pending=call_original(failed['state'])
        post_case='unrelated-after-terminal'
        recovered=post_deliver(pending,post_stream(pending,read_size=8192))
        check('a fresh reviewed call adopts its result when a trailing reply has an unrelated ID',
              'Reviewed original ran' in recovered['state']['output'] and changed(recovered['state']))

        for case,message in (
            ('control-trailer-forbidden','Framing trailer forbidden'),
            ('control-trailer-line','Framing trailer forbidden'),
            ('control-trailer-malformed','Invalid response header'),
            ('control-trailer-extra','Bytes after framed response'),
            ('control-chunk-overlong','Stream chunk header exceeds limit'),
        ):
            pending=call_original(a['state'])
            post_case=case
            before=sum(method=='control-reply' for _,method,_ in requests)
            rejected=post_stream(pending,feed_error=True,read_size=16384)
            check(f'{case} refuses a same-read ping before exporting a control effect',
                  rejected['error']==message and not rejected.get('controls') and not rejected['complete'])
            failed=event(pending['state'],'failure',dict(purpose=pending['effect']['purpose'],error=message,sent=True))
            check(f'{case} retains the catalog without replay or a ping reply',
                  names(failed['state'])==['original'] and not changed(failed['state']) and failed['state']['notice_error']
                  and failed['effect'] is None and sum(method=='control-reply' for _,method,_ in requests)==before)

        pending=call_original(a['state'])
        effect=pending['effect']
        post_case='control-trailer-valid'
        raw=network(effect)
        stream=ask(dict(op='stream_open',protocol=effect['stream_protocol'],mcp_control=effect['mcp_control']))

        def feed(data):
            return ask(dict(op='stream_feed',id=stream,data=base64.b64encode(data).decode()))

        early=feed(raw[:-len(CHUNK_TAIL)])
        check('a validated earlier chunk offers its ping before trailers arrive',
              not early['complete'] and 'error' not in early and len(early['controls'])==1)
        control,=early['controls']
        ack=network(control)
        check('the early ping receives a real HTTP acknowledgment while framing is incomplete',
              not ask(dict(op='stream_control_ack',id=stream,owner=control['owner'],ticket=control['ticket'],
                           data=base64.b64encode(ack).decode(),eof=True))['complete'])
        partial=feed(CHUNK_TAIL[:-1])
        last=feed(CHUNK_TAIL[-1:])
        check('valid fragmented trailers wait for final CRLF and emit no duplicate controls',
              'error' not in partial and not partial['complete'] and not partial['controls']
              and 'error' not in last and last['complete'] and not last['controls'])
        done=post_deliver(pending,ask(dict(op='stream_finish',id=stream,eof=False)))
        check('valid trailers preserve the correlated result', 'Reviewed original ran' in done['state']['output'])

        for case in ('control-chunk-limit','control-chunk-overlong'):
            pending=call_original(a['state'])
            effect=pending['effect']
            post_case=case
            raw=network(effect)
            stream=ask(dict(op='stream_open',protocol=effect['stream_protocol'],mcp_control=effect['mcp_control']))
            early=feed(raw[:8192])
            check(f'{case} offers its earlier ping from an Android-sized read',
                  'error' not in early and not early['complete'] and len(early['controls'])==1)
            control,=early['controls']
            ack=network(control)
            check(f'{case} accepts a real ping acknowledgment before complete chunk framing',
                  not ask(dict(op='stream_control_ack',id=stream,owner=control['owner'],ticket=control['ticket'],
                               data=base64.b64encode(ack).decode(),eof=True))['complete'])
            if case=='control-chunk-overlong':
                rejected=feed(raw[8192:])
                check('the next Android-sized read refuses the overlong header before CRLF',
                      rejected['error']=='Stream chunk header exceeds limit'
                      and not rejected.get('controls') and not rejected['complete'])
                check('earlier ping acknowledgment cannot settle the refused tool result',
                      'error' in ask_reply(dict(op='stream_finish',id=stream,eof=True)))
                continue
            partial=feed(raw[8192:-3])
            check('an exact-limit chunk header with pending CR waits for LF',
                  'error' not in partial and not partial['complete'] and not partial['controls'])
            last=feed(raw[-3:])
            check('the split delimiter completes the exact-limit chunk header without a duplicate reply',
                  'error' not in last and last['complete'] and not last['controls'])
            done=post_deliver(pending,ask(dict(op='stream_finish',id=stream,eof=False)))
            check('the exact-limit header preserves the correlated result', 'Reviewed original ran' in done['state']['output'])

        for case,message in (('trailer-observation','Framing trailer forbidden'),
                             ('chunk-limit-observation','Stream chunk header exceeds limit')):
            listening=negotiate(a['state'],'mcp_client_listen')
            effect=listening['effect']
            stream_case=case
            raw=network(effect)
            stream_case='normal'
            stream=ask(dict(op='stream_open',protocol='mcp_server',mcp_control=effect['mcp_control']))
            body_start=raw.index(b'\r\n\r\n')+4
            first_size_end=raw.index(b'\r\n',body_start)
            split=first_size_end+2+int(raw[body_start:first_size_end],16)+2
            early=feed(raw[:split])['observation']
            rejected=feed(raw[split:])
            check(f'{case} preserves prior GET progress and suppresses same-read observations and controls',
                  early['notifications']==1 and not early.get('tool_changes')
                  and rejected['error']==message and rejected['observation']==early
                  and not rejected.get('controls') and not rejected['complete'])
            check(f'{case} rejected GET progress cannot dirty the catalog',
                  not progress(listening,rejected['observation'])['state'].get('mcp_catalog_changes'))
            ask(dict(op='stream_close',id=stream))

        for case,tail in (('framed-length',LENGTH_TAIL),('framed-chunk',CHUNK_TAIL),
                          ('truncated-length',b''),('truncated-chunk',b''),
                          ('bad-trailer',BAD_CHUNK_TAIL),('close-delimited',b'')):
            pending=call_original(a['state'])
            effect=pending['effect']
            post_case=case
            raw=network(effect)
            stream=ask(dict(op='stream_open',protocol=effect['stream_protocol'],mcp_control=effect['mcp_control']))

            def feed(data):
                return ask(dict(op='stream_feed',id=stream,data=base64.b64encode(data).decode()))

            prefix=raw[:-len(tail)] if tail else raw
            observed=feed(prefix)
            check(f'{case} waits for the declared body or connection close',
                  'error' not in observed and not observed['complete'])
            if case=='bad-trailer':
                rejected=feed(tail[:-1])
                check('a complete forbidden trailer line fails before the final empty line',
                      rejected['error']=='Framing trailer forbidden' and not rejected['complete'])
            elif tail:
                assert raw.endswith(tail)
                check(f'{case} stays incomplete until the final framing byte',not feed(tail[:-1])['complete'])
                check(f'{case} makes the complete wire available for validation',feed(tail[-1:])['complete'])
            result=ask_reply(dict(op='stream_finish',id=stream,eof=case.startswith('truncated-') or case=='close-delimited'))
            if case.startswith('truncated-') or case=='bad-trailer':
                check(f'{case} publishes neither a terminal result nor a catalog indication',
                      'error' in result and 'value' not in result)
                error=rejected['error'] if case=='bad-trailer' else result['error']
                failure=event(pending['state'],'failure',dict(purpose=effect['purpose'],error=error,sent=True))
                check(f'{case} remains a scoped refusal without automatic retry',
                      names(failure['state'])==['original'] and not changed(failure['state'])
                      and failure['state']['notice_error'] and failure['effect'] is None)
                ask(dict(op='stream_close',id=stream))
            else:
                complete=post_deliver(pending,result['value'])
                check(f'{case} publishes the exact correlated result and indication',
                      'Reviewed original ran' in complete['state']['output'] and changed(complete['state']))
        check('ordinary local reads survive incomplete HTTP framing',
              ask(dict(op='tool',tool='read',root=folder,args=dict(path=evidence.name)))=='local evidence')

        invalid_control_cases={
            'control-bad-length':'Invalid response length',
            'control-bad-status':'Invalid HTTP status line',
            'control-bad-name':'Invalid response header',
            'control-bad-value':'Invalid response header',
            'control-bad-interim':'Invalid response header',
            'control-switching':'Unsupported informational response',
            'control-bad-chunk':'Invalid chunk extension',
            'control-bad-last-chunk':'Invalid chunk extension',
        }
        for case,message in invalid_control_cases.items():
            pending=call_original(a['state'])
            post_case=case
            before=sum(method=='control-reply' for _,method,_ in requests)
            rejected=post_stream(pending,feed_error=True,read_size=8192)
            check(f'{case} refuses the wire before exporting a control effect',
                  rejected['error']==message and not rejected.get('controls') and not rejected['complete'])
            failed=event(pending['state'],'failure',dict(purpose=pending['effect']['purpose'],error=message,sent=True))
            check(f'{case} retains the catalog without replay or sending a ping reply',
                  names(failed['state'])==['original'] and failed['state']['notice_error']
                  and failed['effect'] is None and sum(method=='control-reply' for _,method,_ in requests)==before)

        pending=call_original(failed['state'])
        effect=pending['effect']
        post_case='control-valid'
        raw=network(effect)
        stream=ask(dict(op='stream_open',protocol=effect['stream_protocol'],mcp_control=effect['mcp_control']))
        acknowledgments=0
        for offset in range(0,len(raw),31):
            part=ask(dict(op='stream_feed',id=stream,data=base64.b64encode(raw[offset:offset+31]).decode()))
            assert 'error' not in part,part
            for control in part['controls']:
                ack=network(control)
                binding=dict(id=stream,owner=control['owner'],ticket=control['ticket'])
                hint,final=ack.split(b'\r\n\r\n',1)
                assert hint.startswith(b'HTTP/1.1 103') and final.startswith(b'HTTP/1.1 202')
                def ready(wire):
                    return ask(dict(op='stream_control_probe',**binding,data=base64.b64encode(wire).decode()))['ready']
                check('MCP acknowledgment waits for the final response and its complete delimiter',
                      not ready(hint+b'\r\n\r\n') and not ready(ack[:-1]))
                check('framed MCP acknowledgment after 103 is ready without EOF',ready(ack))
                ask(dict(op='stream_control_ack',**binding,data=base64.b64encode(ack).decode(),eof=False))
                acknowledgments+=1
        done=post_deliver(pending,ask(dict(op='stream_finish',id=stream,eof=False)))
        check('valid fragmented interim and chunk extensions allow an actual ping reply and result',
              acknowledgments==1 and requests[-1][1]=='control-reply'
              and 'Reviewed original ran' in done['state']['output'])
        check('local reads stay available after malformed MCP framing',
              ask(dict(op='tool',tool='read',root=folder,args=dict(path=evidence.name)))=='local evidence')

        pending=call_original(a['state'])
        effect=pending['effect']
        post_case='control-header-limit'
        raw=network(effect)
        stream=ask(dict(op='stream_open',protocol=effect['stream_protocol'],mcp_control=effect['mcp_control']))
        header_end=raw.index(b'\r\n\r\n',raw.index(b'\r\n\r\n')+4)
        for offset in range(0,header_end,8192):
            part=ask(dict(op='stream_feed',id=stream,data=base64.b64encode(raw[offset:min(offset+8192,header_end)]).decode()))
            assert 'error' not in part and not part['complete'] and not part['controls'],part
        part=ask(dict(op='stream_feed',id=stream,data=base64.b64encode(raw[header_end:header_end+3]).decode()))
        check('MCP interim bytes and a pending delimiter do not consume the final header budget',
              'error' not in part and not part['complete'] and not part['controls'])
        part=ask(dict(op='stream_feed',id=stream,data=base64.b64encode(raw[header_end+3:]).decode()))
        check('the exact-limit MCP header releases one real ping after its final delimiter byte',
              'error' not in part and not part['complete'] and len(part['controls'])==1)
        control=part['controls'][0]
        ack=ask(dict(op='stream_control_ack',id=stream,owner=control['owner'],ticket=control['ticket'],
                     data=base64.b64encode(network(control)).decode(),eof=True))
        done=post_deliver(pending,ask(dict(op='stream_finish',id=stream,eof=False)))
        check('the exact-limit split MCP header preserves the acknowledged result',
              ack['complete'] and requests[-1][1]=='control-reply' and 'Reviewed original ran' in done['state']['output'])

        for case,tail in (('control-boundary-invalid',b'\rX'),
                          ('control-boundary-valid',b'\r\n0\r\n\r\n')):
            pending=call_original(a['state'])
            effect=pending['effect']
            post_case=case
            raw=network(effect)
            assert raw.endswith(tail)
            stream=ask(dict(op='stream_open',protocol=effect['stream_protocol'],mcp_control=effect['mcp_control']))

            def feed_boundary(data):
                return ask(dict(op='stream_feed',id=stream,data=base64.b64encode(data).decode()))

            part=feed_boundary(raw[:-len(tail)])
            check(f'{case} retains control data until the chunk delimiter arrives',
                  'error' not in part and not part['complete'] and not part['controls'])
            part=feed_boundary(tail[:1])
            check(f'{case} retains control data after only the first delimiter byte',
                  'error' not in part and not part['complete'] and not part['controls'])
            part=feed_boundary(tail[1:2])
            if case=='control-boundary-invalid':
                check('split invalid delimiter refuses the stream without exporting a reply',
                      part['error']=='Invalid stream chunk terminator' and not part.get('controls') and not part['complete'])
                check('invalid chunk delimiter prevents adoption of the terminal result',
                      'error' in ask_reply(dict(op='stream_finish',id=stream,eof=True)))
                failed=event(pending['state'],'failure',dict(purpose=effect['purpose'],error=part['error'],sent=True))
                check('invalid delimiter remains a scoped failure and retains the catalog',
                      names(failed['state'])==['original'] and failed['state']['notice_error'] and failed['effect'] is None)
                ask(dict(op='stream_close',id=stream))
                continue
            check('valid split delimiter exports the ping once across two data chunks',
                  'error' not in part and not part['complete'] and len(part['controls'])==1)
            control=part['controls'][0]
            ack=network(control)
            acknowledgment=ask(dict(op='stream_control_ack',id=stream,owner=control['owner'],ticket=control['ticket'],
                                    data=base64.b64encode(ack).decode(),eof=True))
            check('valid ping acknowledgment still waits for the final HTTP chunk',not acknowledgment['complete'])
            part=feed_boundary(tail[2:])
            check('final chunk completes the owned stream without a second ping reply',
                  'error' not in part and part['complete'] and not part['controls'])
            done=post_deliver(pending,ask(dict(op='stream_finish',id=stream,eof=False)))
            check('valid fragmented chunk boundaries retain the exact tool result',
                  'Reviewed original ran' in done['state']['output'] and requests[-1][1]=='control-reply')
        check('ordinary local reads survive split invalid chunk delimiters',
              ask(dict(op='tool',tool='read',root=folder,args=dict(path=evidence.name)))=='local evidence')

        report=dict(passed=len(checks),checks=checks,http_requests=len(requests))
        (project/'output/checks').mkdir(parents=True,exist_ok=True)
        (project/'output/checks/mcp-catalog-events.json').write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps(report))
    finally:
        proc.terminate()
        proc.wait(timeout=5)
        peer.shutdown()
        worker.join(timeout=5)
