"""Development checks against the shared controller; results describe execution."""
from pathlib import Path
import argparse,json,re,subprocess
root=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--previous-scene-controller',type=Path);p.add_argument('--previous-controller',type=Path,default=root/'output/previous-controller');p.add_argument('--previous-flow-controller',type=Path,default=root/'output/pre-branch-controller');p.add_argument('--previous-auth-controller',type=Path,default=root/'output/pre-auth-controller');p.add_argument('--units',type=Path);p.add_argument('--auth-baseline',type=Path);p.add_argument('--auth-baseline-sha256');args=p.parse_args();out=root/'output/checks';out.mkdir(parents=True,exist_ok=True)
if args.units:
 prior=json.loads((root/'verification/RESULTS.json').read_text())['selected_rust_runs'];results=[]
 for name in ['network::authorization_response_checks::','application::scene::checks::','application::tool_format::tests::','application::search::checks::','application::oauth::checks::','application::oauth::device_authorization::checks::','application::voice_audio::checks::','application::monitor::checks::','application::request_metrics::','application::throughput::','application::checks::live_rate_opens_analytics_without_losing_stream_or_draft','application::checks::tls_guidance_preserves_explicit_endpoint']:
  if not any(r['filter']==name for r in prior):prior.append(dict(filter=name))
 for r in prior:
  c=subprocess.run([str(args.units.resolve()),r['filter'],'--test-threads=1'],capture_output=True,text=True);summary=next((x for x in c.stdout.splitlines()if x.startswith('test result:')),c.stdout[-700:]+c.stderr[-700:]);results.append(dict(filter=r['filter'],code=c.returncode,summary=summary));print(r['filter'],c.returncode,flush=True)
 (out/'selected-units.json').write_text(json.dumps(results,indent=2)+'\n');assert all(r['code']==0 for r in results),results;print('Selected cases:',sum(int(re.search(r'(\d+) passed',r['summary'])[1])for r in results));raise SystemExit()
names=['provider_process','network_drive','drive_ownership','question_process','history','a2a','acp','oauth','device_authorization','binary_flow','pe_declarations','commands','history_migration','hosted_phases','prompt_reference','provider_evidence','delegation_visibility','local_feedback','network_feedback','proposal_feedback','proposal_recovery','review_ownership','task_ownership','dav_listing','server_admission','http_framing','platform_resolution','query_feedback','carry_flow','workspace_navigation','field_flow','flags_flow','product_flow','diagnostics','services','media','jobs','voice','media_stream','media_upload','voice_interruption','voice_background','voice_session','capture_lifecycle','scene','apk_alignment','audio_route']
def run(name):
 command=['python3',str(root/'verification'/f'{name}.py')]
 if name=='scene'and args.previous_scene_controller:command+=['--previous-scene-controller',str(args.previous_scene_controller.resolve())]
 if name in ['history_migration','prompt_reference']:command+=['--previous-controller',str(args.previous_controller.resolve())]
 if name=='binary_flow':command+=['--previous-flow-controller',str(args.previous_flow_controller.resolve()),'--previous-auth-controller',str(args.previous_auth_controller.resolve())]
 if name=='binary_flow'and args.auth_baseline:command+=['--auth-baseline',str(args.auth_baseline.resolve())]
 if name=='binary_flow'and args.auth_baseline_sha256:command+=['--auth-baseline-sha256',args.auth_baseline_sha256]
 try:c=subprocess.run(command,capture_output=True,text=True,timeout=180)
 except subprocess.TimeoutExpired as error:
  text=lambda value:value.decode(errors='replace')if isinstance(value,bytes)else(value or'')
  (out/f'{name}.log').write_text(text(error.stdout)+text(error.stderr)+'\nDevelopment check exceeded 180 seconds.\n');return dict(name=name,code=124)
 (out/f'{name}.log').write_text(c.stdout+c.stderr);return dict(name=name,code=c.returncode)
results=[]
for name in names:
 r=run(name);results.append(r);print(r,flush=True)
(out/'current-regression.json').write_text(json.dumps(results,indent=2)+'\n');assert all(r['code']==0 for r in results),results
