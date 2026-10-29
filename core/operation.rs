//! Correlation for local worker and isolated-process completions.
//! Tokens describe an outstanding operation, never a grant of tool authority.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::os::unix::fs::MetadataExt;
use std::sync::{Mutex, OnceLock};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

static SERIAL: AtomicU64 = AtomicU64::new(1);
#[derive(Clone)]
pub(super) struct InspectionAccess { pub list:bool, pub read:bool, pub graph:bool, pub budget:usize, pub root_identity:Option<(u64,u64)>, pub cancellation_token:u64, pub deadline:Instant }
impl InspectionAccess { pub fn cancelled(&self)->bool { super::super::execution::token_cancelled(self.cancellation_token) } }
struct Permit { root: String, dev: u64, ino: u64, tool: String, invocation: String, policy: String, created: Instant, inspection:InspectionAccess }
static PERMITS: OnceLock<Mutex<BTreeMap<String, Permit>>> = OnceLock::new();
fn permits() -> &'static Mutex<BTreeMap<String, Permit>> { PERMITS.get_or_init(||Mutex::new(BTreeMap::new())) }
fn retire(ticket: &str) { if let Ok(mut current)=permits().lock() { current.remove(ticket); } }
fn invocation(v: &Json) -> String {
    let mut value=v.clone();
    // Root is checked by canonical path and filesystem identity. The remaining
    // excluded fields are routing/version metadata, never execution arguments.
    if let Obj(fields)=&mut value {
        for key in ["op","root","purpose","tool_binding","tool_ticket","mcp_cleanup","mcp_abort_cleanup"] { fields.remove(key); }
    }
    harness_rules::digest(&value)
}

fn workspace(root: &str) -> Result<(String,u64,u64)> {
    let canonical=Path::new(root).canonicalize().map_err(|e|format!("Local operation workspace: {e}"))?;
    let metadata=fs::metadata(&canonical).map_err(|e|e.to_string())?;
    if !metadata.is_dir() { return Err("Local operation workspace is not a directory".into()); }
    Ok((canonical.to_string_lossy().into_owned(),metadata.dev(),metadata.ino()))
}

/// The raw worker is also reachable through the public C ABI. Recheck current
/// global decisions there; an Ask needs the exact process-held dispatch permit.
/// Blank-policy raw engine calls remain usable without a UI or development file.
pub(super) fn authorize(v: &Json) -> Result<()> {
    authorize_scoped(v).map(|_|())
}
pub(super) fn authorize_scoped(v: &Json) -> Result<InspectionAccess> {
    let global=harness_rules::worker_policy(v)?;
    let ticket=text(v,"tool_ticket");
    if ticket.is_empty() {
        return if harness_rules::worker_review_required(&global,v) {
            Err("The current harness rule requires review; use the harness event workflow to approve this operation".into())
        } else { Ok(InspectionAccess{list:true,read:true,graph:false,budget:if text(v,"tool")=="capture"{web_document::response_room(v)}else{2*1024*1024},root_identity:None,deadline:Instant::now()+Duration::from_secs(120),cancellation_token:super::super::execution::cancellation_token()}) };
    }
    if ticket.len()>128 || number(v,"tool_binding")!=1 { return Err("Invalid local execution permit".into()); }
    let (root,dev,ino)=workspace(&text(v,"root"))?;
    let dispatched=invocation(v);
    let policy=harness_rules::digest(&global);
    let mut current=permits().lock().map_err(|_|"Local execution ownership unavailable")?;
    let p=current.get(&ticket).ok_or("Local execution permit was consumed, cancelled, expired or belongs to another process")?;
    if p.created.elapsed()>=Duration::from_secs(120) { current.remove(&ticket); return Err("Local execution permit expired before starting; request the operation again".into()); }
    if p.root!=root || p.dev!=dev || p.ino!=ino || p.tool!=text(v,"tool") || p.invocation!=dispatched || p.policy!=policy {
        return Err("Local execution arguments, workspace or current user rules differ from the approved dispatch".into());
    }
    let access=p.inspection.clone();
    current.remove(&ticket);
    Ok(access)
}

