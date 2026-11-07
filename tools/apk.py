"""Inspect actual signed APK bytes and retain build identity for packaging."""
from pathlib import Path
import argparse,hashlib,json,os,re,struct,subprocess,zipfile
root=Path(__file__).resolve().parents[1];p=argparse.ArgumentParser();p.add_argument('apk',type=Path);p.add_argument('--version',required=True,type=int);args=p.parse_args();apk=args.apk.resolve();tools=Path(os.environ['CLIENT_TOOLS']);env=dict(os.environ,PATH=os.environ['CLIENT_JDK']+'/bin:'+os.environ['PATH'])
from signature import verify
verify(apk)
build_inputs={}
for line in (apk.parent/'build-inputs.sha256').read_text().splitlines():
 digest,name=line.split('  ',1);source=Path(name);relative=source.resolve().relative_to(root).as_posix();assert re.fullmatch('[0-9a-f]{64}',digest)and source.is_file()and hashlib.sha256(source.read_bytes()).hexdigest()==digest,relative;build_inputs[relative]=digest
current={p.relative_to(root).as_posix()for directory in ['core','platform','res','notices']for p in(root/directory).rglob('*')if p.is_file()}|{'AndroidManifest.xml','package.sh','tools/alignment.py'};assert set(build_inputs)==current,'Build input inventory changed after compilation'

badging=subprocess.check_output([str(tools/'aapt'),'dump','badging',str(apk)],text=True);assert "package: name='com.local.client.lab'"in badging and "versionCode='"+str(args.version)+"'"in badging and "application-label:'RLLM'"in badging and "sdkVersion:'26'"in badging
subprocess.run([str(tools/'zipalign'),'-c','-P','16','-v','4',str(apk)],check=True,capture_output=True);native=[]
manifest=subprocess.check_output([str(tools/'aapt'),'dump','xmltree',str(apk),'AndroidManifest.xml'],text=True);resource_id=re.search(r'android:networkSecurityConfig[^=]*=@(0x[0-9a-f]+)',manifest)[1];resources=subprocess.check_output([str(tools/'aapt'),'dump','resources',str(apk)],text=True);assert re.search(r'\bresource\s+'+re.escape(resource_id)+r'\s+\S+:xml/network:',resources)
assert 'android.permission.MODIFY_AUDIO_SETTINGS'in manifest and 'android.permission.RECORD_AUDIO'in manifest
assert all(name in manifest for name in ['android.permission.FOREGROUND_SERVICE_MICROPHONE','android.permission.FOREGROUND_SERVICE_MEDIA_PLAYBACK','android.permission.WAKE_LOCK','com.local.client.VoiceSession'])
voice_service=next(block for block in re.split(r'(?=      E: service )',manifest)if 'com.local.client.VoiceSession'in block);voice_service=voice_service.split('      E: service ',1)[-1].split('      E: activity ',1)[0]
assert 'android:exported(0x01010010)=(type 0x12)0x0'in voice_service and 'android:foregroundServiceType(0x01010599)=(type 0x11)0x82'in voice_service
network=subprocess.check_output([str(tools/'aapt'),'dump','xmltree',str(apk),'res/xml/network.xml'],text=True);assert network.count('E: base-config')==1 and network.count('E: trust-anchors')==1 and network.count('E: certificates')==2 and 'E: debug-overrides'not in network and all('A: src="'+name+'"'in network for name in ['system','user'])and network.count('A: overridePins=(type 0x12)0x0')==2 and 'A: cleartextTrafficPermitted=(type 0x12)0xffffffff'in network

