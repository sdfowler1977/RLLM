"""Development checks against the shared controller; results describe execution."""
from pathlib import Path
import argparse,hashlib,json,os,re,subprocess,tempfile
root=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--previous-scene-controller',type=Path);p.add_argument('--previous-controller',type=Path,default=root/'output/previous-controller');p.add_argument('--previous-flow-controller',type=Path,default=root/'output/pre-branch-controller');p.add_argument('--previous-auth-controller',type=Path,default=root/'output/pre-auth-controller');unit_modes=p.add_mutually_exclusive_group();unit_modes.add_argument('--units',type=Path);unit_modes.add_argument('--all-units',type=Path);p.add_argument('--auth-baseline',type=Path);p.add_argument('--auth-baseline-sha256');args=p.parse_args();out=root/'output/checks';out.mkdir(parents=True,exist_ok=True)
if args.all_units:
 binary=args.all_units.resolve()
 inventory=subprocess.check_output([str(binary),'--list','--format','terse'],text=True,timeout=30)
 cases=[line.removesuffix(': test') for line in inventory.splitlines() if line.endswith(': test')]
 assert cases and len(set(cases))==len(cases)
 prefix='application::checks::'
 isolated=[case for case in cases if case.startswith(prefix)]
 batches=[(case,[case,'--exact'],1) for case in isolated]
 batches.append(('remaining modules',['--skip',prefix],len(cases)-len(isolated)))
 passed=0;failures=[]
 def capture_font():
  # The capture resource cases declare this input; the runner supplies a real
  # installed TrueType font instead of leaving the batch red with no cause.
  configured=os.environ.get('CLIENT_CAPTURE_TEST_FONT','')
  if configured and Path(configured).is_file():return configured
  found=sorted(Path('/usr/share/fonts').rglob('*.ttf'))
  return str(found[0]) if found else ''
 font=capture_font()
 environment={**os.environ,**({'CLIENT_CAPTURE_TEST_FONT':font} if font else {})}
 with tempfile.TemporaryDirectory(prefix='rust-',dir=out) as temporary:
  for index,(name,selection,count) in enumerate(batches):
   work=Path(temporary)/str(index);(work/'toolchain/check-workspace').mkdir(parents=True)
   run=subprocess.run([str(binary),*selection,'--test-threads=1'],cwd=work,env=environment,capture_output=True,text=True,timeout=120)
   summary=re.search(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',run.stdout)
   if summary:passed+=int(summary[1])
   if run.returncode or not summary or tuple(map(int,summary.groups()))!=(count,0,0):
    failures.append(dict(name=name,code=run.returncode,details=(run.stdout+run.stderr)[-5000:]))
 report=dict(cases=len(cases),passed=passed,isolated_application_cases=len(isolated),capture_font=font or 'unavailable',failures=failures,binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
 (out/'full-units.json').write_text(json.dumps(report,indent=2)+'\n')
 print(f'Rust cases passed: {passed}/{len(cases)}; isolated application cases: {len(isolated)}')
 if failures:raise SystemExit(1)
 raise SystemExit()
if args.units:
 recorded=json.loads((root/'verification/RESULTS.json').read_text())
 prior=[dict(filter=name) for name in recorded['selected_rust_filters']] if 'selected_rust_filters' in recorded else recorded['selected_rust_runs']
 results=[]
 for name in ['network::etag_checks::','network::chunk_checks::','network::response_checks::','network::field_checks::','network::authorization_response_checks::','application::scene::checks::','application::scene::scene_texture::checks::','application::tool_format::tests::','application::search::checks::','application::oauth::checks::','application::oauth::credential_checks::','application::inquiry::design_brief::checks::','application::oauth::device_authorization::checks::','application::voice_audio::checks::','application::monitor::checks::','application::request_metrics::','application::throughput::','application::checks::live_rate_opens_analytics_without_losing_stream_or_draft','application::checks::tls_guidance_preserves_explicit_endpoint']:
  if not any(r['filter']==name for r in prior):prior.append(dict(filter=name))
 for r in prior:
  c=subprocess.run([str(args.units.resolve()),r['filter'],'--test-threads=1'],capture_output=True,text=True);summary=next((x for x in c.stdout.splitlines()if x.startswith('test result:')),c.stdout[-700:]+c.stderr[-700:]);results.append(dict(filter=r['filter'],code=c.returncode,summary=summary));print(r['filter'],c.returncode,flush=True)
 (out/'selected-units.json').write_text(json.dumps(results,indent=2)+'\n');assert all(r['code']==0 for r in results),results;print('Selected cases:',sum(int(re.search(r'(\d+) passed',r['summary'])[1])for r in results));raise SystemExit()
names=['provider_process','network_drive','drive_ownership','question_process','history','a2a','acp','oauth','device_authorization','binary_flow','pe_declarations','commands','history_migration','hosted_phases','prompt_reference','provider_evidence','delegation_visibility','local_feedback','network_feedback','proposal_feedback','proposal_recovery','review_ownership','task_ownership','dav_listing','server_admission','http_framing','gateway_expectation','device_shortcut','request_fields','response_fields','chunk_fields','storage_etags','platform_resolution','query_feedback','carry_flow','workspace_navigation','mcp_catalog','mcp_catalog_events','mcp_images','mcp_continuation','mcp_history','document_providers','snapshot_identity','storage_auth','storage_oauth','credential_rotation','storage_owner','voice_settings','field_flow','flags_flow','product_flow','diagnostics','services','media','jobs','voice','media_stream','media_upload','voice_interruption','voice_background','voice_session','capture_lifecycle','scene','apk_alignment','audio_route','state_sealing','desktop_runtime','turn_background','permission_modes','self_rebuild','rrsi_loop']
# The Rust runner keeps its own group list; this list is not derived from it, so
# pin the pair here instead of letting a new group reach only one instrument.
runner=re.findall(r'"([a-z0-9_]+)"',(root/'tools'/'harness.rs').read_text().split('const GROUPS: &[&str] = &[',1)[1].split('];',1)[0])
assert runner==names,(sorted(set(runner)^set(names)),'the two sanctioned runners enumerate different groups')
def run(name):
 command=['python3',str(root/'verification'/f'{name}.py')]
 if name=='scene'and args.previous_scene_controller:command+=['--previous-scene-controller',str(args.previous_scene_controller.resolve())]
 if name=='history_migration'or(name=='prompt_reference'and args.previous_controller.is_file()):command+=['--previous-controller',str(args.previous_controller.resolve())]
 if name=='binary_flow':command+=['--previous-flow-controller',str(args.previous_flow_controller.resolve()),'--previous-auth-controller',str(args.previous_auth_controller.resolve())]
 if name=='binary_flow'and args.auth_baseline:command+=['--auth-baseline',str(args.auth_baseline.resolve())]
 if name=='binary_flow'and args.auth_baseline_sha256:command+=['--auth-baseline-sha256',args.auth_baseline_sha256]
 try:c=subprocess.run(command,capture_output=True,text=True,timeout=180)
 except subprocess.TimeoutExpired as error:
  text=lambda value:value.decode(errors='replace')if isinstance(value,bytes)else(value or'')
  (out/f'{name}.log').write_text(text(error.stdout)+text(error.stderr)+'\nDevelopment check exceeded 180 seconds.\n');return dict(name=name,code=124)
 (out/f'{name}.log').write_text(c.stdout+c.stderr)
 if c.returncode==0 and name in ['response_fields','chunk_fields']:
  result=json.loads(c.stdout)
  assert isinstance(result['passed'],int) and result['passed']==len(result['checks']) and result['passed']>0
  (out/(name.replace('_','-')+'.json')).write_text(json.dumps(result,indent=2)+'\n')
 return dict(name=name,code=c.returncode)
results=[]
for name in names:
 r=run(name);results.append(r);print(r,flush=True)
(out/'current-regression.json').write_text(json.dumps(results,indent=2)+'\n');assert all(r['code']==0 for r in results),results