/// Retire preparation when the native controller could not commit/start it.
/// Current policy is deliberately irrelevant to cleanup of an old permit.
pub(super) fn cancel(v: &Json) -> Result<bool> {
    let ticket=text(v,"tool_ticket");
    if ticket.is_empty() || ticket.len()>128 { return Ok(false); }
    let (root,dev,ino)=workspace(&text(v,"root"))?;
    let dispatched=invocation(v);
    let mut current=permits().lock().map_err(|_|"Local execution ownership unavailable")?;
    if current.get(&ticket).map(|p|p.root==root&&p.dev==dev&&p.ino==ino&&p.invocation==dispatched).unwrap_or(false) {
        current.remove(&ticket); return Ok(true);
    }
    Ok(false)
}

fn context(st: &Json, root: &str) -> Result<Json> {
    let canonical = Path::new(root).canonicalize().map_err(|e| format!("Local operation workspace: {e}"))?;
    let metadata = fs::metadata(&canonical).map_err(|e| e.to_string())?;
    if !metadata.is_dir() { return Err("Local operation workspace is not a directory".into()); }
    let run = get(st, "task_run");
    let task = if run == &Null { Null } else {
        obj(vec![("mode",get(run,"mode").clone()),("phase",get(run,"phase").clone()),
          ("task",get(run,"task").clone()),("agent",get(run,"agent").clone()),
          ("connection",get(run,"connection").clone()),("chat",get(run,"chat").clone())])
    };
    let value = obj(vec![("root",s(&canonical.to_string_lossy())),
      ("dev",Num(metadata.dev().to_string())),("ino",Num(metadata.ino().to_string())),
      ("chat",get(st,"active").clone()),("agent_index",Num(interaction::agent_index(st).to_string())),
      ("agent",harness::active_agent(st)),("profile",profile(st).unwrap_or(Null)),
      ("request_profile",if get(st,"request_index")==&Null {Null} else {array(st,"profiles").get(number(st,"request_index")).cloned().unwrap_or(Null)}),
      ("assigned_profile",if get(run,"connection")==&Null {Null} else {array(st,"profiles").get(number(run,"connection")).cloned().unwrap_or(Null)}),
      ("task",task),("depth",Num(array(st,"delegation_stack").len().to_string())),
      ("agent_running",Bool(boolean(st,"agent_running"))),("agent_pending",Bool(boolean(st,"agent_pending"))),
      // A write may legitimately change AGENTS.md. Its resulting observation
      // must still settle; refreshed project data belongs in the next request,
      // not in the identity of the already approved side effect.
      ("harness",harness_rules::policy_binding(st))]);
    // Keep exact configuration identity without copying a long task/profile
    // into every saved callback record or introducing a smaller task limit.
    Ok(s(&harness_rules::digest(&value)))
}

pub(super) fn is_callback(action: &str, inputs: &Json) -> bool {
    (action == "tool_result" && text(inputs,"purpose") != "web_tool") ||
      (matches!(action,"failure"|"network_result") && text(inputs,"purpose") == "local_tool")
}

/// Runs before form/device capture. Invalid, duplicate and old callbacks are inert.
pub(super) fn consume(st: &mut Json, root: &str, action: &str, inputs: &Json) -> bool {
    if !matches!(action,"tool_result"|"failure") || text(inputs,"purpose") != "local_tool" { return false; }
    if !text(st,"model_expected").is_empty() { return false; }
    let expected = get(st,"operation_expected");
    let ticket = text(expected,"ticket");
    if !boolean(st,"busy") || ticket.is_empty() || ticket.len()>128 ||
      ticket != text(inputs,"tool_ticket") || text(expected,"active_tool") != text(st,"active_tool") ||
      number(expected,"binding") != 1 || get(expected,"context") == &Null { return false; }
    if action=="tool_result" && (boolean(expected,"editor")!=boolean(inputs,"editor") || text(expected,"editor_dialog")!=text(inputs,"editor_dialog")) { return false; }
    match context(st,root) {
        Ok(current) if get(expected,"context") == &current => {
            retire(&ticket);
            put(st,"operation_expected",Null); true
        },
        _ => false,
    }
}

pub(super) fn interrupt(st: &mut Json) { retire(&text(get(st,"operation_expected"),"ticket")); put(st,"operation_expected",Null); }

