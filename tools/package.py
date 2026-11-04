"""Package the current source and its verified signed APK for private delivery."""
from pathlib import Path
import argparse,datetime,hashlib,json,re,shutil,zipfile
from signature import verify
root=Path(__file__).resolve().parents[1];p=argparse.ArgumentParser();p.add_argument('--destination',type=Path,required=True);args=p.parse_args();out=args.destination.resolve();out.mkdir(parents=True,exist_ok=True)
read=lambda n:json.loads((root/n).read_text());apk=read('output/checks/apk.json');
for relative,digest in apk['build_inputs'].items():assert hashlib.sha256((root/relative).read_bytes()).hexdigest()==digest,'Source changed after APK inspection: '+relative
apk_bytes=Path(apk['path']).read_bytes();assert len(apk_bytes)==apk['bytes']and hashlib.sha256(apk_bytes).hexdigest()==apk['sha256'],'APK bytes changed after inspection';version=apk['version'];stamp=Path(apk['path']).parent.name;assert re.fullmatch(r'\d{4}-\d\d-\d\d__\d\d-\d\d-\d\dZ',stamp)
units=read('output/checks/selected-units.json');assert all(v['code']==0 for v in units);unit_count=sum(int(re.search(r'(\d+) passed',v['summary'])[1])for v in units);assert unit_count>=194
regressions=read('output/checks/current-regression.json');assert len(regressions)==30 and all(v['code']==0 for v in regressions)
groups=[('provider_process_child','current-results'),('network_drive','network-results'),('drive_ownership','drive-ownership'),('manual_question_process','process-results'),('history','history-results'),('a2a','a2a'),('binary_declarations_and_flow','binary-flow'),('commands','commands'),('prior_controller_migration','history-migration'),('hosted_phases','hosted-phases'),('prompt_reference','prompt-reference'),('provider_evidence','provider-evidence'),('pe_declarations','pe-declarations'),('delegation_visibility','delegation-visibility'),('local_failure_feedback','local-feedback'),('network_failure_feedback','network-feedback'),('proposal_feedback','proposal-feedback'),('review_ownership','review-ownership'),('task_ownership','task-ownership'),('dav_listing','dav-listing'),('server_admission','server-admission'),('http_framing','http-framing'),('platform_resolution','platform-resolution'),('query_feedback','query-feedback'),('carry_arithmetic','carry-flow'),('workspace_controls','workspace-navigation'),('field_values','field-flow'),('flag_facts','flags-flow'),('product_facts','product-flow'),('diagnostics','diagnostics')]
checks={k:read('output/checks/'+n+'.json')for k,n in groups};retained=checks['binary_declarations_and_flow'].get('retained_compatibility');retained_count=len(retained['checks'])if retained else 0;counts={k:len(v['passed'])for k,v in checks.items()};total=sum(counts.values());assert total>=1186,(total,counts)
validation=dict(published_comparator=dict(version=1793733101,source_sha256='06ffb8e78537914c932da623dc62048ab734bcc42dcbd9a01b0abd523bfad375',method='Controller compiled from the exact shared core; all97core module hashes match published source; actual retained bytes pass through current view, query and exact export'),connected=dict(counts,total=total,executed_current=total-retained_count,retained_prior=retained_count),retained_compatibility=retained,selected_rust_cases=unit_count,selected_rust_runs=units,checks=checks,apk={k:v for k,v in apk.items()if k!='path'},evidence_owner='implementation owner; LLVM metadata and actual prior-core comparison references',authentication_comparator=dict(version=1793593529,source_sha256='af219d4d2269f8f9ef0b9b9656106a8eca5dedbcebc5ef95047216de0c26b289',method='actual revision-two shared-core controller with authenticated words unclassified; retained prior execution is identified separately when present'),flow_comparator=dict(version=1793395702,source_sha256='f9d0dac9e3e8ae02cf5e787ffa71871f7d1242020fa6c87a3740af5b2d827ab8',method='actual prior shared-core controller with thin JSON-lines envelope; held-source revision-one graph and current retained view'),independent_acceptance=False,installed_device_tested=False,live_model_NAS_tested=False,history_comparator=dict(version=1793395702,source_sha256='f9d0dac9e3e8ae02cf5e787ffa71871f7d1242020fa6c87a3740af5b2d827ab8',method='actual prior core with thin JSON-lines envelope adapter; negotiate its requested protocol revision'))
validation['published_comparator']=dict(version=1793742554,source_sha256='feb3a0ea908caa9ca63dbd571f7296ea985eaac6aaa8954f6aba3f861c0abddf',method='Controller compiled from the exact published shared core before product edits; actual retained bytes pass through current view, query and exact export')
validation['additional_audit']=read('output/checks/audit-details.json')
(root/'verification/RESULTS.json').write_text(json.dumps(validation,indent=2)+'\n');(root/'release/RLLM.apk').write_bytes(apk_bytes);verify(root/'release/RLLM.apk');assert (root/'release/RLLM.apk').read_bytes()==apk_bytes;(root/'release/SHA256SUMS.txt').write_text(apk['sha256']+'  RLLM.apk\n')
summary='Model operation catalogs now follow Workspace source-access switches and the assigned non-writing role in all four provider formats. Configured connections remain saved; re-enabled sources regain their permitted operations. Retired document and app-inspection callbacks preserve the exact prior state before project refresh. The intrinsic command process, reviewed internet search and captured child-to-parent return use the existing shared core. Diagnostics retain their exact 1–128 run averaging range and compact gauge layout.'
(root/'release/NOTES.md').write_text(f'''# RLLM {version}

RLLM, com.local.client.lab, Android8.0+, ARM64 and x86-64. The retained current signing identity is used for this update.

{summary}

Validated with {total-retained_count} current connected checks across 30 groups and {unit_count} selected Rust cases. Three historical compatibility outcomes retain their original execution provenance. Additional audit findings and baseline fixture comparisons are recorded in docs/AUDIT.md. The signed APK identity, both architectures, archive integrity and 16 KiB alignment are verified.


APK{apk['bytes']}bytes, SHA256{apk['sha256']}.
''')
(root/'docs/STATUS.md').write_text(f'''# Current implementation

{summary}

The app includes four provider text formats, incremental replies, user agent/skill settings, file inspection/comparison, typed source observations, web search/page capture, reviewed reconstruction/experiments, WebDAV storage, encrypted history and local MCP/A2A access.

Validation: {total-retained_count} current connected owner checks, {retained_count} retained prior compatibility outcomes and {unit_count} selected Rust cases. Exact filters, scoped outcomes and comparison references are recorded in verification/RESULTS.json. Development toolchain restoration, checks, package inspection and readback scripts accompany this source in tools/.
''')
(root/'project-progress.md').write_text(f'''# RLLM checkpoint — {stamp}

Build{version}. {summary}

Validation: {total-retained_count} current connected owner checks across30groups, {retained_count} retained prior compatibility outcomes and {unit_count} selected Rust cases. APK{apk['bytes']}bytes SHA256{apk['sha256']}. AppRLLM, packagecom.local.client.lab, retained signerba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5. This source contains one matching signed APK.

Status: three reproduced audit defects repaired and connected checks passed. Live public search passed through reviewed child and parent routes using controlled model replies. Current implementation work remains: broader ISA, multi-edge/type/alias analysis and structured decompilation; domain/process reconstruction; browser/language/compiler/runtime/inference coverage; provider/ACP/storage authorization, SMB, resource/accelerator and recursive scheduling coverage. Separate acceptance remains for installed Android lifecycle/UI/accessibility/IME/Keystore and current-key installation, configured live primary/child models and NAS/TLS, and a non-owner current-byte review. Earlier accepted requirements remain in the canonical engineering record. Photo-generation models and server speech/open-mic requests retain their future scope.

The latest requested Workspace organization, separate browsing/setup, source access switches, new workspace creation and discoverable MCP server workflow are implemented and exercised in this checkpoint. Next: continue the preserved broader connected analysis and command-flow work, with bitfield/extract propagation now implemented and exercised; preserve bounded work, coordinate provenance and previous captures. Continue connected Android/network/delegation repair when concrete defects are reproduced. The user accepts the current signing key and requested continued implementation; proceed independently without further installation prompts. Protected archive/compression and rejected historical fixtures stay untouched. Reuse retained current signing custody and stable private delivery IDs.

Canonical cumulative engineering task state and the next action are preserved in https://docs.google.com/document/d/1jyJ1ma1juoWacfJFpei5MdzPS05i7EiNy-FyWs9UPRI/edit . Resume authorized implementation from this saved source and that checkpoint. Source recovery/build/check/package/readback tools are in tools/; signing custody remains in the existing separate private key archive.
''')
manifest=read('PUBLICATION-MANIFEST.json');manifest.update(version=version,signed_apk=dict(path='release/RLLM.apk',bytes=apk['bytes'],sha256=apk['sha256']),validation=validation,checkpoint_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),whole_project_complete=False)
assert not any(p.is_symlink()for p in root.rglob('*')if'output'not in p.relative_to(root).parts and'__pycache__'not in p.parts),'Source archive requires owned regular files'
files=[p for p in root.rglob('*')if p.is_file()and'output'not in p.relative_to(root).parts and'__pycache__'not in p.parts and p.name!='PUBLICATION-MANIFEST.json'];assert [p.relative_to(root).as_posix()for p in files if p.suffix=='.apk']==['release/RLLM.apk'];assert not any(p.suffix in['.zip','.tar','.xz','.p12','.jks','.keystore','.pyc','.password']for p in files)
manifest['files']={p.relative_to(root).as_posix():dict(bytes=p.stat().st_size,sha256=hashlib.sha256(p.read_bytes()).hexdigest())for p in sorted(files)};manifest['build_inputs']={p.relative_to(root).as_posix():hashlib.sha256(p.read_bytes()).hexdigest()for p in sorted(files)if p.relative_to(root).parts[0]in['core','platform','res','notices']or p.name in['AndroidManifest.xml','package.sh']};assert len(manifest['build_inputs'])==138 and manifest['build_inputs']==apk['build_inputs'],'Source differs from compiled APK inputs'
(root/'PUBLICATION-MANIFEST.json').write_text(json.dumps(manifest,indent=2)+'\n');files.append(root/'PUBLICATION-MANIFEST.json');source=out/f'RLLM__Source__{stamp}.zip'
with zipfile.ZipFile(source,'w',zipfile.ZIP_DEFLATED,compresslevel=9)as z:
 for p in sorted(files):z.write(p,'RLLM/'+p.relative_to(root).as_posix())
with zipfile.ZipFile(source)as z:
 assert z.testzip()is None and len(z.namelist())==len(manifest['files'])+1
 for n,m in manifest['files'].items():data=z.read('RLLM/'+n);assert len(data)==m['bytes']and hashlib.sha256(data).hexdigest()==m['sha256']
 assert z.read('RLLM/release/RLLM.apk')==apk_bytes
published=out/f'RLLM__Application__{stamp}.apk';published.write_bytes(apk_bytes);verify(published);assert published.read_bytes()==apk_bytes;sums=out/f'RLLM__Checksums__{stamp}.txt';sums.write_text(''.join(hashlib.sha256(p.read_bytes()).hexdigest()+'  '+p.name+'\n'for p in[published,source]));assert all(not re.search(r'a[0-9]',p.name,re.I)for p in[published,source,sums])
record=dict(version=version,members=len(files),build_inputs=len(manifest['build_inputs']),paths={k:str(p)for k,p in[('apk',published),('source',source),('checksums',sums)]},files={p.name:dict(bytes=p.stat().st_size,sha256=hashlib.sha256(p.read_bytes()).hexdigest())for p in[published,source,sums]});(root/'output/checks/packaged.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record,indent=2))
