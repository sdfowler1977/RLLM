"""Exercise model-directed image evidence through the local HTTP and media paths."""
import base64
from collections import deque
import hashlib
import http.server
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import threading
import zlib

project = Path(__file__).resolve().parents[1]
checks, mcp_requests, model_requests = [], [], []
model_replies = deque()


def check(label, value):
    assert value, label
    checks.append(label)


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


png = (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 1, 1, 8, 2, 0, 0, 0))
       + chunk(b'IDAT', zlib.compress(b'\0\x11\x22\x33')) + chunk(b'IEND', b''))
encoded = base64.b64encode(png).decode()
image = dict(type='image', data=encoded, mimeType='image/png')
peer_text = dict(type='text', text='Actual image peer observation')
mixed = dict(content=[peer_text, image, dict(image, mimeType='image/gif')], structuredContent={'frame': 1})
result = mixed


class Peer(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        status = 200
        if self.path == '/mcp':
            assert self.headers['Authorization'] == 'Bearer image-test'
            method = request['method']
            mcp_requests.append(method)
            if method == 'initialize':
                answer = dict(protocolVersion='2025-11-25', capabilities={'tools': {}},
                              serverInfo={'name': 'Image peer', 'version': '1'})
            elif method == 'notifications/initialized':
                status = 202
            elif method == 'tools/list':
                answer = {'tools': [{'name': 'image', 'inputSchema': {'type': 'object'}}]}
            else:
                assert method == 'tools/call' and request['params'] == {'name': 'image', 'arguments': {}}
                answer = result
            body = b'' if status == 202 else json.dumps(dict(jsonrpc='2.0', id=request['id'], result=answer)).encode()
        else:
            assert self.path in ('/primary', '/child')
            model_requests.append((self.path, request))
            response = model_replies.popleft()
            body = json.dumps(dict(choices=[dict(message=dict(content=response), finish_reason='stop')])).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


with tempfile.TemporaryDirectory() as directory, http.server.ThreadingHTTPServer(('127.0.0.1', 0), Peer) as peer:
    root = Path(directory)
    thread = threading.Thread(target=peer.serve_forever, daemon=True)
    thread.start()
    controller = os.environ.get('RLLM_CONTROLLER', str(project/'output/headless/RLLM'))
    proc = subprocess.Popen([controller, '--jsonl'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

    def call(value):
        proc.stdin.write(json.dumps(value)+'\n')
        proc.stdin.flush()
        return json.loads(proc.stdout.readline())

    def event(state, action, inputs=None, workspace=root):
        reply = call(dict(op='event', root=str(workspace), state=state, action=action, inputs=inputs or {}))
        assert 'value' in reply, reply
        return reply['value']

    def socket_result(d, response=None):
        effect = d['effect']
        assert effect and effect['kind'] == 'network', d['state']['notice']
        if response is not None:
            model_replies.append(response)
        with socket.create_connection((effect['host'], effect['port']), timeout=5) as sock:
            sock.sendall(effect['wire'].encode())
            raw = b''.join(iter(lambda: sock.recv(65536), b''))
        inputs = dict(purpose=effect['purpose'], data=base64.b64encode(raw).decode())
        if effect['purpose'] == 'model':
            inputs['model_ticket'] = d['state']['model_expected']
        return inputs

    def exchange(d, response=None):
        return event(d['state'], 'network_result', socket_result(d, response))

    def proposal(d, tool, args):
        d = exchange(d, json.dumps(dict(tool=tool, args=args)))
        assert d['state']['pending']['tool'] == tool and d['effect'] is None, d['state'].get('notice')
        return d

    def configured():
        d = event(None, 'resume')
        d = event(d['state'], 'mcp_client_settings')
        endpoint = f'http://127.0.0.1:{peer.server_port}'
        d = event(d['state'], 'mcp_client_save', dict(endpoint=endpoint+'/mcp', key='image-test', access=True))
        st = d['state']
        st['profiles'] = [dict(name=name, model=name, endpoint=endpoint+'/'+name, plain=True,
                               protocol='chat', stream=False, tool_format='json') for name in ('primary', 'child')]
        st['agents'] = [dict(name='Parent', model='0', tools='mcp,delegate', subagents=[1]),
                        dict(name='Child', model='1', tools='mcp')]
        return st

    def start(st, child=False):
        d = event(st, 'send', {'draft':'Request the reviewed MCP image.'})
        if child:
            d = proposal(d, 'delegate', {'agent':1, 'task':'Retrieve the reviewed image result.'})
            d = event(d['state'], 'approve')
        d = proposal(d, 'mcp', {'name':'image', 'arguments':{}})
        check('model proposal waits for approval', d['effect'] is None)
        d = event(d['state'], 'approve')
        return exchange(exchange(d))

    def controls(d):
        return [b['event'] for b in d['ui']['modal']['buttons']]

    def review(d):
        d = event(d['state'], 'chat_actions')
        action = 'mcp_image_review'
        assert action in controls(d), controls(d)
        return event(d['state'], action)

    def active(d):
        return next(c for c in d['state']['chats'] if c['id'] == d['state']['active'])

    def followup(st, child=False):
        pending = start(st, child)
        callback = socket_result(pending)
        d = event(pending['state'], 'network_result', callback)
        wire = d['effect']['wire']
        check('typed image base64 never enters model continuation', encoded not in wire and 'dataOmitted' in wire)
        repeated = event(d['state'], 'network_result', callback)
        check('duplicate result does not install a new continuation or result', repeated['effect'] is None
              and active(repeated)['mcp_image_evidence'] == active(d)['mcp_image_evidence'])
        check('text and structured evidence continue on the owning model route', 'Actual image peer observation' in wire
              and 'frame' in wire and d['state']['request_index'] == int(child))
        check('callback offers review without creating a file', not list((root/'service-files').glob('*')))
        d = exchange(d, 'Image result received as evidence.')
        if child:
            check('child continuation returns to the original parent', d['state']['request_index'] == 0 and encoded not in d['effect']['wire'])
            d = exchange(d, 'Parent received the child finding.')
        return d

    try:
        d = followup(configured())
        original = active(d)['id']
        switched = event(d['state'], 'new')
        check('another conversation does not expose the first result', 'mcp_image_review' not in controls(event(switched['state'], 'chat_actions')))
        moved = root/'moved'
        moved.mkdir()
        check('another workspace cannot expose the result', 'mcp_image_review' not in controls(event(d['state'], 'chat_actions', workspace=moved)))
        d = review(event(switched['state'], 'history:'+original))
        raw = event(d['state'], 'mcp_image_raw')
        check('local details preserve the exact typed and structured result', json.loads(raw['ui']['modal']['fields'][0]['value']) == mixed)
        d = event(raw['state'], 'mcp_image_back')
        offered = [a for a in controls(d) if a.startswith('mcp_image_save:')]
        check('only a supported image offers an explicit save', len(offered) == 1 and offered[0].startswith('mcp_image_save:1:'))
        # Child execution uses a separate empty workspace media directory before the save.
        child = followup(configured(), True)
        child_review = review(child)
        check('child result is visible in the originating conversation', any(a.startswith('mcp_image_save:1:') for a in controls(child_review)))
        d = event(d['state'], offered[0])
        record = d['state']['service_artifact']
        check('explicit save uses exact image and existing conversation artifact', (root/record['path']).read_bytes() == png
              and record['sha256'] == hashlib.sha256(png).hexdigest() and active(d)['service_artifacts'][-1] == record)
        preview = event(d['state'], 'media_preview')['effect']
        check('native preview verifier reads the same saved bytes', call(dict(preview, op='media_verify'))['value']['path'] == preview['path'])
        result = dict(content=[peer_text, image], isError=True)
        d = exchange(start(configured()))
        check('tool-error text continues without image base64', 'isError' in d['effect']['wire'] and encoded not in d['effect']['wire'])
        error = review(exchange(d, 'The tool reported an error.'))
        check('tool errors allow raw inspection without an image save', not any(a.startswith('mcp_image_save:') for a in controls(error)))
        result = dict(content=[peer_text])
        d = exchange(start(configured()))
        check('plain MCP output keeps its original text and creates no image review', 'Actual image peer observation' in d['effect']['wire']
              and 'mcp_image_review' not in controls(event(exchange(d, 'Text received.')['state'], 'chat_actions')))
        large_png = png[:33] + chunk(b'tEXt', b'Note\0' + b'x' * 240_000) + png[33:]
        large_image = dict(image, data=base64.b64encode(large_png).decode())
        result = dict(content=[peer_text, large_image])
        pending = start(configured())
        callback = socket_result(pending)
        pending['state']['capacity_fixture'] = 'x' * (6 * 1024 * 1024 - 200_000)
        limited = event(pending['state'], 'network_result', callback)
        check('local retention capacity cannot block the text continuation', limited['effect']['purpose'] == 'model'
              and 'Actual image peer observation' in limited['effect']['wire'] and large_image['data'] not in limited['effect']['wire']
              and not active(limited).get('mcp_image_evidence') and 'could not be retained' in limited['state']['notice'])
        result = mixed
        pending = start(configured())
        callback = socket_result(pending)
        pending['state']['capability_mode'] = 'local'
        local = event(pending['state'], 'network_result', callback)
        check('paused model leaves the already received image available locally', local['effect'] is None
              and local['state']['notice_error'] and 'Remote model requests are paused' in local['state']['notice']
              and any(a.startswith('mcp_image_save:1:') for a in controls(review(local))))
        discovered = exchange(exchange(exchange(event(local['state'], 'mcp_client_list'))))
        check('a stopped model continuation cannot gate ordinary MCP discovery', discovered['state']['dialog'] == 'mcp_tools'
              and [entry['name'] for entry in discovered['state']['mcp_client_catalog']] == ['image'])
        pending = start(configured())
        late = socket_result(pending)
        wrong_root = event(pending['state'], 'network_result', late, workspace=moved)
        check('in-flight MCP result is refused in a different workspace', wrong_root['effect'] is None
              and not active(wrong_root).get('mcp_image_evidence'))
        stopped = event(pending['state'], 'cancel')
        d = event(stopped['state'], 'network_result', late)
        check('cancelled callbacks cannot continue or install image evidence', d['effect'] is None
              and 'mcp_image_review' not in controls(event(d['state'], 'chat_actions')))
        check('exact calls occurred through the loopback MCP and both model routes', mcp_requests == (['initialize','notifications/initialized','tools/call']*6
              + ['initialize','notifications/initialized','tools/list','initialize','notifications/initialized','tools/call'])
              and '/child' in [route for route, _ in model_requests])
        report = dict(passed=len(checks), checks=checks, mcp_requests=len(mcp_requests), model_requests=len(model_requests))
        (project/'output/checks').mkdir(parents=True, exist_ok=True)
        (project/'output/checks/mcp-continuation.json').write_text(json.dumps(report, indent=2)+'\n')
        print(json.dumps(report))
    finally:
        proc.terminate()
        proc.wait(timeout=5)
        peer.shutdown()
        thread.join(timeout=5)
