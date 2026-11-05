"""Package exact signed inputs; incomplete compatibility remains explicit in candidates."""
from pathlib import Path
import argparse,datetime,hashlib,json,re,zipfile
from signature import verify
root=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--destination',type=Path,required=True);p.add_argument('--candidate',action='store_true',help='Retain the explicitly unavailable historical comparator in a signed candidate; never accepts a product check failure.');args=p.parse_args()
out=args.destination.resolve();out.mkdir(parents=True,exist_ok=True)
read=lambda path:json.loads((root/path).read_text())
apk=read('output/checks/apk.json');apk_bytes=Path(apk['path']).read_bytes()
assert len(apk_bytes)==apk['bytes'] and hashlib.sha256(apk_bytes).hexdigest()==apk['sha256']
for relative,digest in apk['build_inputs'].items():assert hashlib.sha256((root/relative).read_bytes()).hexdigest()==digest,relative
units=read('output/checks/selected-units.json');assert all(v['code']==0 for v in units)
unit_count=sum(int(re.search(r'(\d+) passed',v['summary'])[1])for v in units)
assert unit_count>=199
runs=read('output/checks/current-regression.json');assert len(runs)==40
failed=[v for v in runs if v['code']!=0]
if failed:assert args.candidate and failed==[dict(name='binary_flow',code=1)],failed
pairs=[('provider_process','current-results'),('network_drive','network-results'),('drive_ownership','drive-ownership'),('question_process','process-results'),('history','history-results'),('a2a','a2a'),('binary_flow','binary-flow'),('commands','commands'),('history_migration','history-migration'),('hosted_phases','hosted-phases'),('prompt_reference','prompt-reference'),('provider_evidence','provider-evidence'),('pe_declarations','pe-declarations'),('delegation_visibility','delegation-visibility'),('local_feedback','local-feedback'),('network_feedback','network-feedback'),('proposal_feedback','proposal-feedback'),('review_ownership','review-ownership'),('task_ownership','task-ownership'),('dav_listing','dav-listing'),('server_admission','server-admission'),('http_framing','http-framing'),('platform_resolution','platform-resolution'),('query_feedback','query-feedback'),('carry_flow','carry-flow'),('workspace_navigation','workspace-navigation'),('field_flow','field-flow'),('flags_flow','flags-flow'),('product_flow','product-flow'),('diagnostics','diagnostics')]+[(n,n)for n in ['services','media','jobs','voice','media_stream','media_upload','voice_interruption','scene','acp','oauth']]
checks={name:read('output/checks/'+file+'.json') for name,file in pairs}
if failed:assert len(checks['binary_flow'].get('unavailable',[]))==3 and checks['binary_flow']['retained_compatibility'] is None
counts={name:len(value['passed'])if isinstance(value['passed'],list)else value['passed'] for name,value in checks.items()}
for name in ['services','media','jobs','voice','media_stream','media_upload','voice_interruption','scene','acp','oauth']:assert counts[name]==len(checks[name]['checks'])
validation=dict(connected=dict(counts=counts,total_executed=sum(counts.values()),groups_passed=len(runs)-len(failed),groups=len(runs),unavailable=failed),selected_rust_cases=unit_count,selected_rust_runs=units,checks=checks,apk={k:v for k,v in apk.items()if k!='path'},evidence_owner='implementation owner; real local HTTP/file fixtures, compiled prior core and LLVM references',published_comparator=dict(version=1793900957,source_sha256='f9e7ef03ff78c661690dab6c5c063b594830ae01f756a64a07a2106048902484',method='Exact published shared core compiled locally; actual retained representations exercised through current query, native view and export'),flow_comparator=dict(version=1793395702,source_sha256='f9d0dac9e3e8ae02cf5e787ffa71871f7d1242020fa6c87a3740af5b2d827ab8',method='Actual prior core with thin original JSON-lines envelope'),authentication_comparator=dict(version=1793593529,source_sha256='af219d4d2269f8f9ef0b9b9656106a8eca5dedbcebc5ef95047216de0c26b289',available=not bool(failed),method='Genuine comparator required; no substitution or weakened historical reader fingerprint'),independent_acceptance=False,installed_device_tested=False,live_service_endpoints_tested=False,live_model_NAS_tested=False)
validation['upgrade_audit']=read('verification/UPGRADE-AUDIT.json')
validation['historical_full_rust_suite']=dict(source_version=1793837555,source_sha256='6a24fa55a6cd10e299c49655858772866fa849c3c37aa99bfd34772b49eabfdb',reexecuted_for_this_candidate=False,results=validation['upgrade_audit']['full_rust_suite'],scope='Preserved earlier full-suite outcome only. Current execution is the selected Rust cases and connected groups; excluded historical fixture scopes were not reexecuted.')
if (root/'output/checks/live-web.json').exists():validation['live_public_web']=read('output/checks/live-web.json')
version=apk['version'];stamp=Path(apk['path']).parent.name;assert re.fullmatch(r'\d{4}-\d\d-\d\d__\d\d-\d\d-\d\dZ',stamp)
summary='Returned GLB triangle geometry now has an owned interactive in-app software viewport with Rust accessor, sparse-data, hierarchy, transform, material and source validation. Native orbit, zoom and reset controls present actual depth-tested geometry. Unsupported scenes retain their original files for export. Optional foreground spoken interruption now uses explicit consent, device echo control and the same Rust PCM capture across playback interruption, preserving leading speech. Saved microphone review controls now reach their actual settings. Unsupported echo control retires only the optional watch; explicit Speak now and ordinary text remain available. Direct binary service submissions now stream complete 200/201 responses to owned files, with configured limits, exact statuses, scoped fallback and expiry cleanup. Accepted, partial, redirected, truncated or uncertain submissions are not replayed. Selected files up to 64 MiB now import and upload in bounded chunks through configurable binary, multipart and JSON requests, with file ownership, shared storage reservations and scoped GET upload refusal. Optional voice conversation now supports explicit Speak now interruption, immediate Stop for pending playback and stale callback isolation. Missing CPU metadata discovers bounded multicore host task concurrency. Optional generic S256 authorization includes owned automatic refresh and browser-flow recovery while retaining API-key settings. Read-only ACP v1 sessions share actual workspace operations. Configurable recognition/synthesis, image/video/scene/document routes, fallbacks, jobs, streamed artifacts, ordinary text, local work and captured primary/child returns remain part of the same app. Extra services remain optional.'
limits='Installed Android microphone/playback/UI/lifecycle acceptance, real user server endpoints and independent review remain unverified. Background listening, bundled neural speech recognition and a separate medical application are not implemented. Physical effectiveness of echo control and simultaneous capture/playback remain unverified on a device. The historical revision-two comparator is unavailable; its three compatibility checks remain open.' if failed else 'Installed Android microphone/playback/UI/lifecycle acceptance, real user server endpoints and independent review remain unverified. Background listening, bundled neural speech recognition and a separate medical application are not implemented. Physical effectiveness of echo control and simultaneous capture/playback remain unverified on a device.'
limits+=' Scene preview displays static triangle geometry with base and vertex colors. Textures, animation and physical material effects are not displayed. Skins, morph targets, transparency and required unsupported extensions refuse only preview; export remains available. GPU rendering and wider scene formats remain open.'
limits+=' The preserved earlier full Rust sweep had14 failing historical fixtures on both then-current and baseline sources. That full sweep was not rerun for this candidate because its historical fixture exclusions remain in force; it is not claimed as fresh current acceptance. Current selected cases and operation equivalents are reported separately.'
release=root/'release';release.mkdir(exist_ok=True)
(release/'RLLM.apk').write_bytes(apk_bytes);verify(release/'RLLM.apk');assert (release/'RLLM.apk').read_bytes()==apk_bytes
(release/'SHA256SUMS.txt').write_text(apk['sha256']+'  RLLM.apk\n')
(root/'verification/RESULTS.json').write_text(json.dumps(validation,indent=2)+'\n')
(release/'NOTES.md').write_text(f'# RLLM {version} — signed candidate\n\n{summary}\n\n{len(runs)-len(failed)}/{len(runs)} connected groups passed; {sum(counts.values())} executed checks and {unit_count} selected Rust cases. Incomplete groups are explicitly recorded in verification/RESULTS.json.\n\n{limits}\n\nPackage com.local.client.lab, Android 8.0+, ARM64 and x86-64; retained current signing identity, APK CRC, exact build inputs and 16 KiB alignment verified. APK {apk["bytes"]} bytes, SHA256 {apk["sha256"]}.\n')
(root/'docs/STATUS.md').write_text(f'# Current implementation\n\n{summary}\n\nSetup, mappings, job behavior and limits: [SERVICES.md](SERVICES.md). The established text, investigation, web, file, storage, MCP/A2A and delegation paths remain part of the same app.\n\nValidation: {len(runs)-len(failed)}/{len(runs)} connected groups, {sum(counts.values())} executed checks, {unit_count} selected Rust cases. Reports describe owner execution, not independent acceptance.\n\n{limits}\n')
previous_progress=(root/'project-progress.md').read_text()
(root/'project-progress.md').write_text(f'''# RLLM checkpoint — {stamp}

Build {version}; signed candidate. {summary}

Validation: {len(runs)-len(failed)}/{len(runs)} connected groups, {sum(counts.values())} executed checks, {unit_count} selected current Rust cases. APK {apk['bytes']} bytes, SHA256 {apk['sha256']}. Retained signer ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5. Exactly one matching signed APK is included.

{limits}

Protected baseline: version1793750802, immutable source SHA2562676d5476035cc32db9b85f652c94ecc5e21ca09f925ac2dc983cce932e94917, baseline commit36a5075caa58110ba33ef58990d91c1a102ae970. Immutable baseline source Drive1pBK9HP49rMqBuftLOhH_IMfgWIs0gIdB, APK1mFRkR3ubuMLrVE0Y0iDroRlWivpChtqi. Do not overwrite those baseline copies.

Next: preserve/upload this exact signed candidate and read back bytes; update the already-authorized existing project publication. Recover genuine revision-two source through the selected stable source's revision history if browser authentication becomes available; run all three historical checks without changing the reader fingerprint or current decoder. Complete current-device/live-endpoint and independent acceptance separately. Earlier broader accepted roadmap remains in the canonical cumulative engineering record, not cancelled by this upgrade: https://docs.google.com/document/d/1jyJ1ma1juoWacfJFpei5MdzPS05i7EiNy-FyWs9UPRI/edit . This build does not claim whole-project closure. No scheduled/background continuation is active. Signing custody remains in the separate protected archive.
'''+'\n---\n\nPreserved prior checkpoints:\n\n'+previous_progress)
manifest=read('PUBLICATION-MANIFEST.json');manifest.update(state='in_progress; signed candidate',version=version,signed_apk=dict(path='release/RLLM.apk',bytes=apk['bytes'],sha256=apk['sha256']),validation=validation,whole_project_complete=False,checkpoint_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
files=sorted(f for f in root.rglob('*')if f.is_file()and not set(f.relative_to(root).parts)&{'output','__pycache__','.git'}and f.name!='PUBLICATION-MANIFEST.json')
assert all(not f.is_symlink()for f in files)
assert [f.relative_to(root).as_posix()for f in files if f.suffix=='.apk']==['release/RLLM.apk']
assert not any(f.suffix in ['.p12','.jks','.password','.keystore','.zip','.tar','.xz']for f in files)
manifest['files']={f.relative_to(root).as_posix():dict(bytes=f.stat().st_size,sha256=hashlib.sha256(f.read_bytes()).hexdigest())for f in files}
manifest['build_inputs']={f.relative_to(root).as_posix():hashlib.sha256(f.read_bytes()).hexdigest()for f in files if f.relative_to(root).parts[0]in ['core','platform','res','notices']or f.name in ['AndroidManifest.xml','package.sh']}
assert manifest['build_inputs']==apk['build_inputs'],'Source differs from inspected APK inputs'
(root/'PUBLICATION-MANIFEST.json').write_text(json.dumps(manifest,indent=2)+'\n');files.append(root/'PUBLICATION-MANIFEST.json')
source=out/f'RLLM__Candidate-Source__{stamp}.zip'
with zipfile.ZipFile(source,'w',zipfile.ZIP_DEFLATED,compresslevel=9)as z:
 for f in sorted(files):z.write(f,'RLLM/'+f.relative_to(root).as_posix())
with zipfile.ZipFile(source)as z:
 assert z.testzip()is None and len(z.namelist())==len(manifest['files'])+1
 for name,record in manifest['files'].items():data=z.read('RLLM/'+name);assert len(data)==record['bytes']and hashlib.sha256(data).hexdigest()==record['sha256']
 assert z.read('RLLM/release/RLLM.apk')==apk_bytes
published=out/f'RLLM__Candidate-Application__{stamp}.apk';published.write_bytes(apk_bytes);verify(published)
sums=out/f'RLLM__Candidate-Checksums__{stamp}.txt';sums.write_text(''.join(hashlib.sha256(f.read_bytes()).hexdigest()+'  '+f.name+'\n'for f in [published,source]))
record=dict(status='signed candidate; acceptance incomplete',version=version,members=len(files),build_inputs=len(manifest['build_inputs']),paths={k:str(f)for k,f in [('apk',published),('source',source),('checksums',sums)]},files={f.name:dict(bytes=f.stat().st_size,sha256=hashlib.sha256(f.read_bytes()).hexdigest())for f in [published,source,sums]})
(root/'output/checks/packaged.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record,indent=2))
