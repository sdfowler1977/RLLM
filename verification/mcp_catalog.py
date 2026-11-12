"""Run paginated discovery against an actual local MCP HTTP peer."""
import base64
import http.server
import json
import os
import pathlib
import socket
import subprocess
import tempfile
import threading

ROOT = pathlib.Path(__file__).resolve().parents[1]
checks, requests = [], []
mode = 'original'

def check(label, value):
    assert value, label
    checks.append(label)

def tool(name):
    return dict(name=name, inputSchema={'type': 'object'})

class Peer(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        assert self.path == '/mcp' and self.headers['Authorization'] == 'Bearer catalog-test'
        msg = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        method, params = msg['method'], msg.get('params', {})
        requests.append((method, params))
        status, body = 200, b''
        if method == 'notifications/initialized':
            status = 202
        else:
            if method == 'initialize':
                result = dict(protocolVersion='2025-11-25', capabilities={'tools': {}},
                              serverInfo={'name': 'Catalog peer', 'version': '1'})
            elif method == 'tools/list':
                if mode == 'empty':
                    result = dict(tools=[])
                elif mode == 'original':
                    result = dict(tools=[tool('original')])
                elif mode == 'duplicate-page':
                    result = dict(tools=[tool('first'), tool('first')])
                elif not params:
                    result = dict(tools=[tool('original' if mode == 'duplicate-original' else 'first')], nextCursor='second-page')
                elif params == {'cursor': 'second-page'}:
                    if mode == 'failure':
                        result = dict(tools=[{'name': 'invalid', 'inputSchema': False}])
                    elif mode in ('duplicate-chain', 'duplicate-original'):
                        name = 'original' if mode == 'duplicate-original' else 'first'
                        result = dict(tools=[dict(name=name, inputSchema={'type': 'object', 'required': ['different']})])
                    elif mode == 'case-sensitive':
                        result = dict(tools=[tool('First')])
                    else:
                        result = dict(tools=[tool('second')])
                else:
                    raise AssertionError(params)
            else:
                raise AssertionError(method)
            body = json.dumps(dict(jsonrpc='2.0', id=msg['id'], result=result)).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

with tempfile.TemporaryDirectory() as folder, http.server.ThreadingHTTPServer(('127.0.0.1', 0), Peer) as peer:
    worker = threading.Thread(target=peer.serve_forever, daemon=True)
    worker.start()
    controller = os.environ.get('RLLM_CONTROLLER', str(ROOT/'output/headless/RLLM'))
    process = subprocess.Popen([controller, '--jsonl'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    def event(state, action, inputs=None):
        request = dict(op='event', root=folder, state=state, action=action, inputs=inputs or {})
        process.stdin.write(json.dumps(request)+'\n')
        process.stdin.flush()
        reply = json.loads(process.stdout.readline())
        assert 'value' in reply, reply
        return reply['value']
    def exchange(step):
        effect = step['effect']
        assert effect['kind'] == 'network', step['state']['notice']
        with socket.create_connection((effect['host'], effect['port']), timeout=5) as sock:
            sock.sendall(effect['wire'].encode())
            raw = b''.join(iter(lambda: sock.recv(65536), b''))
        inputs = dict(purpose=effect['purpose'], data=base64.b64encode(raw).decode())
        return event(step['state'], 'network_result', inputs), inputs
    def names(state):
        return [entry['name'] for entry in state.get('mcp_client_catalog', [])]
    def start(state):
        d = event(state, 'mcp_client_list')
        for _ in range(2):
            d, _ = exchange(d)
        return d
    try:
        d = event(None, 'resume')
        d = event(d['state'], 'mcp_client_settings')
        d = event(d['state'], 'mcp_client_save', dict(endpoint=f'http://127.0.0.1:{peer.server_port}/mcp', key='catalog-test', access=True))
        d, _ = exchange(start(d['state']))
        prior = d['state']
        check('original catalog published after successful discovery', names(prior) == ['original'])
        mode = 'failure'
        pending, _ = exchange(start(prior))
        failed, late = exchange(pending)
        check('invalid later page cannot replace the last complete catalog', names(failed['state']) == ['original'])
        check('refused page reports scoped failure and settles discovery', failed['state']['notice_error'] and not failed['state']['busy'])
        check('failed refresh preserves exact published catalog text', failed['state']['mcp_client_tools'] == prior['mcp_client_tools'])
        check('failed refresh discards staged pages', failed['state'].get('mcp_client_listing') is None)
        check('pending first page never appeared in the published list', names(pending['state']) == ['original'])
        legacy = dict(prior, mcp_client_catalog=[tool('legacy'), tool('legacy'), tool('other')])
        catalog = event(legacy, 'mcp_tools')
        buttons = {row['event']: row for row in catalog['ui']['modal']['buttons']}
        check('ambiguous legacy rows are disabled and unique rows remain selectable', all(buttons[f'mcp_tool:{i}'].get('enabled') is False for i in (0, 1)) and buttons['mcp_tool:2'].get('enabled') is not False)
        rejected = event(catalog['state'], 'mcp_tool:1')
        check('an explicit legacy selection is also refused', rejected['effect'] is None and rejected['state']['notice_error'] and 'duplicate tool name' in rejected['state']['notice'])
        call = event(catalog['state'], 'mcp_client_call')
        rejected = event(call['state'], 'mcp_client_review', dict(tool='legacy', arguments='{}'))
        check('manual entry cannot bypass the legacy duplicate refusal', rejected['effect'] is None and rejected['state']['notice_error'] and rejected['state'].get('mcp_client_pending') is None)
        selected = event(catalog['state'], 'mcp_tool:2')
        review = event(selected['state'], 'mcp_client_review', dict(arguments='{}'))
        check('a distinct tool in older state can still be reviewed', review['state']['dialog'] == 'mcp_client_review')
        mode = 'duplicate-original'
        pending, _ = exchange(start(failed['state']))
        newly_ambiguous, _ = exchange(pending)
        check('a newly duplicated name keeps the complete earlier catalog visible', names(newly_ambiguous['state']) == ['original'])
        refreshed = event(newly_ambiguous['state'], 'mcp_tools')
        check('the retained row for that name is now disabled', next(row for row in refreshed['ui']['modal']['buttons'] if row['event'] == 'mcp_tool:0')['enabled'] is False)
        manual = event(refreshed['state'], 'mcp_client_call')
        blocked = event(manual['state'], 'mcp_client_review', dict(tool='original', arguments='{}'))
        check('manual calls cannot bypass a newly observed duplicate', blocked['effect'] is None and blocked['state']['notice_error'])
        ordinary = event(manual['state'], 'mcp_client_review', dict(tool='other', arguments='{}'))
        check('a different exact tool remains available on the same server', ordinary['state']['dialog'] == 'mcp_client_review')
        server_list = event(newly_ambiguous['state'], 'mcp_client_connections')
        adding = event(server_list['state'], 'mcp_client_new')
        second = event(adding['state'], 'mcp_client_save', dict(endpoint=f'http://127.0.0.1:{peer.server_port}/other', key='second-test', access=True))
        manual = event(second['state'], 'mcp_client_call')
        usable = event(manual['state'], 'mcp_client_review', dict(tool='original', arguments='{}'))
        check('a different saved connection can independently use the same exact name', usable['state']['dialog'] == 'mcp_client_review')
        server_list = event(second['state'], 'mcp_client_connections')
        selected_server = event(server_list['state'], 'mcp_client_select:0')
        restored = event(selected_server['state'], 'resume')
        manual = event(restored['state'], 'mcp_client_call')
        blocked = event(manual['state'], 'mcp_client_review', dict(tool='original', arguments='{}'))
        check('switching away and back and restarting retain the duplicate protection', blocked['effect'] is None and blocked['state']['notice_error'])
        mode = 'original'
        clean, _ = exchange(start(blocked['state']))
        selected = event(clean['state'], 'mcp_tool:0')
        review = event(selected['state'], 'mcp_client_review', dict(arguments='{}'))
        check('a complete valid refresh makes the name available again', review['state']['dialog'] == 'mcp_client_review')
        mode = 'duplicate-chain'
        pending, _ = exchange(start(failed['state']))
        collision, _ = exchange(pending)
        check('repeated names with different schemas cannot replace the last complete catalog', names(collision['state']) == ['original'])
        check('cross-page duplicate settles only the discovery with an explicit diagnostic', collision['state']['notice_error'] and 'tool name' in collision['state']['notice'].lower() and not collision['state']['busy'] and collision['state'].get('mcp_client_listing') is None)
        mode = 'duplicate-page'
        collision, _ = exchange(start(collision['state']))
        check('repeated identical entries within one page are also refused', names(collision['state']) == ['original'] and collision['state']['notice_error'])
        mode = 'case-sensitive'
        first, _ = exchange(start(collision['state']))
        distinct, _ = exchange(first)
        check('case-distinct names have separate wire identities', names(distinct['state']) == ['first', 'First'])
        mode = 'success'
        pending, first_reply = exchange(start(prior))
        check('cursor sent on the actual continuation request', json.loads(pending['effect']['wire'].split('\r\n\r\n')[1])['params'] == {'cursor': 'second-page'})
        stale = event(pending['state'], 'network_result', first_reply)
        check('stale first callback does not disturb the active continuation', stale['state'] == pending['state'] and stale['effect'] is None)
        done, _ = exchange(pending)
        check('both pages replace the prior catalog together', names(done['state']) == ['first', 'second'] and not done['state']['busy'])
        check('the finished list and UI expose both current tools', all(n in done['state']['mcp_client_tools'] and n in json.dumps(done['ui']['modal']) for n in ('first', 'second')))
        pending, _ = exchange(start(done['state']))
        cancelled = event(pending['state'], 'cancel')
        check('cancel keeps published list and drops the pending catalog', names(cancelled['state']) == ['first','second'] and cancelled['state'].get('mcp_client_listing') is None)
        resumed = event(pending['state'], 'resume')
        check('restart keeps published list and drops the pending catalog', names(resumed['state']) == ['first','second'] and resumed['state'].get('mcp_client_listing') is None)
        stale = event(resumed['state'], 'network_result', late)
        check('old response after restart cannot publish or dispatch', names(stale['state']) == ['first','second'] and stale['effect'] is None)
        mode = 'empty'
        empty, _ = exchange(start(resumed['state']))
        check('a successful empty catalog replaces the last complete list', names(empty['state']) == [] and not empty['state']['busy'])
        check('empty discovery leaves no staged pages', empty['state'].get('mcp_client_listing') is None)
        report = dict(passed=len(checks), checks=checks, requests=len(requests))
        (ROOT/'output/checks').mkdir(parents=True, exist_ok=True)
        (ROOT/'output/checks/mcp-catalog.json').write_text(json.dumps(report, indent=2)+'\n')
        print(json.dumps(report))
    finally:
        process.terminate()
        process.wait(timeout=5)
        peer.shutdown()
        worker.join(timeout=5)