with zipfile.ZipFile(apk)as z:
 assert z.testzip()is None
 for abi,machine in [('arm64-v8a',183),('x86_64',62)]:
  raw=z.read('lib/'+abi+'/libclient.so');assert raw[:6]==b'\x7fELF\x02\x01'and struct.unpack_from('<H',raw,18)[0]==machine;phoff=struct.unpack_from('<Q',raw,32)[0];ents,count=struct.unpack_from('<HH',raw,54);aligns=[struct.unpack_from('<Q',raw,phoff+i*ents+48)[0]for i in range(count)if struct.unpack_from('<I',raw,phoff+i*ents)[0]==1];assert aligns and all(x>=16384 for x in aligns)
  for marker in [b'Investigation is intrinsic',b'MODEL AND DELEGATION CONFIGURATION',b'Delegation result: blocked before start.',b'PE base-relocation declarations',b'Returning the operation error to the requesting model']:assert marker in raw,(abi,marker)
  for marker in [b'GET routes cannot upload a selected file',b'Input selection is no longer owned',b'File upload is no longer owned',b'Media storage reservation exceeded',b'Artifact or status response exceeds',b'Service response did not contain a complete generated artifact']:assert marker in raw,(abi,marker)
  # Match emitted effects, state and PCM receipts; optimized input comparisons
  # need not retain a contiguous callback-name literal in either native image.
  for marker in [b'voice-onset:',b'voice_interrupt_listen',b'voice_interruption',b'voice_watch_expected',b'Spoken interruption ownership changed',b'Interrupt replies by speaking']:assert marker in raw,(abi,marker)
  for marker in [b'Voice conversation stopped after an audio route or focus interruption; review voice to start again']:assert marker in raw,(abi,marker)
  for marker in [b'Waiting for voice request approval',b'resume_available',b'Voice request was not started; resume the microphone when ready']:assert marker in raw,(abi,marker)
  assert b'Voice request stopped; microphone paused until you resume it'in raw,abi
  assert b'Microphone paused while another operation is pending; resume it when finished'in raw,abi
  for marker in [b'Review proposed request',b'Review recovered request',b'Response details',b'Alternative source only',b'No current reply is eligible for request recovery']:assert marker in raw,(abi,marker)
  for marker in [b'Sign in with device code',b'Check authorization',b'Device authorization',b'Device verification URI contains private protocol fields']:assert marker in raw,(abi,marker)
  for marker in [b'voice_background',b'voice_lease_ms',b'Continue voice outside the app']:assert marker in raw,(abi,marker)
  for marker in [b'Document providers',b'Provider document selected',b'cloud_release_result',b'Selected document grants removed']:assert marker in raw,(abi,marker)
  for marker in [b'Request framing is ambiguous; no TLS identity was applied',b'Request headers are incomplete; no TLS identity was applied']:assert marker in raw,(abi,marker)
  for marker in [b'Voice and microphone',b'Save microphone preferences',b'Review open microphone conversation',b'Key/token header active; username/password retained',b'Network storage access key exceeds8192bytes']:assert marker in raw,(abi,marker)
  for marker in [b'Network storage authorization',b'This network storage connection only; model credentials remain separate',b'Storage authorization active; manual credentials retained',b'Network storage authorization destination changed; sign in again',b'Network storage authorization mode must be a toggle']:assert marker in raw,(abi,marker)
  for marker in [b'scene_ticket',b'Static geometry preview',b'Scene viewport ownership changed',b'Scene source bytes changed while loading']:assert marker in raw,(abi,marker)
  for marker in [b'RLLM Workspace',b'ReverseEngineerAnything',b'reverse_engineer_anything']:assert marker not in raw,(abi,marker)
  native.append(dict(abi=abi,machine=machine,load_alignments=aligns,bytes=len(raw)))
with zipfile.ZipFile(apk)as z:
 dex=z.read('classes.dex')
 for marker in [b'media_input_open',b'media_input_feed',b'media_input_finish',b'media_upload_read',b'upload_bytes',b'media_receipt']:assert marker in dex,marker
 for marker in [b'voice_watch',b'voice_watch_start',b'voice_watch_unavailable',b'voice_interrupt_listen',b'Landroid/media/audiofx/AcousticEchoCanceler;',b'echo_active']:assert marker in dex,marker
 for marker in [b'Lcom/local/client/AudioRoute;',b'setWillPauseWhenDucked',b'voice_route_lost',b'android.media.AUDIO_BECOMING_NOISY',b'resume_available']:assert marker in dex,marker
 for marker in [b'Lcom/local/client/VoiceSession;',b'Landroid/os/PowerManager$WakeLock;',b'voiceSessionStopped',b'startCapture']:assert marker in dex,marker
 for marker in [b'Lcom/local/client/DocumentAccess;',b'completeCloudSelection',b'selectedDocumentAccess',b'cloud_release_result']:assert marker in dex,marker
 for marker in [b'microphone_permissions',b'microphone_granted',b'android.settings.APPLICATION_DETAILS_SETTINGS']:assert marker in dex,marker
 for marker in [b'com.google.android.apps.docs',b'Choose Google Drive',b'com.google']:assert marker not in dex,marker
 for marker in [b'Lcom/local/client/SceneView;',b'scene_frame',b'scene_close',b'Rotate left',b'Reset view']:assert marker in dex,marker
record=dict(version=args.version,path=str(apk),bytes=apk.stat().st_size,sha256=hashlib.sha256(apk.read_bytes()).hexdigest(),native=native,label='RLLM',package='com.local.client.lab',signature='v2/v3; one pinned current signer',zip_crc='passed',build_inputs=build_inputs,network_security=dict(path='res/xml/network.xml',resource_id=resource_id,trust_anchors=['system','user'],pin_overrides=False));out=root/'output/checks';out.mkdir(parents=True,exist_ok=True);(out/'apk.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record,indent=2))
