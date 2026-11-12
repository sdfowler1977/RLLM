"""Saved-state sealing is the core's own AES-256-GCM; the platform only holds the key."""
import base64,json,pathlib,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
(PROJECT/'output/checks').mkdir(parents=True,exist_ok=True)
def check(name,value):assert value,name;passed.append(name)
with tempfile.TemporaryDirectory(prefix='state-sealing-',dir=PROJECT/'output')as tmp:
 root=pathlib.Path(tmp);p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def send(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
 def op(v):
  r=send(v);assert'value'in r,r;return r['value']
 def failed(v):
  r=send(v);assert'error'in r,r;return r['error']
 key=base64.b64encode(bytes(range(32))).decode()
 other=base64.b64encode(bytes([255-i for i in range(32)])).decode()
 vault=json.dumps({'messages':[{'role':'user','content':'the exact conversation text that must not be stored in clear'}],'profiles':[{'name':'home','endpoint':'http://127.0.0.1:8080/v1'}]}).encode()
 def seal(data,key=key):return op(dict(op='state_seal',key=key,data=base64.b64encode(data).decode()))['sealed']
 def open_sealed(sealed,key=key):return op(dict(op='state_open',key=key,data=sealed))['data']
 sealed=seal(vault)
 check('a sealed vault opens again to the exact saved bytes',base64.b64decode(open_sealed(sealed))==vault)
 check('the sealed vault does not keep the conversation in clear text',b'the exact conversation text'not in base64.b64decode(sealed))
 check('the sealed vault is self-describing without a separate nonce field',len(base64.b64decode(sealed))==len(vault)+12+16)
 fresh=seal(vault)
 check('sealing the same bytes twice yields different ciphertext',fresh!=sealed and base64.b64decode(open_sealed(fresh))==vault and base64.b64decode(open_sealed(sealed))==vault)
 tampered=list(bytearray(base64.b64decode(sealed)));tampered[20]^=1
 check('a one-byte change in the ciphertext is refused',isinstance(failed(dict(op='state_open',key=key,data=base64.b64encode(bytes(tampered)).decode())),str))
 tampered=list(bytearray(base64.b64decode(sealed)));tampered[-1]^=1
 check('a one-byte change in the tag is refused',isinstance(failed(dict(op='state_open',key=key,data=base64.b64encode(bytes(tampered)).decode())),str))
 tampered=list(bytearray(base64.b64decode(sealed)));tampered[3]^=1
 check('a one-byte change in the nonce is refused',isinstance(failed(dict(op='state_open',key=key,data=base64.b64encode(bytes(tampered)).decode())),str))
 check('a vault from another key is refused',isinstance(failed(dict(op='state_open',key=other,data=sealed)),str))
 check('a truncated vault is refused',isinstance(failed(dict(op='state_open',key=key,data=sealed[:20])),str))
 check('an empty vault round-trips',base64.b64decode(open_sealed(seal(b'')))==b'')
 large=bytes((i%251)for i in range(200_000))
 check('a large saved state round-trips byte for byte',base64.b64decode(open_sealed(seal(large)))==large)
 for label,bad in [('a short key is refused',base64.b64encode(b'short').decode()),('a long key is refused',base64.b64encode(bytes(33)).decode()),('a non-base64 key is refused','not base64!')]:
  check(label,isinstance(failed(dict(op='state_seal',key=bad,data=base64.b64encode(b'vault').decode())),str))
 check('the sealed payload stays inside its declared size bound',isinstance(failed(dict(op='state_open',key=key,data=base64.b64encode(bytes(9*1024*1024)).decode())),str))
 check('sealing is reachable only through the declared operation',isinstance(failed(dict(op='state_seals',key=key,data=base64.b64encode(b'vault').decode())),str))
 p.stdin.close();p.wait(timeout=10)
result={'passed':passed,'scope':'Saved-state sealing through the shared core operation: exact round trip, no clear-text storage, self-describing framing, distinct ciphertext per seal, per-byte tamper refusal for nonce, ciphertext and tag, foreign-key and truncation refusal, empty and large payloads, key length and encoding refusal, declared size bound and unknown-operation refusal.'};(PROJECT/'output/checks/state-sealing.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