/// Called only for a final admitted local-tool effect, after existing permission
/// and resource checks. Network/web/MCP/revision callbacks keep their own owners.
pub(super) fn bind(st: &mut Json, effect: &mut Json, root: &str, model_origin:bool) -> Result<()> {
    if text(effect,"kind") != "tool" { return Ok(()); }
    let owner = context(st,root)?;
    let serial = SERIAL.try_update(Ordering::SeqCst,Ordering::SeqCst,|n|n.checked_add(1))
      .map_err(|_|"Local operation ticket space exhausted")?;
    let epoch = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_|"Local operation clock unavailable")?.as_nanos();
    let ticket = format!("local-{epoch:x}-{serial:x}");
    let tool = text(effect,"tool");
    if tool.is_empty() { return Err("Local operation has no tool identity".into()); }
    let active=if text(st,"active_tool").is_empty(){tool.clone()}else{text(st,"active_tool")};
    let mut expected = obj(vec![("binding",Num("1".into())),("ticket",s(&ticket)),
      ("tool",s(&tool)),("active_tool",s(&active)),("context",owner),("editor",Bool(boolean(effect,"editor"))),
      ("editor_dialog",s(&text(effect,"editor_dialog")))]);
    let mut next = st.clone();
    let experiment=experiment_records::bind(&mut next,effect,&ticket)?;if experiment!=Null{put(&mut expected,"experiment",experiment);}
    put(&mut next,"operation_expected",expected.clone());
    // Manual editors may not previously have named their worker operation.
    put(&mut next,"active_tool",s(&active));
    put(&mut next,"busy",Bool(true));
    if super::super::mcp_schema::state_bytes(&next)>6*1024*1024-8192 {
        return Err("History cannot retain this operation's callback identity; existing data retained".into());
    }
    // Optional report provenance must not shrink admission for the actual operation.
    // The mandatory callback identity above is byte-compatible with the original path.
    let before_bytes=super::super::mcp_schema::state_bytes(&next);
    let before_policy=super::super::resources::policy(get(&next,"device"),get(&next,"resources"),before_bytes);
    let descriptor=observation_records::descriptor(st,effect,&ticket);
    if descriptor!=Null && before_bytes<=6*1024*1024-512*1024 {
        let mut trial=next.clone();let mut optional=expected.clone();put(&mut optional,"observation",descriptor);put(&mut optional,"observation_format",Bool(true));put(&mut trial,"operation_expected",optional.clone());
        let bytes=super::super::mcp_schema::state_bytes(&trial);let after=super::super::resources::policy(get(&trial,"device"),get(&trial,"resources"),bytes);
        if bytes<=6*1024*1024-512*1024&&number(&after,"response_bytes")>=number(&before_policy,"response_bytes"){next=trial;expected=optional;}
    }
    let graph=boolean(&expected,"observation_format");
    let (path,dev,ino)=workspace(root)?;
    let agent=if get(st,"task_run")==&Null{harness::active_agent(st)}else{get(get(st,"task_run"),"agent").clone()};
    let tools=text(&agent,"tools");
    let permitted=|name:&str|!model_origin||tools.split(',').map(str::trim).any(|v|v==name);
    let inspection=InspectionAccess{list:permitted("list"),read:permitted("read"),graph,budget:if tool=="capture"{web_document::response_room(&next)}else{number(get(st,"policy"),"response_bytes")},root_identity:Some((dev,ino)),deadline:Instant::now()+Duration::from_secs(120),cancellation_token:super::super::execution::cancellation_token()};
    let permit=Permit{root:path,dev,ino,tool:tool.clone(),invocation:invocation(effect),
      policy:harness_rules::digest(&harness_rules::gateway_global(st)),created:Instant::now(),inspection};
    {
        let mut current=permits().lock().map_err(|_|"Local execution ownership unavailable")?;
        current.retain(|_,p|p.created.elapsed()<Duration::from_secs(120));
        if current.len()>=64 { return Err("Local execution dispatch capacity is occupied; no operation was started".into()); }
        let old=text(get(st,"operation_expected"),"ticket");
        current.remove(&old);
        current.insert(ticket.clone(),permit);
    }
    put(effect,"purpose",s("local_tool"));
    put(effect,"tool_binding",Num("1".into()));
    put(effect,"tool_ticket",s(&ticket));
    *st=next;
    Ok(())
}
