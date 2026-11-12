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
full_units=read('output/checks/full-units.json') if (root/'output/checks/full-units.json').is_file() else None
if full_units:
 assert full_units['passed']==full_units['cases'] and full_units['cases']>0 and not full_units['failures'],full_units['failures']
 units=[];unit_count=full_units['passed'];unit_label='Rust cases from the full suite'
else:
 units=read('output/checks/selected-units.json');assert all(v['code']==0 for v in units)
 unit_count=sum(int(re.search(r'(\d+) passed',v['summary'])[1])for v in units)
 assert unit_count>=199
 unit_label='selected Rust cases'
runs=read('output/checks/current-regression.json')
# Match the current runner's exact ordered inventory, never a stale group count.
import ast
runner=ast.parse((root/'tools/checks.py').read_text())
expected=next(ast.literal_eval(node.value) for node in runner.body if isinstance(node,ast.Assign) and any(isinstance(target,ast.Name) and target.id=='names' for target in node.targets))
assert [value['name'] for value in runs]==expected,'Regression inventory differs from the current runner'
assert len(set(expected))==len(expected),'Duplicate regression group'
failed=[v for v in runs if v['code']!=0]
if failed:assert args.candidate and failed==[dict(name='binary_flow',code=1)],failed
pairs=[('provider_process','current-results'),('network_drive','network-results'),('drive_ownership','drive-ownership'),('question_process','process-results'),('history','history-results'),('a2a','a2a'),('binary_flow','binary-flow'),('commands','commands'),('history_migration','history-migration'),('hosted_phases','hosted-phases'),('prompt_reference','prompt-reference'),('provider_evidence','provider-evidence'),('pe_declarations','pe-declarations'),('delegation_visibility','delegation-visibility'),('local_feedback','local-feedback'),('network_feedback','network-feedback'),('proposal_feedback','proposal-feedback'),('proposal_recovery','proposal-recovery'),('review_ownership','review-ownership'),('task_ownership','task-ownership'),('dav_listing','dav-listing'),('server_admission','server-admission'),('http_framing','http-framing'),('platform_resolution','platform-resolution'),('query_feedback','query-feedback'),('carry_flow','carry-flow'),('workspace_navigation','workspace-navigation'),('field_flow','field-flow'),('flags_flow','flags-flow'),('product_flow','product-flow'),('diagnostics','diagnostics'),('desktop_runtime','desktop_runtime'),('state_sealing','state-sealing'),('turn_background','turn_background'),('permission_modes','permission_modes')]+[(n,n)for n in ['services','media','jobs','voice','media_stream','media_upload','voice_interruption','voice_background','voice_session','capture_lifecycle','scene','apk_alignment','audio_route','document_providers','snapshot_identity','storage_auth','storage_oauth','credential_rotation','voice_settings','acp','oauth','device_authorization']]
pairs += [(name,name.replace('_','-')) for name in ['gateway_expectation','device_shortcut','request_fields','response_fields','chunk_fields','storage_etags','storage_owner','mcp_catalog','mcp_catalog_events','mcp_images','mcp_continuation','mcp_history']]
assert set(name for name,_ in pairs)==set(expected),'Packaging report inventory differs from current checks'
checks={name:read('output/checks/'+file+'.json') for name,file in pairs}
if failed:assert len(checks['binary_flow'].get('unavailable',[]))==3 and checks['binary_flow']['retained_compatibility'] is None
def executed_count(value):
 if 'passed' in value:
  count=len(value['passed']) if isinstance(value['passed'],list) else value['passed']
 else:
  count=value['count']
 assert isinstance(count,int) and not isinstance(count,bool) and count>0
 if 'checks' in value:assert count==len(value['checks'])
 if 'count' in value:assert count==value['count']
 return count
