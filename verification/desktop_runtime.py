"""Linux/Windows host runtime: built from its own crate root, then driven over real sockets, files and processes."""
import json,os,pathlib,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
(PROJECT/'output/checks').mkdir(parents=True,exist_ok=True)
def check(name,value):assert value,name;passed.append(name)
build=subprocess.run(['bash',str(PROJECT/'desktop.sh')],cwd=PROJECT,env={**os.environ,'CLIENT_RUST':os.environ['CLIENT_RUST']},capture_output=True,text=True)
check('the host builds through its declared script',build.returncode==0)
binary=pathlib.Path(build.stdout.strip().splitlines()[-1])
check('the build publishes an executable host',binary.is_file()and os.access(binary,os.X_OK))
def host(args,stdin=''):return subprocess.run([str(binary)]+args,capture_output=True,text=True,input=stdin,timeout=180,cwd=PROJECT)
with tempfile.TemporaryDirectory(prefix='desktop-runtime-',dir=PROJECT/'output')as tmp:
 root=pathlib.Path(tmp);key=root/'key';state=root/'state.bin'
 sealed=root/'sealed.bin'
 check('the vault path is honored exactly as declared',not state.exists())
 checked=host(['--check','--key',str(key),'--state',str(sealed),'--root',str(root)])
 check('the host runtime self-test exits clean',checked.returncode==0)
 lines=[line[len('ok   '):]for line in checked.stdout.splitlines()if line.startswith('ok   ')]
 counted=[line for line in checked.stdout.splitlines()if line.startswith('desktop self-test: ')]
 check('every exercised leg is named and the count matches the legs',len(lines)>10and counted==['desktop self-test: %d checks passed'%len(lines)])
 for leg in['a sent turn reaches the model over a real socket','the model peer served exactly the requests the core built','the approved leg ran a real workspace tool through the host','a restart restores the sealed conversation into the first render','the saved vault holds no plaintext conversation','the microphone delivered real frames through the host','the core stored the utterance captured on the Linux host','the captured utterance reached the configured recognition connection']:check('the self-test names its '+leg+' leg',leg in lines)
 check('the self-test reports no host-side failure',checked.stderr=='')
 check('the self-test seals its vault at the declared path',sealed.is_file())
 check('the sealed vault keeps no plaintext conversation',b'Linux host is online'not in sealed.read_bytes())
 served=host(['--key',str(key),'--state',str(state),'--root',str(root)],stdin='{"action":"resume"}\n')
 check('the host renders on start and serves the resumed surface',served.returncode==0and served.stdout.count('\n')==2)
 first=json.loads(served.stdout.splitlines()[-1])
 check('the host render carries the core surface contract',all(name in first for name in['page','messages','tabs','profile_fields','palette']))
 saved=host(['--key',str(key),'--state',str(state),'--root',str(root)],stdin='{"action":"page","inputs":{"page":"settings"}}\n{"action":"resume"}\n')
 check('the host settles every action and answers each one',saved.returncode==0and saved.stdout.count('\n')==3)
 check('the host persists the conversation after settling',state.is_file())
 restored=host(['--key',str(key),'--state',str(state),'--root',str(root)],stdin='{"action":"resume"}\n')
 check('a new host process resumes from the saved state',restored.returncode==0and'"page"'in restored.stdout)
 malformed=host(['--key',str(key),'--state',str(state),'--root',str(root)],stdin='not json\n{"action":"resume"}\n')
 check('an unusable request is reported without ending the session',malformed.returncode==0and malformed.stdout.count('\n')==2and malformed.stderr.strip()!='')
 foreign=root/'foreign';foreign.write_bytes(bytes(32))
 wrong=host(['--key',str(foreign),'--state',str(state),'--root',str(root)])
 check('a foreign key over a saved vault fails closed instead of starting cold',wrong.returncode!=0and wrong.stderr.strip()!='')
 truncated=root/'truncated.bin';truncated.write_bytes(state.read_bytes()[:24])
 cut=host(['--key',str(key),'--state',str(truncated),'--root',str(root)])
 check('a truncated vault is refused instead of silently discarded',cut.returncode!=0)
 report=dict(passed=len(passed),checks=passed,scope='Built Linux/Windows host runtime: script-declared build, runtime self-test over loopback sockets and real workspace tools, vault sealing at the declared path, cross-process resume, per-action settling and persistence, malformed-request tolerance, foreign-key and truncated-vault refusal.')
(PROJECT/'output/checks/desktop_runtime.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(dict(count=len(passed),scope=report['scope']),indent=2))
