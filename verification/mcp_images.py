"""Exercise reviewed MCP image results through the actual local HTTP and media paths."""
import base64
import copy
import hashlib
import http.server
import json
import os
import pathlib
import socket
import struct
import subprocess
import tempfile
import threading
import zlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
checks, requests = [], []
def check(label, value):
    assert value, label
    checks.append(label)
def png_chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
png = (b'\x89PNG\r\n\x1a\n' + png_chunk(b'IHDR', struct.pack('>IIBBBBB', 1, 1, 8, 2, 0, 0, 0))
       + png_chunk(b'IDAT', zlib.compress(b'\0\xff\0\0')) + png_chunk(b'IEND', b''))
encoded = base64.b64encode(png).decode()
# 1×1 baseline JPEG encoded with ImageMagick from a solid red pixel.
jpeg_encoded = (
    '/9j/4AAQSkZJRgABAQAAAAAAAAD/2wBDAAUDBAQEAwUEBAQFBQUGBwwIBwcHBw8LCwkMEQ8SEhEPERETFhwXExQaFRERGCEY'
    'Gh0dHx8fExciJCIeJBweHx7/2wBDAQUFBQcGBw4ICA4eFBEUHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4e'
    'Hh4eHh4eHh4eHh4eHh7/wAARCAABAAEDAREAAhEBAxEB/8QAFAABAAAAAAAAAAAAAAAAAAAAB//EABQQAQAAAAAAAAAAAAAA'
    'AAAAAAD/xAAVAQEBAAAAAAAAAAAAAAAAAAAHCP/EABQRAQAAAAAAAAAAAAAAAAAAAAD/2gAMAwEAAhEDEQA/ACwdLLf/2Q=='
)
jpeg = base64.b64decode(jpeg_encoded, validate=True)
def image(data=encoded, mime='image/png'):
    return dict(type='image', data=data, mimeType=mime)
text_item = dict(type='text', text='Keep this real peer text.')
result = dict(content=[text_item, image(), image(mime='image/jpeg'),
                       image(encoded+'trailing'), image(mime='image/gif'),
                       dict(type='resource_link', name='no fetch', uri='https://example.invalid/file')])