counts={name:executed_count(value) for name,value in checks.items()}
for name in ['services','media','jobs','voice','media_stream','media_upload','voice_interruption','voice_background','voice_session','capture_lifecycle','scene','apk_alignment','audio_route','document_providers','snapshot_identity','storage_auth','storage_oauth','credential_rotation','voice_settings','acp','oauth','device_authorization']:assert counts[name]==len(checks[name]['checks'])
published_reference=read('output/checks/published-comparator.json')
assert published_reference['source_sha256']==hashlib.sha256(Path(published_reference['source_path']).read_bytes()).hexdigest(),'Published comparator source changed'
assert published_reference['controller_sha256']==hashlib.sha256((root/'output/published-controller').read_bytes()).hexdigest(),'Published comparator executable changed'
validation=dict(connected=dict(counts=counts,total_executed=sum(counts.values()),groups_passed=len(runs)-len(failed),groups=len(runs),unavailable=failed),selected_rust_cases=unit_count if not full_units else None,selected_rust_runs=units,checks=checks,apk={k:v for k,v in apk.items()if k!='path'},evidence_owner='implementation owner; real local HTTP/file fixtures, compiled prior core and LLVM references',published_comparator=published_reference,flow_comparator=dict(version=1793395702,source_sha256='f9d0dac9e3e8ae02cf5e787ffa71871f7d1242020fa6c87a3740af5b2d827ab8',method='Actual prior core with thin original JSON-lines envelope'),authentication_comparator=dict(version=1793593529,source_sha256='af219d4d2269f8f9ef0b9b9656106a8eca5dedbcebc5ef95047216de0c26b289',available=not bool(failed),method='Genuine comparator required; no substitution or weakened historical reader fingerprint'),independent_acceptance=False,installed_device_tested=False,live_service_endpoints_tested=False,live_model_NAS_tested=False)
if full_units:
 previous=read('verification/RESULTS.json')
 validation['full_rust_suite']=full_units
 validation['selected_rust_filters']=previous.get('selected_rust_filters',[run['filter'] for run in previous.get('selected_rust_runs',[])])