class Peer(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass
    def do_POST(self):
        assert self.path == '/mcp' and self.headers['Authorization'] == 'Bearer image-test'
        msg = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        method = msg['method']
        requests.append(method)
        code, data = 200, b''
        if method == 'notifications/initialized':
            code = 202
        else:
            if method == 'initialize':
                body = dict(protocolVersion='2025-11-25', capabilities={'tools': {}},
                            serverInfo={'name': 'Image peer', 'version': '1'})
            else:
                assert method == 'tools/call' and msg['params'] == {'name': 'image', 'arguments': {}}
                body = result
            data = json.dumps(dict(jsonrpc='2.0', id=msg['id'], result=body)).encode()
        self.send_response(code)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

with tempfile.TemporaryDirectory() as directory, http.server.ThreadingHTTPServer(('127.0.0.1', 0), Peer) as peer:
    root = pathlib.Path(directory)
    thread = threading.Thread(target=peer.serve_forever, daemon=True)
    thread.start()
    controller = os.environ.get('RLLM_CONTROLLER', str(ROOT/'output/headless/RLLM'))
    proc = subprocess.Popen([controller, '--jsonl'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    def ask(payload):
        proc.stdin.write(json.dumps(payload)+'\n'); proc.stdin.flush()
        return json.loads(proc.stdout.readline())
    def event(state, action, inputs=None, workspace=root):
        reply = ask(dict(op='event', root=str(workspace), state=state, action=action, inputs=inputs or {}))
        assert 'value' in reply, reply
        return reply['value']
    def call(state):
        d = event(state, 'mcp_client_call')
        d = event(d['state'], 'mcp_client_review', {'tool': 'image', 'arguments': '{}'})
        check('review sends no request before approval', d['effect'] is None)
        d = event(d['state'], 'mcp_client_approve')
        for _ in range(3):
            e = d['effect']; assert e['kind'] == 'network', d['state']['notice']
            with socket.create_connection((e['host'], e['port']), timeout=5) as sock:
                sock.sendall(e['wire'].encode())
                raw = b''.join(iter(lambda: sock.recv(65536), b''))
            d = event(d['state'], 'network_result', {'purpose':e['purpose'], 'data':base64.b64encode(raw).decode()})
        return d
    def buttons(d):
        return [b['event'].rsplit(':', 1)[0] if b['event'].startswith('mcp_image_save:') else b['event']
                for b in d['ui']['modal']['buttons']]
    def save_action(d, index):
        return f"mcp_image_save:{index}:{d['state']['mcp_image_result']['id']}"
    def files():
        return sorted((root/'service-files').glob('*'))
    try:
        d = event(None, 'resume')
        d = event(d['state'], 'mcp_client_settings')
        d = event(d['state'], 'mcp_client_save', dict(endpoint=f'http://127.0.0.1:{peer.server_port}/mcp', key='image-test', access=True))
        d = call(d['state'])
        check('manual MCP result exposes supported image controls', {'mcp_image_save:1','mcp_image_save:2','mcp_image_save:3'} <= set(buttons(d)))
        check('mixed result preserves exact typed payload and text', json.loads(d['state']['output']) == result)
        check('network callback does not write image bytes', not files() and d['effect'] is None)
        check('unsupported MIME and resource URI do not gain save controls', 'mcp_image_save:4' not in buttons(d) and 'mcp_image_save:5' not in buttons(d))
        raw = event(d['state'], 'mcp_image_raw')
        check('raw details expose exact typed result without burying image controls', raw['ui']['modal']['fields'][0]['value'] == d['state']['output']
              and 'mcp_image_back' in buttons(raw) and all(f['value'] != d['state']['output'] for f in d['ui']['modal']['fields']))
        blocked = event(raw['state'], save_action(d, 1))
        check('hidden save action is refused in the raw details view', blocked['state'].get('notice_error') and not files())
        d = event(raw['state'], 'mcp_image_back')
        closed = event(d['state'], 'close')
        check('closed result does not accept a stale save action', event(closed['state'], save_action(d, 1))['state'].get('notice_error') and not files())
        for index in (0,2,3,4,5,99):
            bad = event(d['state'], save_action(d, index))
            check(f'image selection {index} refuses without writing', bad['state'].get('notice_error') and bad['effect'] is None and not files())
            check(f'image selection {index} preserves exact result', bad['state']['output'] == d['state']['output'])
        tampered = copy.deepcopy(d['state']); tampered['output'] += ' '
        bad = event(tampered, save_action(d, 1))
        check('changed result cannot be saved as the owned image', bad['state'].get('notice_error') and not files())
        other = root/'other'; other.mkdir()
        bad = event(d['state'], save_action(d, 1), workspace=other)
        check('changed workspace cannot adopt the earlier image', bad['state'].get('notice_error') and not list((other/'service-files').glob('*')))
        saved = event(d['state'], save_action(d, 1))
        artifact = saved['state']['service_artifact']
        check('explicit save retains exact original image bytes', (root/artifact['path']).read_bytes() == png and artifact['sha256'] == hashlib.sha256(png).hexdigest())
        check('image saved locally with MIME and source provenance', artifact['mime']=='image/png' and artifact['kind']=='image' and artifact['route']=='mcp')
        check('image is visible in the conversation artifact list', any(a['path']==artifact['path'] for a in saved['ui']['service_artifacts']))
        check('existing preview and export actions are offered', {'media_preview','media_export'} <= set(buttons(saved)))
        preview = event(saved['state'], 'media_preview')['effect']
        check('preview uses the existing native-owned path and record', preview['kind']=='media_preview' and preview['record']==artifact and pathlib.Path(preview['path']).read_bytes()==png)
        verified = ask(dict(preview, op='media_verify'))
        check('native preview readback verifies original bytes', verified['value']['path']==preview['path'])
        export_state = copy.deepcopy(saved['state'])
        export_state.setdefault('drive', {})['phone_default'] = True
        export_state['phone_uri'] = 'content://fixture/tree/owned'
        exported = event(export_state, 'media_export')['effect']
        check('native export retains detected PNG and existing transfer contract', exported['prepare']['name'].endswith('.png') and exported['prepare']['mime']=='image/png')
        again = event(saved['state'], save_action(d, 1))
        check('repeat save reuses the same still-verified artifact', again['state']['service_artifact']['path']==artifact['path'] and len(files())==1)
        (root/artifact['path']).write_bytes(bytes([png[0]^1])+png[1:])
        check('same-size tamper fails native preview verification', 'error' in ask(dict(preview, op='media_verify')))
        result = dict(content=[text_item, image()], isError=True)
        error = call(saved['state'])
        check('reported tool error keeps the raw result but has no image save', json.loads(error['state']['output']) == result and not any(a.startswith('mcp_image') for a in buttons(error)))
        result = dict(content=[text_item])
        ordinary = call(error['state'])
        check('ordinary text tool result remains in its original UI', ordinary['state']['dialog']=='output' and json.loads(ordinary['state']['output'])==result)
        result = dict(content=[text_item] + [image()]*8 + [image(jpeg_encoded, 'image/jpeg')])
        paged = call(ordinary['state'])
        check('first image page has eight original result slots', 'mcp_image_next' in buttons(paged) and 'mcp_image_save:8' in buttons(paged)
              and 'mcp_image_save:9' not in buttons(paged))
        page_two = event(paged['state'], 'mcp_image_next')
        check('second image page preserves the original image index', 'mcp_image_save:9' in buttons(page_two)
              and 'mcp_image_previous' in buttons(page_two) and 'mcp_image_next' not in buttons(page_two))
        page_one = event(page_two['state'], 'mcp_image_previous')
        check('previous page restores the original controls', buttons(page_one) == buttons(paged))
        stale = event(page_two['state'], save_action(d, 1))
        check('earlier result identity cannot save from a new result', stale['state'].get('notice_error') and len(files()) == 1)
        jpeg_saved = event(page_two['state'], save_action(page_two, 9))
        jpeg_artifact = jpeg_saved['state']['service_artifact']
        check('JPEG is saved exactly with its own detected format', (root/jpeg_artifact['path']).read_bytes() == jpeg
              and jpeg_artifact['sha256'] == hashlib.sha256(jpeg).hexdigest() and jpeg_artifact['mime'] == 'image/jpeg'
              and jpeg_artifact['name'].endswith('.jpg'))
        check('all image actions were local; only reviewed calls hit the peer', requests == ['initialize','notifications/initialized','tools/call']*4)
        report = dict(passed=len(checks), checks=checks, requests=len(requests))
        (ROOT/'output/checks').mkdir(parents=True, exist_ok=True)
        (ROOT/'output/checks/mcp-images.json').write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps(report))
    finally:
        proc.terminate(); proc.wait(timeout=5); peer.shutdown(); thread.join(timeout=5)