validation['upgrade_audit']=read('verification/UPGRADE-AUDIT.json')
validation['historical_full_rust_suite']=dict(source_version=1793837555,source_sha256='6a24fa55a6cd10e299c49655858772866fa849c3c37aa99bfd34772b49eabfdb',results=validation['upgrade_audit']['full_rust_suite'],scope='Original historical run retained separately from current execution.')
if (root/'output/checks/answer-mode-flow.json').exists():validation['answer_mode_flow']=read('output/checks/answer-mode-flow.json')
if (root/'output/checks/public-page-flow.json').exists():validation['public_page_flow']=read('output/checks/public-page-flow.json')
if (root/'output/search-flow.json').exists():validation['provider_search_flow']=read('output/search-flow.json')
if (root/'output/checks/live-web.json').exists():validation['live_public_web']=read('output/checks/live-web.json')
if (root/'output/checks/independent-improvement.json').exists():validation['independent_improvement_slice']=read('output/checks/independent-improvement.json')
if (root/'output/checks/independent-file-transfer.json').exists():validation['independent_file_transfer_slice']=read('output/checks/independent-file-transfer.json')
if (root/'output/checks/independent-media.json').exists():validation['independent_media_slice']=read('output/checks/independent-media.json')
if (root/'output/checks/independent-storage.json').exists():validation['independent_storage_slice']=read('output/checks/independent-storage.json')
if (root/'output/checks/independent-transport.json').exists():validation['independent_transport_slice']=read('output/checks/independent-transport.json')
if (root/'output/checks/independent-search.json').exists():validation['independent_search_slice']=read('output/checks/independent-search.json')
version=apk['version'];stamp=Path(apk['path']).parent.name;assert re.fullmatch(r'\d{4}-\d\d-\d\d__\d\d-\d\d-\d\dZ',stamp)
summary='Auto chooses a direct reply, a short current-fact lookup, or deeper investigation. The conversation has explicit mode choices. Simple local time and date questions use the device clock. Quick lookups keep full source and error details available on tap, and a verified DJIA daily close is labeled with its observation date. Automatic web access now includes ordinary public HTTPS pages and marketplace searches opened directly by primary or delegated agents. The app checks the resolved address before connecting and applies the same public scope to every redirect. Requests for private destinations, page scripts and captures follow existing review controls. Supported OpenAI, xAI, Anthropic and Gemini connections can use provider-hosted search on both the primary agent and delegated agents. Provider searches run on the selected connection, and RLLM keeps returned source metadata. One evidence-only follow-up can recover an empty answer after search without repeating the query. Automatic web search approval is on by default for configured search sources. A single valid search proposal in model prose runs under the same approval policy, and eligible ambiguous proposals get one bounded model correction. Web-search fallback and subsequent queries stay in the current turn. Chat and local streaming enforce declared lengths and encodings and validate chunk boundaries and trailers before adopting text or completion. Bodyless HTTP 204 responses cannot publish reply text. Owned responses enforce the shared 16 KiB trailer limit. Framed MCP control acknowledgments settle after informational headers without waiting for a close. An explicit search deferral requires fresh approval even when automatic or saved approval is enabled, and omits the provider-hosted search tool for that turn. RLLM offers a second bounded refinement round after a comparable partial gain, keeps the stronger tested instructions, and compares JSON values in separate evaluation. Shared response framing accepts split delimiters at the exact 8 KiB chunk-size and 64 KiB response-header limits. RLLM can evaluate the original and retained agent instructions on up to two separate tasks with local phrase, exact-text, JSON or digest checks. Both retained replies must pass comparable checks before the evaluation can contribute a measured lesson. Evaluation proposals, interrupted responses and incomparable contexts cannot promote a lesson. A completed training candidate remains reviewable after an interrupted evaluation. RLLM learns the default built-in search order from validated same-query fallbacks. Checked agent refinements replay the exact revised instructions and retain a passing candidate when a revision fails. User adoption preserves the original agent profile. Duplicate correlated MCP POST stream replies are refused before result adoption. RLLM retains multiple named MCP connections and uses one selected server for discovery, calls and streams. A refresh publishes only the complete tool catalog and refuses duplicate tool names across pages. Ambiguous names reported by the active connection require a successful refresh before use, including after switching away and back. New conversation leads the plus menu. An explicit request to search the web authorizes built-in queries for the current turn without repeated prompts, including a follow-up command that relies on the current topic. An unavailable built-in source can fail over, and raw search diagnostics stay in message details. Higher-priority destination review remains available. Conversation displays compact web results and keeps raw tool JSON in message details. Storage authorization uses the configured certificate hostname on the matching HTTPS route while separate origins retain their own identity. Shared Rust HTTP request and response handling validates field syntax, credential-header boundaries, gateway request lengths and expectations, informational responses and chunk extensions. Optional pre-filled device verification links retain the manual address and code and fall back on unusable links. Safe WebDAV updates also preserve valid UTF-8 opaque ETags in exact conditional writes while retaining weak, malformed and duplicate validator refusals. Shared token adoption refuses non-text refresh credentials before replacing saved grants and preserves scoped uncertain-refresh recovery. Existing conversation, intrinsic investigation, local storage and optional provider-neutral services remain integrated. The signed candidate matches the recorded cumulative source inputs.'
summary+=' Recursive refinement can check individual JSON fields in training and held-out tasks. It keeps expected values local and only treats a partial score increase as progress when earlier passing checks still pass. WebDAV listings respect declared XML namespaces, preserving valid file and folder routing when extension elements use similar names.'
summary+=' An owned media feed failure retires its ticket before it can mint a completion receipt, including when the HTTP framer has reached its last byte. Structured refinement can unwrap a single JSON code block for local verification and records the repair separately.'
summary+=' A failed owned import cannot issue a receipt for its staged prefix. A failed upload read retires only its own ticket before another read can advance it. Media chunks reject malformed Base64 instead of accepting a decodable prefix.'
limits='Installed Android microphone/playback/UI/lifecycle acceptance, real user server endpoints and whole-product independent acceptance remain unverified. Physical background/screen-off and battery behavior remain unverified. Bundled neural speech recognition and a separate medical application are not implemented. Physical effectiveness of echo control and simultaneous capture/playback remain unverified on a device. The historical revision-two comparator is unavailable; its three compatibility checks remain open.' if failed else 'Installed Android microphone/playback/UI/lifecycle acceptance, real user server endpoints and whole-product independent acceptance remain unverified. Physical background/screen-off and battery behavior remain unverified. Bundled neural speech recognition and a separate medical application are not implemented. Physical effectiveness of echo control and simultaneous capture/playback remain unverified on a device.'
limits+=' Scene preview displays static triangle geometry with base and vertex colors. Embedded PNG base-color textures and MASK cutouts are displayed. External images and other image codecs retain geometry-only viewing; animation and physical material effects remain open. Skins, morph targets, blended transparency and required unsupported extensions refuse only preview; export remains available. GPU rendering and wider scene formats remain open.'
if not full_units:limits+=' Full Rust suite execution is recorded separately when available.'
release=root/'release';release.mkdir(exist_ok=True)
(root/'verification/RESULTS.json').write_text(json.dumps(validation,indent=2)+'\n')
(release/'NOTES.md').write_text(f'# RLLM {version} — signed candidate\n\n{summary}\n\n{len(runs)-len(failed)}/{len(runs)} connected groups passed; {sum(counts.values())} executed checks and {unit_count} {unit_label}. Incomplete groups are explicitly recorded in verification/RESULTS.json.\n\n{limits}\n\nPackage com.local.client.lab, Android 8.0+, ARM64 and x86-64; retained current signing identity, APK CRC, exact build inputs and 16 KiB alignment verified. APK {apk["bytes"]} bytes, SHA256 {apk["sha256"]}.\n')
(root/'docs/STATUS.md').write_text(f'# Current implementation\n\n{summary}\n\nSetup, mappings, job behavior and limits: [SERVICES.md](SERVICES.md). The established text, investigation, web, file, storage, MCP/A2A and delegation paths remain part of the same app.\n\nValidation: {len(runs)-len(failed)}/{len(runs)} connected groups, {sum(counts.values())} executed checks, {unit_count} {unit_label}. Reports describe owner execution, not independent acceptance.\n\n{limits}\n')
# Current checkpoint only; earlier versions remain in their existing complete releases.
(root/'project-progress.md').write_text(f'''# RLLM checkpoint — {stamp}

Build {version}; signed candidate. {summary}

Validation: {len(runs)-len(failed)}/{len(runs)} connected groups, {sum(counts.values())} executed checks, {unit_count} {unit_label}. APK {apk['bytes']} bytes, SHA256 {apk['sha256']}. Retained signer ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5. The matching signed APK is supplied separately.

{limits}

Protected baseline: version1793750802, immutable source SHA2562676d5476035cc32db9b85f652c94ecc5e21ca09f925ac2dc983cce932e94917, baseline commit36a5075caa58110ba33ef58990d91c1a102ae970. Immutable baseline source Drive1pBK9HP49rMqBuftLOhH_IMfgWIs0gIdB, APK1mFRkR3ubuMLrVE0Y0iDroRlWivpChtqi. Do not overwrite those baseline copies.

Next: publish and verify this signed candidate. Broaden product-owned semantic repair and reusable deterministic verification. Continue the protocol, storage and resource roadmap with remaining integration gaps. Recover genuine revision-two source through the selected stable source's revision history if browser authentication becomes available; run all three historical checks without changing the reader fingerprint or current decoder. Complete current-device/live-endpoint and independent acceptance separately. Earlier broader accepted roadmap remains in the canonical cumulative engineering record, not cancelled by this upgrade: https://docs.google.com/document/d/1jyJ1ma1juoWacfJFpei5MdzPS05i7EiNy-FyWs9UPRI/edit . This build does not claim whole-project closure. The existing hourly build task Automation_787bc0676d388191bc52212c823ab48b and hourly delivery task Automation_1188b1045d7481919a5069a82a9def98 were updated and read back. Both are enabled. Their prompts prefer the newest verified project Drive source and reconcile later working changes. The scheduler did not expose the next-run timestamps at this checkpoint. Signing custody remains in the separate protected archive.
''')
readme=root/'README.md'
text=readme.read_text()
start=text.index('# RLLM\n\n')+len('# RLLM\n\n')
end=text.index('RLLM connects your Android device',start)
text=text[:start]+f'Current signed candidate {version} includes the MCP connections, conversation menu, web search, transport and authorization changes described in the release notes. Owner validation, unavailable historical comparisons and pending device/independent acceptance are recorded separately.\n\n'+text[end:]
text=text.replace('The source includes one matching signed RLLM APK in release/RLLM.apk.', 'The separately supplied signed APK matches this source; its SHA-256 is recorded in PUBLICATION-MANIFEST.json.')
readme.write_text(text)
published=out/f'RLLM__Candidate-Application__{stamp}.apk'
manifest=read('PUBLICATION-MANIFEST.json');manifest.update(state='in_progress; signed candidate',version=version,signed_apk=dict(location='companion',path=published.name,bytes=apk['bytes'],sha256=apk['sha256']),validation=dict(path='verification/RESULTS.json',connected=validation['connected'],rust_cases=unit_count,rust_scope=unit_label),whole_project_complete=False,checkpoint_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
files=sorted(f for f in root.rglob('*')if f.is_file()and not set(f.relative_to(root).parts)&{'output','__pycache__','.git'}and f.name!='PUBLICATION-MANIFEST.json')
assert all(not f.is_symlink()for f in files)
assert not any(f.suffix=='.apk' for f in files),'The signed APK is distributed separately'
assert not any(f.suffix in ['.p12','.jks','.password','.keystore','.zip','.tar','.xz']for f in files)
manifest['files']={f.relative_to(root).as_posix():dict(bytes=f.stat().st_size,sha256=hashlib.sha256(f.read_bytes()).hexdigest())for f in files}
manifest['build_inputs']={f.relative_to(root).as_posix():hashlib.sha256(f.read_bytes()).hexdigest()for f in files if f.relative_to(root).parts[0]in ['core','platform','res','notices']or f.relative_to(root).as_posix()in ['AndroidManifest.xml','package.sh','tools/alignment.py']}
assert manifest['build_inputs']==apk['build_inputs'],'Source differs from inspected APK inputs'
(root/'PUBLICATION-MANIFEST.json').write_text(json.dumps(manifest,indent=2)+'\n');files.append(root/'PUBLICATION-MANIFEST.json')
source=out/f'RLLM__Candidate-Source__{stamp}.zip'
with zipfile.ZipFile(source,'w',zipfile.ZIP_DEFLATED,compresslevel=9)as z:
 for f in sorted(files):z.write(f,'RLLM/'+f.relative_to(root).as_posix())
with zipfile.ZipFile(source)as z:
 assert z.testzip()is None and len(z.namelist())==len(manifest['files'])+1
 for name,record in manifest['files'].items():data=z.read('RLLM/'+name);assert len(data)==record['bytes']and hashlib.sha256(data).hexdigest()==record['sha256']
published.write_bytes(apk_bytes);verify(published)
sums=out/f'RLLM__Candidate-Checksums__{stamp}.txt';sums.write_text(''.join(hashlib.sha256(f.read_bytes()).hexdigest()+'  '+f.name+'\n'for f in [published,source]))
record=dict(status='signed candidate; acceptance incomplete',version=version,members=len(files),build_inputs=len(manifest['build_inputs']),paths={k:str(f)for k,f in [('apk',published),('source',source),('checksums',sums)]},files={f.name:dict(bytes=f.stat().st_size,sha256=hashlib.sha256(f.read_bytes()).hexdigest())for f in [published,source,sums]})
(root/'output/checks/packaged.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record,indent=2))
