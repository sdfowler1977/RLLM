//! Workspace navigation, source access and app-owned project directories.
//! Device storage is the native filesystem on every platform. Internal `phone_*`
//! identifiers stay so saved access state still loads.
use super::*;
const SECTIONS:&[&str]=&["cloud_selected","workspace_projects","workspace_storage","workspace_setup","workspace_permissions","workspace_defaults","workspace_new","workspace_locations","phone_locations","network_locations","document_providers","workspace_programs","workspace_research","workspace_snapshots","workspace_files"];
fn provider_controls()->bool{cfg!(target_os="android")}
pub(super) fn retired_provider_route(action:&str)->bool{
 let bytes=action.as_bytes();
 bytes.len()==12&&bytes[6]==b'_'&&bytes[7..]==*b"drive"&&bytes[..6].iter().all(|b|b.is_ascii_lowercase())
}
fn legacy_route(name:&str)->&str{
 if retired_provider_route(name){if provider_controls(){"document_providers"}else{"workspace_storage"}}
 else if !provider_controls()&&matches!(name,"document_providers"|"cloud_selected"){"workspace_storage"}
 else if !provider_controls()&&name=="cloud_setup"{"workspace_setup"}
 else{name}
}
fn require_provider()->Result<()>{if provider_controls(){Ok(())}else{Err("Document providers are not available on this host".into())}}
fn migrate_saved_routes(st:&mut Json){
 let dialog=text(st,"dialog");let migrated=legacy_route(&dialog);
 if migrated!=dialog{put(st,"dialog",s(migrated));}
 let items=match get(st,"workspace_stack"){Arr(items)=>items.as_slice(),_=>&[][..]};
 let start=items.len().saturating_sub(12);
 let mut changed=start>0;
 let next:Vec<Json>=items[start..].iter().map(|item|{if let Str(name)=item{let migrated=legacy_route(name);if migrated!=name.as_str(){changed=true;return s(migrated);}}item.clone()}).collect();
 if changed{put(st,"workspace_stack",Arr(next));}
}
const SAVED_STATE_LIMIT:usize=6*1024*1024-4096;
const EVENT_INPUT_LIMIT:usize=8*1024*1024;
const PROVIDER_TEXT_CAP:usize=1024*1024;
const PLATFORM_SAVE_LIMIT:usize=6*1024*1024;
const PLATFORM_EVENT_SLACK:usize=32*1024;
const JSON_ESCAPE_MAX:usize=6;
const PROVIDER_REASON_LIMIT:usize=180;
fn provider_request(st:&Json,root:&str,content:&str)->Json{
 let mut inputs=obj(vec![
  ("ticket",s("")),
  ("cloud_response",obj(vec![("content",s(content))])),
  ("phone_access_granted",Bool(true)),
  ("gateway_status",s("")),
  ("device",obj(vec![
   ("memory_bytes",Num("0".into())),("heap_bytes",Num("0".into())),("cores",Num("1".into())),
   ("abi",s(&"a".repeat(64))),
   ("application_id",s(&"a".repeat(256))),
   ("microphone_granted",Bool(false)),("all_files_access",Bool(false)),
   ("device_root_status",s(&"a".repeat(64))),
   ("device_root_enabled",Bool(false)),
  ])),
 ]);
 let draft=text(st,"draft");if !draft.is_empty(){put(&mut inputs,"draft",s(&draft));}
 obj(vec![("op",s("event")),("defer_policy_commit",Bool(true)),("action",s("cloud_text_result")),("state",st.clone()),("inputs",inputs),("root",s(root))])
}
fn provider_event_base(st:&Json,root:&str)->usize{provider_request(st,root,"").string().len().saturating_add(PLATFORM_EVENT_SLACK)}
fn provider_text_growth(content:&str)->usize{quote(content).len().saturating_sub(2)}
fn provider_text_limit(st:&Json,root:&str)->usize{
 let room=EVENT_INPUT_LIMIT.saturating_sub(provider_event_base(st,root));
 (room/JSON_ESCAPE_MAX).min(PROVIDER_TEXT_CAP)
}
fn provider_text_fits(st:&Json,root:&str,content:&str)->bool{
 provider_event_base(st,root).saturating_add(provider_text_growth(content))<=EVENT_INPUT_LIMIT
}
fn cloud_read_effect(doc:&Json,ticket:&str,limit:usize)->Json{
 obj(vec![
  ("kind",s("cloud_read")),("uri",get(doc,"uri").clone()),("mime",get(doc,"mime").clone()),("ticket",s(ticket)),
  ("limit",Num(limit.to_string())),("reason_limit",Num(PROVIDER_REASON_LIMIT.to_string())),
 ])
}
fn bounded_provider_reason(reason:&str)->String{
 let mut out=String::new();
 for c in reason.chars(){if c.is_control(){continue;}let extra=c.len_utf8();if out.len().saturating_add(extra)>PROVIDER_REASON_LIMIT{break;}out.push(c);}
 if out.is_empty(){"Document provider read failed".into()}else{out}
}
fn persisted_bytes(value:&Json)->usize{value.string().len().max(super::mcp_schema::state_bytes(value))}
fn encoded_len(value:&str)->usize{
 let mut n=0usize;
 for c in value.chars(){
  let cost=match c{'"'|'\\'|'/'|'\n'|'\r'|'\t'|'\u{8}'|'\u{c}'=>2,c if c<' '=>6,c=>c.len_utf8()};
  n=n.saturating_add(cost);
 }
 n
}
fn failure_state(st:&Json,notice:&str)->Json{
 let mut candidate=st.clone();
 put(&mut candidate,"notice",s(notice));
 put(&mut candidate,"notice_error",Bool(true));
 candidate
}
fn fit_provider_notice(st:&Json,reason:&str)->String{
 let full=bounded_provider_reason(reason);
 let base_bytes=persisted_bytes(&failure_state(st,""));
 let fits=|notice:&str,slack:usize| base_bytes.saturating_add(encoded_len(notice)).saturating_add(slack)<=PLATFORM_SAVE_LIMIT;
 let marker=" [shortened to fit the save limit]";
 for slack in [256usize,64,0]{
  if fits(&full,slack){return full;}
  let chars:Vec<char>=full.chars().collect();
  let mut best:Option<String>=None;
  let mut lo=0usize;
  let mut hi=chars.len();
  while lo<hi{
   let mid=lo+(hi-lo+1)/2;
   let mut trial:String=chars.iter().take(mid).collect();
   if mid<chars.len(){trial.push_str(marker);}
   if fits(&trial,slack){best=Some(trial);lo=mid;}
   else if mid==0{break;}
   else{hi=mid-1;}
  }
  if let Some(notice)=best{return notice;}
  if fits("Document provider read failed",slack){return "Document provider read failed".into();}
  if fits("",slack){return String::new();}
 }
 String::new()
}
fn provider_stopped(st:&mut Json,reason:&str)->Result<Json>{
 let notice=fit_provider_notice(st,reason);
 *st=failure_state(st,&notice);
 Err(notice)
}
fn callback_text<'a>(value:&'a Json,key:&str)->Option<&'a str>{match get(value,key){Str(text)=>Some(text.as_str()),_=>None}}
pub(super) fn rows()->Vec<Json>{vec![button("Projects","workspace_projects"),button("Storage","workspace_storage"),button("Tools and servers","mcp_client_menu"),button("Investigation","workspace_research"),button("Programs","workspace_programs"),button("Setup","workspace_setup")]}
pub(super) fn set_access(st:&mut Json,provider:&str,value:bool){let mut access=get(st,"storage_access").clone();if access==Null{access=obj(vec![]);}put(&mut access,provider,Bool(value));put(st,"storage_access",access);}
pub(super) fn allowed(st:&Json,provider:&str)->bool{get(get(st,"storage_access"),provider)!=&Bool(false)}
pub(super) fn check_tool(st:&Json,tool:&str)->Result<()>{
 let provider=if tool.starts_with("phone_"){"phone"}else if tool.starts_with("drive_"){"network"}else if tool=="mcp"{"mcp"}else{return Ok(())};
 if !allowed(st,provider){return Err(format!("{} access is off in Workspace → Setup → Access permissions",match provider{"phone"=>"Device storage","network"=>"Network storage",_=>"MCP server"}));}Ok(())
}
pub(super) fn source_for_purpose(st:&Json,purpose:&str)->&'static str{
 if purpose.starts_with("mcp_client_"){return "mcp";}
 if purpose.starts_with("project_transfer:"){return if boolean(get(st,"project_transfer"),"phone"){"phone"}else{"network"};}
 if purpose.starts_with("drive")||purpose.starts_with("remote_")||purpose.starts_with("storage_")||["saved","saved_verify","saved_probe","project_saved","project_file","file","listing"].contains(&purpose){return "network";}
 ""
}
pub(super) fn check_effect(st:&Json,e:&Json)->Result<()>{
 let kind=text(e,"kind");if kind=="tool"{return check_tool(st,&text(e,"tool"));}
 let provider=if kind=="network"{source_for_purpose(st,&text(e,"purpose"))}else if ["phone_revision","project_phone"].contains(&kind.as_str()){ "phone" }else if kind=="project_payload"{if boolean(get(e,"record"),"phone"){"phone"}else{"network"}}else{""};
 if !provider.is_empty()&&!allowed(st,provider){return check_tool(st,if provider=="phone"{"phone_read"}else if provider=="mcp"{"mcp"}else{"drive_read"});}Ok(())
}
pub(super) fn denied_callback(st:&Json,action:&str,inputs:&Json)->Option<Json>{
 if !["network_result","tool_result","phone_revision_result","project_phone_result"].contains(&action){return None;}
 let source=source_for_purpose(st,&text(inputs,"purpose"));let tool=text(get(st,"operation_expected"),"tool");
 let source=if !source.is_empty(){source}else if tool.starts_with("phone_")||action=="phone_revision_result"{"phone"}else if tool.starts_with("drive_"){ "network" }else{""};
 if source.is_empty()||allowed(st,source){return None;}let mut next=inputs.clone();if let Obj(fields)=&mut next{fields.remove("data");fields.remove("output");}put(&mut next,"error",s("Source access was turned off; the operation result was withheld"));Some(next)
}
fn title(st:&Json)->String{let id=text(st,"workspace_selected");let listed=match get(st,"workspaces"){Arr(items)=>items.as_slice(),_=>&[][..]};listed.iter().take(32).find(|v|text(v,"id")==id).map(|v|text(v,"name")).unwrap_or_else(||"Workspace".into())}
fn popup(title:&str,fields:Vec<Json>,buttons:Vec<Json>)->Json{let mut value=modal(title,fields,buttons);put(&mut value,"close_label",s("Back"));value}
fn setting(st:&Json,key:&str)->String{if let Bool(value)=get(get(st,"edit"),key){if *value{"true".into()}else{"false".into()}}else{allowed(st,key).to_string()}}
pub(super) fn modal_for(st:&Json)->Option<Json>{
 let name=legacy_route(&text(st,"dialog")).to_string();Some(match name.as_str(){
 "workspace_projects"=>popup("Projects",vec![field_ui("current","Current workspace",&title(st),"readonly")],vec![button("New workspace","workspace_new"),button("Switch workspace","workspace_locations"),button("Choose work tree","phone_folder"),button("Project files","workspace_files"),button("Project snapshots","workspace_snapshots")]),
 "workspace_files"=>popup("Project files",vec![],vec![button("Browse files","files"),button("New or edited text file","editor"),button("Read text","reader"),button("Search files","search")]),
 "workspace_storage"=>{let mut buttons=vec![button("Device storage","phone_locations"),button("Network locations","network_locations")];if provider_controls(){buttons.push(button("Document providers","document_providers"));}popup("Storage",vec![],buttons)},
 "phone_locations"=>{let access=crate::host_fs::process_access();let mut buttons=vec![button("Choose device folder","phone_folder"),button("Browse device files","phone_files"),button("Read device text","phone_reader"),button("New device file","phone_editor"),button("Device text revision","phone_update_editor"),button("Last revision","phone_revision_status"),button("Attach selected file to chat","phone_file")];if access.contains("Windows consent prompt"){buttons.insert(0,button("Request administrator access","device_elevation"));}popup("Device storage",vec![field_ui("location","Selected folder",&text(st,"phone_uri"),"readonly"),field_ui("privilege","Device access",&access,"readonly")],buttons)},
 "network_locations"=>popup("Network locations",vec![field_ui("location","Network folder",&endpoint_summary(&text(get(st,"drive"),"url")),"readonly")],vec![button("Browse network files","drive_list"),button("Read network text","drive_reader"),button("New network file","drive_editor"),button("Network text revision","drive_update_editor"),button("New network folder","drive_folder"),button("Copy text to workspace","drive_copy")]),
 "document_providers"=>popup("Document providers",vec![field_ui("account","File access","Choose an installed document provider or a local file in the device file locations. Provider accounts are managed by the selected app.","readonly")],vec![button("Browse provider documents","cloud_browse")]),
 "workspace_setup"=>{let mut buttons=vec![button(if cfg!(windows){"Device storage"}else{"Device storage and scope"},"phone_access"),button("Network drive connection","drive_edit")];if provider_controls(){buttons.push(button("Document provider access","cloud_setup"));}buttons.extend([button("MCP connections","mcp_client_connections"),button("Edit active MCP connection","mcp_client_settings"),button("Access permissions","workspace_permissions"),button("Default saves","workspace_defaults")]);popup("Setup",vec![],buttons)},
 "workspace_permissions"=>{let mut fields=vec![field_ui("phone","Device storage access",&setting(st,"phone"),"check"),field_ui("network","Network storage access",&setting(st,"network"),"check")];if provider_controls(){fields.push(field_ui("cloud","Document provider access",&setting(st,"cloud"),"check"));}fields.push(field_ui("mcp","MCP server access",&setting(st,"mcp"),"check"));popup("Access permissions",fields,vec![button("Apply permissions","workspace_access_save")])},
 "workspace_defaults"=>popup("Default saves",vec![field_ui("phone_default","Use selected device folder",if boolean(get(st,"drive"),"phone_default"){"true"}else{"false"},"check"),field_ui("autosave","Save completed replies",if boolean(get(st,"drive"),"autosave"){"true"}else{"false"},"check")],vec![button("Apply default saves","workspace_defaults_save")]),
 "workspace_new"=>popup("New workspace",vec![field_ui("name","Workspace name",&text(get(st,"edit"),"name"),"text")],vec![button("Create workspace","workspace_create")]),
 "workspace_locations"=>{let mut buttons=vec![button("Original workspace","workspace_select:default")];let listed=match get(st,"workspaces"){Arr(items)=>items.as_slice(),_=>&[][..]};for v in listed.iter().take(32){buttons.push(button(&text(v,"name"),&format!("workspace_select:{}",text(v,"id"))));}let fields=if listed.len()>32{vec![field_ui("coverage","Workspace list","partial: workspace list stopped at 32 locations","readonly")]}else{vec![]};popup("Workspaces",fields,buttons)},
 "workspace_snapshots"=>popup("Project snapshots",vec![field_ui("archive_path","Project archive path",&text(get(st,"edit"),"archive_path"),"text")],vec![button("Export project ZIP","project_export"),button("Import project ZIP","project_import"),button("Save project to default storage","project_save"),button("Save project to network storage","project_save_drive"),button("Save a project file","artifact")]),
 "workspace_programs"=>popup("Programs",vec![],vec![button("Shell program check","script_check"),button("Shell program execution","script_run")]),
 "workspace_research"=>popup("Investigation",vec![],vec![button("Question and design process","inquiry"),button("File investigation","investigation"),button("Web search","web_search"),button("Observations","investigation_records"),button("Hypotheses","investigation_claims"),button("Experiments","investigation_experiments"),button("Reconstruction","reconstruction_new")]),
 "cloud_setup"=>popup("Document provider access",vec![field_ui("access","Allow document provider access",if allowed(st,"cloud"){"true"}else{"false"},"check"),field_ui("accounts","Accounts","Use the selected provider's account setup or the device account settings. No particular provider is required.","readonly")],vec![button("Apply access","workspace_source_access:cloud"),button("Add a provider account","cloud_sign_in"),button("Access permissions","workspace_permissions"),button("Remove selected document grants","cloud_disconnect")]),
 "cloud_selected"=>{if !allowed(st,"cloud"){return Some(popup("Document providers",vec![field_ui("access","Source access","Off","readonly")],vec![button("Access permissions","workspace_permissions")]));}let doc=get(st,"cloud_document");let mut buttons=vec![button("Open selected file","cloud_open"),button("Read selected text","cloud_read")];if get(doc,"content")!=&Null{buttons.push(button("Copy text to workspace","cloud_copy"));}popup("Provider document",vec![field_ui("name","File",&text(doc,"name"),"readonly"),field_ui("mime","Type",&text(doc,"mime").replace("\r","").replace("\n",""),"readonly"),field_ui("preview","Text",&text(doc,"content"),"readonly")],buttons)},
 _=>return None,
 })
}
pub(super) fn remember(st:&mut Json,action:&str){
 migrate_saved_routes(st);
 if ["workspace","chat","history","config","settings","navigation","agent_settings"].contains(&action){put(st,"workspace_stack",Arr(vec![]));return;}
 let navigation=SECTIONS.contains(&action)||action.starts_with("mcp_tool:")||["cloud_setup","cloud_browse","cloud_read","mcp_client_menu","mcp_tools","mcp_streams","mcp_client_connections","mcp_client_new","mcp_client_settings","mcp_client_call","mcp_client_review","phone_access","drive_edit","phone_files","phone_reader","phone_editor","phone_update_editor","phone_revision_status","drive_list","drive_reader","drive_editor","drive_update_editor","drive_folder","drive_copy","files","editor","reader","search","investigation","web_search","script_check","script_run","project_export","project_import","project_save","project_save_drive","artifact","inquiry","investigation_records","investigation_claims","investigation_experiments","reconstruction_new"].contains(&action);
 if !navigation{return;}let current=text(st,"dialog");if current.is_empty(){put(st,"workspace_stack",Arr(vec![]));return;}
 if !SECTIONS.contains(&current.as_str())&&!["cloud_setup","mcp_client_menu","mcp_client_connections","mcp_tools","mcp_streams","mcp_client_call","storage_settings"].contains(&current.as_str()){return;}if current==action{return;}
 let mut stack=array(st,"workspace_stack");if stack.len()<12&&stack.last()!=Some(&s(&current)){stack.push(s(&current));put(st,"workspace_stack",Arr(stack));}
}

pub(super) fn back(st:&mut Json)->bool{
 let current=text(st,"dialog");if current=="mcp_client_review"{put(st,"mcp_client_pending",Null);}
 let items=match get(st,"workspace_stack"){Arr(items)=>items.as_slice(),_=>&[][..]};
 let start=items.len().saturating_sub(12);
 let mut stack=items[start..].to_vec();
 if let Some(previous)=stack.pop(){put(st,"workspace_stack",Arr(stack));put(st,"dialog",previous);return true;}
 if SECTIONS.contains(&current.as_str())||current=="cloud_setup"{put(st,"dialog",Null);return true;}false
}
pub(super) fn handles(action:&str)->bool{retired_provider_route(action)||SECTIONS.contains(&action)||["workspace_access_save","workspace_defaults_save","workspace_create","cloud_setup","cloud_browse","cloud_sign_in","cloud_result","cloud_text_result","cloud_release_result","cloud_read","cloud_open","cloud_copy","cloud_disconnect","cloud_cancel","cloud_failure","project_import","archive_accept","archive_reject"].contains(&action)||action.starts_with("workspace_source_access:")||action.starts_with("workspace_select:")}
fn plain_directory_name(name:&str)->bool{
 if name.is_empty()||directory_name_too_long(name)||name.chars().any(|c|c.is_control()||c=='/'||c=='\\'){return false;}
 let mut parts=std::path::Path::new(name).components();
 match parts.next(){Some(std::path::Component::Normal(part)) if parts.next().is_none()=>part==std::ffi::OsStr::new(name),_=>false}
}
#[cfg(windows)]
fn directory_name_too_long(name:&str)->bool{name.encode_utf16().count()>255}
#[cfg(not(windows))]
fn directory_name_too_long(name:&str)->bool{name.len()>255}
fn ordinary_directory(info:&std::fs::Metadata)->bool{
 if !info.is_dir()||info.file_type().is_symlink(){return false;}
 #[cfg(windows)]{use std::os::windows::fs::MetadataExt;if info.file_attributes()&0x400!=0{return false;}}
 true
}
fn ordinary_file(info:&std::fs::Metadata)->bool{
 if !info.is_file()||info.file_type().is_symlink(){return false;}
 #[cfg(windows)]{use std::os::windows::fs::MetadataExt;if info.file_attributes()&0x400!=0{return false;}}
 true
}
fn root_for(st:&Json,root:&str,id:&str)->Result<PathBuf>{
 let current=Path::new(root).canonicalize().map_err(|e|e.to_string())?;let parent=current.parent().ok_or("Workspace parent unavailable")?;
 let base=if text(st,"workspace_selected").is_empty(){current.file_name().ok_or("Workspace name unavailable")?.to_string_lossy().into_owned()}else{text(st,"workspace_original")};
 let leaf=if id.is_empty(){base}else{format!("workspace-{id}")};
 if !plain_directory_name(&leaf){return Err("Original workspace location changed".into());}
 let target=parent.join(&leaf);if target.parent()!=Some(parent){return Err("Original workspace location changed".into());}
 Ok(target)
}
fn select(st:&mut Json,root:&str,id:&str)->Result<()>{
 if interaction::locked(st)||get(st,"pending")!=&Null||get(st,"mcp_client_pending")!=&Null||get(st,"project_transfer")!=&Null{return Err("Finish the current operation before switching workspaces".into());}
 let target=root_for(st,root,id)?;let info=fs::symlink_metadata(&target).map_err(|e|e.to_string())?;if !ordinary_directory(&info){return Err("Choose an existing app workspace directory".into());}
 let resolved=fs::canonicalize(&target).map_err(|e|e.to_string())?;let anchor=Path::new(root).canonicalize().map_err(|e|e.to_string())?;let anchor_parent=anchor.parent().ok_or("Workspace parent unavailable")?;
 if resolved.parent()!=Some(anchor_parent){return Err("Original workspace location changed".into());}
 if text(st,"workspace_selected").is_empty(){let name=anchor.file_name().ok_or("Workspace name unavailable")?.to_string_lossy().into_owned();if !plain_directory_name(&name){return Err("Original workspace location changed".into());}put(st,"workspace_original",s(&name));}
 put(st,"workspace_selected",s(id));approval_rules::namespace(st,&resolved.to_string_lossy());harness_rules::identity_refresh(st);put(st,"dialog",s("workspace_projects"));put(st,"page",s("workspace"));put(st,"workspace_stack",Arr(vec![]));put(st,"cloud_expected",Null);inquiry::forget(st);Ok(())
}
fn cloud_owner(st:&mut Json,phase:&str)->String{let ticket=now();let owner=obj(vec![("ticket",s(&ticket)),("phase",s(phase)),("root",get(st,"approval_root").clone()),("identity",get(st,"approval_root_identity").clone())]);put(st,"cloud_expected",owner);ticket}
fn document_uri(uri:&str)->bool{let Some(rest)=uri.strip_prefix("content://")else{return false};let Some((authority,path))=rest.split_once('/')else{return false};!authority.is_empty()&&!path.is_empty()&&uri.len()<=8192&&authority.bytes().all(|b|b.is_ascii_alphanumeric()||b"._-".contains(&b))&&!uri.chars().any(char::is_control)&&!uri.contains('#')}
const GRANT_CAP:usize=64;
fn document_grants(st:&Json)->Vec<Json>{
 let mut grants=Vec::new();
 let saved=match get(st,"cloud_grants"){Arr(items)=>items.as_slice(),_=>&[][..]};
 let current=get(get(st,"cloud_document"),"uri");
 for uri in saved{
  if grants.len()>=GRANT_CAP{break;}
  let Str(value)=uri else{continue};
  if document_uri(value)&&grants.iter().all(|kept|kept!=uri){grants.push(uri.clone());}
 }
 if let Str(value)=current{if document_uri(value)&&grants.iter().all(|kept|kept!=current){
  if grants.len()>=GRANT_CAP{grants.pop();}
  grants.push(current.clone());
 }}
 grants
}
fn host_archive_chooser()->bool{cfg!(windows)||cfg!(target_os="android")}
static PENDING_ARCHIVE:std::sync::Mutex<Option<Vec<u8>>>=std::sync::Mutex::new(None);
fn pending_archive()->std::sync::MutexGuard<'static,Option<Vec<u8>>>{PENDING_ARCHIVE.lock().unwrap_or_else(|poisoned|poisoned.into_inner())}
fn clear_pending_archive(){*pending_archive()=None;}
fn take_pending_archive()->Option<Vec<u8>>{pending_archive().take()}
fn keep_pending_archive(bytes:Vec<u8>){*pending_archive()=Some(bytes);}
fn base64_len(len:u64)->Option<u64>{len.checked_add(2).map(|n|n/3).and_then(|n|n.checked_mul(4))}
/// Largest raw project archive whose encoding and decoded copy both fit the
/// same memory budget project saves use, and the snapshot file cap.
fn archive_fits(raw:u64)->bool{
 let budget=crate::archive::project_budget();
 if raw>budget.min(256*1024*1024){return false;}
 let Some(encoded)=base64_len(raw) else {return false;};
 let Some(zip_cap)=encoded.checked_add(3).map(|n|n/4*3) else {return false;};
 match (raw.checked_add(encoded),encoded.checked_add(zip_cap)){
  (Some(encode_peak),Some(preview_peak))=>encode_peak<=budget&&preview_peak<=budget,
  _=>false,
 }
}
fn archive_byte_limit()->u64{
 let mut lo=0u64;
 let mut hi=crate::archive::project_budget().min(256*1024*1024);
 while lo<hi{
  let mid=lo+(hi-lo)/2+1;
  if archive_fits(mid){lo=mid;}else if mid==0{break;}else{hi=mid-1;}
 }
 lo
}
fn open_archive_file(path:&str)->Result<std::fs::File>{
 let mut options=std::fs::OpenOptions::new();
 options.read(true);
 #[cfg(windows)]
 {use std::os::windows::fs::OpenOptionsExt;let _=options.custom_flags(0x0020_0000);}
 #[cfg(any(target_os="linux",target_os="android"))]
 {use std::os::unix::fs::OpenOptionsExt;
  #[cfg(any(target_arch="aarch64",target_arch="arm",target_arch="powerpc",target_arch="powerpc64"))]
  {let _=options.custom_flags(0x8000);}
  #[cfg(not(any(target_arch="aarch64",target_arch="arm",target_arch="powerpc",target_arch="powerpc64")))]
  {let _=options.custom_flags(0x20000);}
 }
 #[cfg(any(target_os="macos",target_os="ios",target_os="tvos",target_os="watchos",target_os="visionos"))]
 {use std::os::unix::fs::OpenOptionsExt;let _=options.custom_flags(0x100);}
 options.open(path).map_err(|e|format!("The project archive could not be opened: {e}"))
}
fn read_archive_file(path:&str)->Result<Vec<u8>>{
 if path.is_empty()||path.len()>4096||path.chars().any(char::is_control){return Err("Enter a project archive path".into());}
 let listed=fs::symlink_metadata(path).map_err(|e|format!("The project archive could not be opened: {e}"))?;
 if !ordinary_file(&listed){return Err("Select a project archive file".into());}
 let mut file=open_archive_file(path)?;
 let info=file.metadata().map_err(|e|format!("The project archive could not be opened: {e}"))?;
 if !ordinary_file(&info)||info.len()!=listed.len(){return Err("Select a project archive file".into());}
 let limit=archive_byte_limit();
 if info.len()>limit{return Err("Project archive exceeds the memory budget".into());}
 let size=usize::try_from(info.len()).map_err(|_| "Project archive exceeds addressable memory".to_string())?;
 let mut bytes=Vec::new();
 bytes.try_reserve_exact(size).map_err(|_| "Project archive exceeds the memory budget".to_string())?;
 let mut offset=0usize;
 let mut scratch=[0u8;65536];
 while offset<size{
  if crate::execution::cancelled(){return Err("Project import cancelled before completion".into());}
  let want=size-offset;
  let take=want.min(scratch.len());
  let n=file.read(&mut scratch[..take]).map_err(|e|format!("The project archive could not be read: {e}"))?;
  if n==0{return Err("Project archive changed while reading".into());}
  let end=offset.checked_add(n).ok_or("Project archive offset overflow")?;
  bytes.extend_from_slice(&scratch[..n]);
  offset=end;
 }
 let mut extra=[0u8;1];
 if file.read(&mut extra).map_err(|e|format!("The project archive could not be read: {e}"))?!=0||bytes.len()!=size{
  return Err("Project archive changed while reading".into());
 }
 Ok(bytes)
}
fn encode_archive(bytes:&[u8])->Result<String>{
 let raw=u64::try_from(bytes.len()).map_err(|_| "Project archive exceeds the memory budget")?;
 if !archive_fits(raw){return Err("Project archive exceeds the memory budget".into());}
 let chars=usize::try_from(base64_len(raw).ok_or("Project archive exceeds the memory budget")?).map_err(|_| "Project archive exceeds the memory budget")?;
 let mut encoded=String::new();
 encoded.try_reserve_exact(chars).map_err(|_| "Project archive exceeds the memory budget".to_string())?;
 let table=b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
 for chunk in bytes.chunks(3){
  let n=((chunk[0] as u32)<<16)|((chunk.get(1).copied().unwrap_or(0) as u32)<<8)|(chunk.get(2).copied().unwrap_or(0) as u32);
  for i in 0..4{
   if i>chunk.len(){encoded.push('=');}else{encoded.push(table[((n>>(18-6*i))&63) as usize] as char);}
  }
 }
 if encoded.len()!=chars{return Err("Project archive encoding length mismatch".into());}
 Ok(encoded)
}
fn stage_archive_review(st:&mut Json,bytes:Vec<u8>)->Result<Json>{
 let encoded=encode_archive(&bytes)?;
 drop(bytes);
 let previewed=crate::archive::preview(&obj(vec![("archive",s(&encoded))]))?;
 let raw=crate::network::unbase64(&encoded)?;
 drop(encoded);
 let mut candidate=st.clone();
 put(&mut candidate,"busy",Bool(false));
 put(&mut candidate,"archive_names",get(&previewed,"names").clone());
 put(&mut candidate,"archive_bytes",get(&previewed,"bytes").clone());
 put(&mut candidate,"dialog",s("archive_review"));
 put(&mut candidate,"notice",s("Review the project filenames before creating files"));
 put(&mut candidate,"notice_error",Bool(false));
 if persisted_bytes(&candidate)>SAVED_STATE_LIMIT.min(PLATFORM_SAVE_LIMIT){return Err("Project archive listing does not fit saved state".into());}
 keep_pending_archive(raw);
 *st=candidate;
 Ok(Null)
}
fn import_project(st:&mut Json,inputs:&Json)->Result<Json>{
 let path=text(inputs,"archive_path").trim().to_string();
 clear_pending_archive();
 if path.is_empty(){
  if !host_archive_chooser(){return Err("Enter a project archive path".into());}
  put(st,"busy",Bool(true));
  put(st,"notice",s("Select a project archive for review"));
  return Ok(obj(vec![("kind",s("document_zip"))]));
 }
 let bytes=read_archive_file(&path)?;
 stage_archive_review(st,bytes)
}
pub(super) fn cloud_callback(action:&str)->bool{matches!(action,"cloud_result"|"cloud_text_result"|"cloud_release_result"|"cloud_cancel"|"cloud_failure")}
pub(super) fn cloud_response(inputs:&Json)->&Json{if get(inputs,"cloud_response")==&Null{inputs}else{get(inputs,"cloud_response")}}
pub(super) fn cloud_expected(st:&Json,action:&str,inputs:&Json)->bool{
 let owner=get(st,"cloud_expected");let phase=text(owner,"phase");!text(owner,"ticket").is_empty()&&text(owner,"ticket")==text(inputs,"ticket")&&get(owner,"root")==get(st,"approval_root")&&get(owner,"identity")==get(st,"approval_root_identity")&&matches!(phase.as_str(),"select"|"read"|"release")&&match action{"cloud_result"=>phase=="select","cloud_text_result"=>phase=="read","cloud_release_result"=>phase=="release","cloud_cancel"|"cloud_failure"=>true,_=>false}
}
pub(super) fn event(st:&mut Json,action:&str,inputs:&Json,root:&str)->Result<Json>{
 migrate_saved_routes(st);
 if action=="cloud_release_result"{if !cloud_expected(st,action,inputs){return Ok(Null);}let response=cloud_response(inputs);let mut retained=document_grants(st);if let Arr(released)=get(response,"released"){for item in released{if retained.is_empty(){break;}if let Some(pos)=retained.iter().position(|uri|uri==item){retained.remove(pos);}}}let failed=!retained.is_empty();let notice=if failed{"Some selected document grants could not be released; use Remove selected document grants to retry. Access remains off."}else{"Selected document grants removed; unrelated folder grants retained"};let mut candidate=st.clone();put(&mut candidate,"cloud_grants",Arr(retained));put(&mut candidate,"cloud_expected",Null);put(&mut candidate,"notice",s(notice));put(&mut candidate,"notice_error",Bool(failed));if persisted_bytes(&candidate)>SAVED_STATE_LIMIT.min(PLATFORM_SAVE_LIMIT){put(st,"cloud_expected",Null);return provider_stopped(st,"Document grant update does not fit saved state; the previous grant list is unchanged");}*st=candidate;return Ok(Null);}
 if let Some(provider)=action.strip_prefix("workspace_source_access:"){if !["phone","cloud"].contains(&provider){return Err("Choose a storage source".into());}let required=if provider=="phone"{"phone_access"}else{"cloud_setup"};if text(st,"dialog")!=required{return Err("Open the source setup first".into());}let Bool(value)=get(inputs,"access")else{return Err("Choose the source access toggle".into());};set_access(st,provider,*value);return Ok(obj(vec![("kind",s("storage_policy"))]));}
 if SECTIONS.contains(&action)||action=="cloud_setup"||retired_provider_route(action){put(st,"edit",obj(vec![]));put(st,"dialog",s(legacy_route(action)));return Ok(Null);}
 match action{
 "workspace_access_save"=>{if text(st,"dialog")!="workspace_permissions"{return Err("Open source access permissions first".into());}let mut access=obj(vec![]);let mut keys=vec!["phone","network","mcp"];if provider_controls(){keys.insert(2,"cloud");}for key in keys{let Bool(value)=get(inputs,key)else{return Err("Choose each source access toggle".into());};put(&mut access,key,Bool(*value));}if !provider_controls(){if get(get(st,"storage_access"),"cloud")!=&Null{put(&mut access,"cloud",get(get(st,"storage_access"),"cloud").clone());}}put(st,"storage_access",access);put(st,"notice",s("Source access permissions applied; connection settings retained"));return Ok(obj(vec![("kind",s("storage_policy"))]));},
 "workspace_defaults_save"=>{if text(st,"dialog")!="workspace_defaults"{return Err("Open Default saves first".into());}let phone=boolean(inputs,"phone_default");if phone&&text(st,"phone_uri").is_empty(){return Err("Select a device folder in Setup first".into());}if !phone{drive_root(st)?;}let mut drive=get(st,"drive").clone();if drive==Null{drive=obj(vec![]);}put(&mut drive,"phone_default",Bool(phone));put(&mut drive,"autosave",Bool(boolean(inputs,"autosave")));put(st,"drive",drive);put(st,"notice",s("Default saves applied"));},
 "workspace_create"=>{
 if text(st,"dialog")!="workspace_new"||interaction::locked(st)||get(st,"pending")!=&Null||get(st,"mcp_client_pending")!=&Null||get(st,"project_transfer")!=&Null{return Err("Open New workspace after the current operation finishes".into());}let name=text(inputs,"name").trim().to_string();if name.is_empty()||name.len()>128||name.chars().any(char::is_control){return Err("Enter a workspace name of1–128 UTF-8 bytes".into());}let existing=match get(st,"workspaces"){Arr(items)=>items.as_slice(),_=>&[]};if existing.len()>=32{return Err("The workspace list holds32 locations".into());}let mut list=existing.to_vec();
 let id=format!("{}-{}",now(),list.len());let target=root_for(st,root,&id)?;list.push(obj(vec![("id",s(&id)),("name",s(&name))]));let mut candidate=st.clone();put(&mut candidate,"workspaces",Arr(list.clone()));if persisted_bytes(&candidate)>6*1024*1024-8192{return Err("Workspace settings exceed the current saved-state allowance".into());}fs::create_dir(&target).map_err(|e|e.to_string())?;put(st,"workspaces",Arr(list));select(st,root,&id)?;put(st,"notice",s(&format!("Workspace {name} created")));},
 "cloud_sign_in"=>{require_provider()?;return Ok(obj(vec![("kind",s("cloud_account"))]));},
 "cloud_browse"=>{require_provider()?;if !allowed(st,"cloud"){put(st,"edit",obj(vec![]));put(st,"dialog",s("workspace_permissions"));return Ok(Null);}if text(get(st,"cloud_expected"),"phase")=="read"{return Err("Finish or cancel the selected document read first".into());}if document_grants(st).len()>=GRANT_CAP{return Err("Remove selected document grants before adding more documents".into());}let ticket=cloud_owner(st,"select");return Ok(obj(vec![("kind",s("cloud_document")),("ticket",s(&ticket))]));},
 "cloud_open"|"cloud_read"=>{require_provider()?;if !allowed(st,"cloud"){return Err("Document provider access is off in Setup".into());}let doc=get(st,"cloud_document").clone();if !document_uri(&text(&doc,"uri")){return Err("Select a provider document first".into());}if action=="cloud_open"{return Ok(obj(vec![("kind",s("cloud_open")),("uri",get(&doc,"uri").clone()),("mime",get(&doc,"mime").clone())]));}if interaction::locked(st)||get(st,"cloud_expected")!=&Null{return Err("Finish or cancel the current operation before reading the selected document".into());}let limit=provider_text_limit(st,root);if limit==0{return Err("Selected text does not fit the current event input limit".into());}let ticket=cloud_owner(st,"read");let mut owner=get(st,"cloud_expected").clone();put(&mut owner,"limit",Num(limit.to_string()));put(&mut owner,"reason_limit",Num(PROVIDER_REASON_LIMIT.to_string()));put(st,"cloud_expected",owner);put(st,"busy",Bool(true));return Ok(cloud_read_effect(&doc,&ticket,limit));},
 "cloud_copy"=>{require_provider()?;if !allowed(st,"cloud"){return Err("Document provider access is off in Setup".into());}let doc=get(st,"cloud_document");let Str(content)=get(doc,"content")else{return Err("Read the selected text first".into());};let name=text(doc,"name");let name=if name.is_empty()||name.contains('/')||name.contains('\\')||name==".."{"selected.txt"}else{&name};let edit=obj(vec![("path",s(name)),("content",s(content))]);let mut candidate=st.clone();put(&mut candidate,"edit",edit);put(&mut candidate,"dialog",s("editor"));if persisted_bytes(&candidate)>SAVED_STATE_LIMIT.min(PLATFORM_SAVE_LIMIT){return Err("Selected text does not fit saved state".into());}*st=candidate;},
 "cloud_disconnect"=>{require_provider()?;let grants=document_grants(st);if text(get(st,"cloud_expected"),"phase")=="read"{put(st,"busy",Bool(false));}put(st,"cloud_document",Null);put(st,"cloud_grants",Arr(grants.clone()));let ticket=cloud_owner(st,"release");let mut access=get(st,"storage_access").clone();if access==Null{access=obj(vec![]);}put(&mut access,"cloud",Bool(false));put(st,"storage_access",access);return Ok(obj(vec![("kind",s("cloud_release")),("uris",Arr(grants)),("ticket",s(&ticket))]));},
 "cloud_result"|"cloud_text_result"|"cloud_cancel"|"cloud_failure"=>{
 if !cloud_expected(st,action,inputs){return Ok(Null);}let phase=text(get(st,"cloud_expected"),"phase");let published_limit=match get(get(st,"cloud_expected"),"limit"){Num(n)=>n.parse::<usize>().ok(),_=>None};put(st,"cloud_expected",Null);if phase=="read"{put(st,"busy",Bool(false));}if !allowed(st,"cloud"){put(st,"notice",s("Document provider access is off"));return Ok(Null);}
 let response=cloud_response(inputs);
 if action=="cloud_result"{let Some(uri)=callback_text(response,"uri")else{return provider_stopped(st,"Invalid selected document metadata");};let Some(name)=callback_text(response,"name")else{return provider_stopped(st,"Invalid selected document metadata");};let Some(mime)=callback_text(response,"mime")else{return provider_stopped(st,"Invalid selected document metadata");};let uri=uri.to_string();let name=name.to_string();let mime=mime.to_string();if !document_uri(&uri)||name.len()>512||mime.len()>256||mime.chars().any(char::is_control)||name.chars().any(char::is_control){return provider_stopped(st,"Invalid selected document metadata");}let mut grants=document_grants(st);if !grants.contains(&s(&uri)){if grants.len()>=GRANT_CAP{return provider_stopped(st,"Selected document grant allowance is full");}grants.push(s(&uri));}let mut candidate=st.clone();put(&mut candidate,"cloud_grants",Arr(grants));put(&mut candidate,"cloud_document",obj(vec![("uri",s(&uri)),("name",s(&name)),("mime",s(&mime))]));put(&mut candidate,"dialog",s("cloud_selected"));put(&mut candidate,"notice",s("Provider document selected"));put(&mut candidate,"notice_error",Bool(false));if persisted_bytes(&candidate)>SAVED_STATE_LIMIT.min(PLATFORM_SAVE_LIMIT){return provider_stopped(st,"Selected document does not fit saved state; the previous document is unchanged");}*st=candidate;}
 else if action=="cloud_text_result"{let Str(content)=get(response,"content") else {return provider_stopped(st,"Provider text was not supplied");};if content.len()>PROVIDER_TEXT_CAP{return provider_stopped(st,"Selected text exceeds1MiB");}if published_limit.is_some_and(|limit|content.len()>limit){return provider_stopped(st,"Selected text exceeds the published read limit");}if !provider_text_fits(st,root,content){return provider_stopped(st,"Selected text exceeds the current event input limit");}let mut doc=get(st,"cloud_document").clone();put(&mut doc,"content",s(content));let mut candidate=st.clone();put(&mut candidate,"cloud_document",doc);put(&mut candidate,"dialog",s("cloud_selected"));put(&mut candidate,"busy",Bool(false));put(&mut candidate,"notice",s("Selected provider text opened"));put(&mut candidate,"notice_error",Bool(false));if persisted_bytes(&candidate)>SAVED_STATE_LIMIT.min(PLATFORM_SAVE_LIMIT){put(st,"busy",Bool(false));return provider_stopped(st,"Selected text does not fit saved state; the previous document is unchanged");}*st=candidate;}else if action=="cloud_failure"{let reason=match get(response,"error"){Str(value) if !value.is_empty()=>value.as_str(),_=>"Document provider read failed"};return provider_stopped(st,reason);}},
 "project_import"=>return import_project(st,inputs),
 "archive_accept"=>{
  if text(st,"dialog")!="archive_review"{return Err("No project import awaiting approval".into());}
  if let Some(bytes)=take_pending_archive(){
   let encoded=match encode_archive(&bytes){Ok(encoded)=>encoded,Err(error)=>{keep_pending_archive(bytes);return Err(error);}};
   drop(bytes);
   match crate::archive::import(&obj(vec![("archive",s(&encoded)),("root",s(root))])){
    Ok(report)=>{
     drop(encoded);
     put(st,"dialog",Null);put(st,"busy",Bool(false));put(st,"active_import",Bool(false));
     put(st,"archive_names",Null);put(st,"archive_bytes",Null);
     put(st,"notice",s(&format!("Created {} project files and {} folders",number(&report,"files"),number(&report,"directories"))));
     put(st,"notice_error",Bool(false));
     return Ok(Null);
    }
    Err(error)=>{
     if let Ok(raw)=crate::network::unbase64(&encoded){keep_pending_archive(raw);}
     put(st,"busy",Bool(false));put(st,"active_import",Bool(false));
     return Err(error);
    }
   }
  }
  put(st,"dialog",Null);put(st,"busy",Bool(true));put(st,"active_import",Bool(true));
  return Ok(obj(vec![("kind",s("archive_import"))]));
 },
 "archive_reject"=>{
  clear_pending_archive();put(st,"busy",Bool(false));put(st,"dialog",Null);put(st,"archive_names",Null);
  put(st,"notice",s("Project import cancelled"));return Ok(obj(vec![("kind",s("archive_clear"))]));
 },
 _=>{if let Some(id)=action.strip_prefix("workspace_select:"){let id=if id=="default"{""}else{id};if !id.is_empty(){let listed=match get(st,"workspaces"){Arr(items)=>items.as_slice(),_=>&[]};if listed.len()>32{return Err("The workspace list holds32 locations".into());}if !listed.iter().any(|v|text(v,"id")==id){return Err("Workspace location unavailable".into());}}select(st,root,id)?;}}
 }Ok(if action=="workspace_create"||action.starts_with("workspace_source_access:")||action.starts_with("workspace_select:"){obj(vec![("kind",s("workspace_selected"))])}else{Null})
}
#[cfg(any(test, rllm_module_tests))]
mod checks {
 use super::*;
 #[cfg(rllm_module_tests)]
 #[no_mangle]
 pub extern "C" fn g2_workspace_tests() {
  provider_text_must_be_a_string_and_fit_saved_state();
  saved_provider_route_migrates_and_desktop_hides_unsupported_controls();
  saved_workspace_original_stays_under_its_parent();
  root_for_accepts_a_two_hundred_character_cjk_name_on_windows();
  provider_failure_commits_cleared_read_on_the_public_event();
  provider_text_stays_inside_the_event_input_limit();
  provider_failure_text_stays_inside_the_save_limit();
  provider_failure_near_the_save_ceiling_stays_saved();
  provider_read_rejects_text_over_the_published_limit();
  snapshot_actions_round_trip_project_bytes();
  provider_selection_requires_text_metadata();
  saved_retired_storage_route_opens_a_workspace_panel();
  provider_grants_stop_at_the_allowance();
  path_import_round_trips_without_a_host_chooser();
  archive_larger_than_five_mib_round_trips_under_the_memory_budget();
  archive_over_the_memory_budget_is_refused();
  chooser_accept_keeps_the_archive_on_the_host();
  oversized_callback_does_not_parse_and_a_bounded_failure_clears_the_read();
  parsed_oversized_failure_clears_the_read();
  workspace_list_over_the_allowance_is_partial();
  published_read_effect_carries_the_callback_limits();
 }
 fn provider_ready()->Json {
  obj(vec![
   ("approval_root",s("")),("approval_root_identity",s("")),
   ("storage_access",obj(vec![("cloud",Bool(true))])),
   ("cloud_expected",obj(vec![("ticket",s("t1")),("phase",s("read")),("root",s("")),("identity",s(""))])),
   ("cloud_document",obj(vec![("uri",s("content://local.provider/doc")),("name",s("a.txt")),("mime",s("text/plain"))])),
   ("busy",Bool(true)),
  ])
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn provider_text_must_be_a_string_and_fit_saved_state() {
  let mut missing=provider_ready();
  let err=event(&mut missing,"cloud_text_result",&obj(vec![("ticket",s("t1")),("content",Bool(true))]),"").unwrap_err();
  assert!(err.contains("not supplied"),"{err}");
  assert_eq!(get(get(&missing,"cloud_document"),"content"),&Null);
  let mut empty=provider_ready();
  event(&mut empty,"cloud_text_result",&obj(vec![("ticket",s("t1")),("content",s(""))]),"").unwrap();
  assert_eq!(text(get(&empty,"cloud_document"),"content"),"");
  assert_eq!(text(&empty,"notice"),"Selected provider text opened");
  let mut heavy=provider_ready();
  put(&mut heavy,"padding",s(&"x".repeat(5*1024*1024)));
  let err=event(&mut heavy,"cloud_text_result",&obj(vec![("ticket",s("t1")),("content",s(&"y".repeat(1024*1024)))]),"").unwrap_err();
  assert!(err.contains("saved state"),"{err}");
  assert_eq!(get(get(&heavy,"cloud_document"),"content"),&Null);
  assert_eq!(get(&heavy,"busy"),&Bool(false));
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn saved_provider_route_migrates_and_desktop_hides_unsupported_controls() {
  assert!(!retired_provider_route("document_providers"));
  assert!(!retired_provider_route("workspace_storage"));
  assert!(!handles("retired_storage_route"));
  let mut saved=obj(vec![("dialog",s("document_providers")),("workspace_stack",Arr(vec![s("cloud_selected"),s("cloud_setup")]))]);
  remember(&mut saved,"workspace_files");
  #[cfg(target_os="android")]
  {assert_eq!(text(&saved,"dialog"),"document_providers");}
  #[cfg(not(target_os="android"))]
  {
   assert_eq!(text(&saved,"dialog"),"workspace_storage");
   let stack=array(&saved,"workspace_stack");
   assert!(stack.iter().any(|item|matches!(item,Str(name) if name=="workspace_storage")));
   assert!(stack.iter().any(|item|matches!(item,Str(name) if name=="workspace_setup")));
   assert!(stack.iter().all(|item|matches!(item,Str(name) if name!="cloud_selected"&&name!="cloud_setup")));
   let storage=modal_for(&obj(vec![("dialog",s("workspace_storage"))])).unwrap().string();
   let setup=modal_for(&obj(vec![("dialog",s("workspace_setup"))])).unwrap().string();
   let providers=modal_for(&obj(vec![("dialog",s("document_providers"))])).unwrap().string();
   assert!(!storage.contains("Document providers"),"{storage}");
   assert!(!setup.contains("Document provider access"),"{setup}");
   assert!(!providers.contains("Document providers"),"{providers}");
   let permissions=modal_for(&obj(vec![("dialog",s("workspace_permissions"))])).unwrap().string();
   assert!(!permissions.contains("Document provider access"),"{permissions}");
   let mut access=obj(vec![("dialog",s("workspace_permissions")),("storage_access",obj(vec![("cloud",Bool(false))]))]);
   event(&mut access,"workspace_access_save",&obj(vec![("phone",Bool(true)),("network",Bool(true)),("mcp",Bool(false))]),"").unwrap();
   assert_eq!(get(get(&access,"storage_access"),"cloud"),&Bool(false));
   assert!(event(&mut obj(vec![]),"cloud_sign_in",&obj(vec![]),"").unwrap_err().contains("not available"));
  }
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn saved_workspace_original_stays_under_its_parent() {
  let scratch=std::env::temp_dir().join(format!("ws-escape-{}",super::super::now()));
  let workspace=scratch.join("deep").join("nest").join("real-workspace");
  let outside=scratch.join("outside");
  std::fs::create_dir_all(&workspace).unwrap();
  std::fs::create_dir_all(&outside).unwrap();
  let root=workspace.to_str().unwrap().to_string();
  let outside_canon=outside.canonicalize().unwrap().to_string_lossy().into_owned();
  for hostile in ["..\\..\\outside","../../outside","..\\outside",".."] {
   let mut st=obj(vec![("workspace_selected",s("kept")),("workspace_original",s(hostile))]);
   let err=event(&mut st,"workspace_select:default",&obj(vec![]),&root).unwrap_err();
   assert!(err.contains("Original workspace location changed")||err.contains("existing app workspace"),"{hostile}: {err}");
   assert_eq!(text(&st,"workspace_selected"),"kept","{hostile}");
   assert_ne!(text(&st,"approval_root"),outside_canon,"{hostile}");
  }
  for id in ["\\..\\..\\outside","/../../outside"] {
   let mut st=obj(vec![("workspace_selected",s("kept")),("workspace_original",s("real-workspace")),("workspaces",Arr(vec![obj(vec![("id",s(id)),("name",s("bad"))])]))]);
   let err=event(&mut st,&format!("workspace_select:{id}"),&obj(vec![]),&root).unwrap_err();
   assert!(err.contains("Original workspace location changed"),"{id}: {err}");
   assert_eq!(text(&st,"workspace_selected"),"kept","{id}");
   assert_ne!(text(&st,"approval_root"),outside_canon,"{id}");
  }
  let mut ok=obj(vec![("workspace_selected",s("kept")),("workspace_original",s("real-workspace"))]);
  event(&mut ok,"workspace_select:default",&obj(vec![]),&root).unwrap();
  assert_eq!(text(&ok,"workspace_selected"),"");
  let adopted=text(&ok,"approval_root");
  assert!(adopted.ends_with("real-workspace"),"{adopted}");
  assert_ne!(adopted,outside_canon);
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn root_for_accepts_a_two_hundred_character_cjk_name_on_windows() {
  let name="文".repeat(200);
  assert_eq!(name.chars().count(),200);
  assert_eq!(name.len(),600);
  assert!(plain_directory_name(&"a".repeat(255)));
  assert!(!plain_directory_name(&"a".repeat(256)));
  let scratch=std::env::temp_dir().join(format!("ws-cjk-{}",super::super::now()));
  std::fs::create_dir_all(&scratch).unwrap();
  let root=scratch.to_str().unwrap();
  #[cfg(windows)]
  {
   assert_eq!(name.encode_utf16().count(),200);
   assert!(plain_directory_name(&name));
   let at_limit="文".repeat(255);
   assert_eq!(at_limit.encode_utf16().count(),255);
   assert_eq!(at_limit.len(),765);
   assert!(plain_directory_name(&at_limit));
   assert!(!plain_directory_name(&"文".repeat(256)));
   let st=obj(vec![("workspace_selected",s("kept")),("workspace_original",s(&name))]);
   let target=root_for(&st,root,"").expect("200-character CJK workspace name");
   assert_eq!(target.file_name().and_then(|part|part.to_str()),Some(name.as_str()));
   let limit_st=obj(vec![("workspace_selected",s("kept")),("workspace_original",s(&at_limit))]);
   let limit_target=root_for(&limit_st,root,"").expect("255 UTF-16 workspace name");
   assert_eq!(limit_target.file_name().and_then(|part|part.to_str()),Some(at_limit.as_str()));
  }
  #[cfg(not(windows))]
  {
   assert!(!plain_directory_name(&name));
   let st=obj(vec![("workspace_selected",s("kept")),("workspace_original",s(&name))]);
   let err=root_for(&st,root,"").unwrap_err();
   assert!(err.contains("Original workspace location changed"),"{err}");
  }
 }
 fn rooted(path:&std::path::Path)->(String,String){
  let canonical=path.canonicalize().unwrap();
  let identity={use crate::host_fs::MetadataExt;let meta=std::fs::metadata(&canonical).unwrap();format!("{}:{}",meta.dev(),meta.ino())};
  (canonical.to_str().unwrap().to_string(),identity)
 }
 fn public_read(root:&str,identity:&str,extra:Vec<(&str,Json)>,action:&str,inputs:Json,defer:bool)->Json{
  let mut st=super::super::blank();
  put(&mut st,"busy",Bool(true));
  put(&mut st,"storage_access",obj(vec![("cloud",Bool(true))]));
  put(&mut st,"cloud_document",obj(vec![("uri",s("content://local.provider/doc")),("name",s("a.txt")),("mime",s("text/plain"))]));
  put(&mut st,"cloud_expected",obj(vec![("ticket",s("t1")),("phase",s("read")),("root",s(root)),("identity",s(identity))]));
  for (key,value) in extra {put(&mut st,key,value);}
  super::super::event(&obj(vec![("state",st),("action",s(action)),("inputs",inputs),("root",s(root)),("defer_policy_commit",Bool(defer))])).unwrap()
 }
 fn assert_cleared(result:&Json,notice:&str){
  let state=get(result,"state");
  assert_eq!(get(state,"busy"),&Bool(false));
  assert_eq!(get(state,"cloud_expected"),&Null);
  assert_eq!(get(get(state,"cloud_document"),"content"),&Null);
  assert_eq!(get(state,"notice_error"),&Bool(true));
  assert!(text(state,"notice").contains(notice),"{notice} in {}",text(state,"notice"));
  assert!(text(get(result,"ui"),"visible_status").contains(notice),"{}",text(get(result,"ui"),"visible_status"));
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn provider_failure_commits_cleared_read_on_the_public_event() {
  let scratch=std::env::temp_dir().join(format!("ws-public-{}",super::super::now()));
  std::fs::create_dir_all(&scratch).unwrap();
  let (root,identity)=rooted(&scratch);
  let failure=obj(vec![("ticket",s("t1")),("cloud_response",obj(vec![("error",s("Provider read failed"))]))]);
  for defer in [true,false] {
   let failed=public_read(&root,&identity,vec![],"cloud_failure",failure.clone(),defer);
   assert_cleared(&failed,"Provider read failed");
  }
  let text_inputs=obj(vec![("ticket",s("t1")),("content",s(&"y".repeat(1024*1024)))]);
  let padding=vec![("padding",s(&"x".repeat(5*1024*1024)))];
  for defer in [true,false] {
   let heavy=public_read(&root,&identity,padding.clone(),"cloud_text_result",text_inputs.clone(),defer);
   assert_cleared(&heavy,"saved state");
  }
 }
 fn with_padding(mut st:Json,target:usize)->Json{
  put(&mut st,"padding",s(""));
  let fill=target.saturating_sub(st.string().len());
  put(&mut st,"padding",s(&"x".repeat(fill)));
  st
 }
 fn slim_event(st:&Json,root:&str,content:&str)->String{
  obj(vec![
   ("op",s("event")),("defer_policy_commit",Bool(true)),("action",s("cloud_text_result")),
   ("state",st.clone()),
   ("inputs",obj(vec![
    ("ticket",s("t1")),
    ("cloud_response",obj(vec![("content",s(content))])),
    ("phone_access_granted",Bool(true)),("gateway_status",s("")),
    ("device",obj(vec![
     ("memory_bytes",Num("8589934592".into())),("heap_bytes",Num("536870912".into())),("cores",Num("8".into())),
     ("abi",s("aarch64")),("application_id",s("local.client")),
     ("microphone_granted",Bool(false)),("all_files_access",Bool(true)),
     ("device_root_status",s("unchecked")),("device_root_enabled",Bool(false)),
    ])),
   ])),
   ("root",s(root)),
  ]).string()
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn provider_text_stays_inside_the_event_input_limit() {
  let small=provider_ready();
  assert_eq!(provider_text_limit(&small,""),PROVIDER_TEXT_CAP);
  let scratch=std::env::temp_dir().join(format!("ws-event-limit-{}",super::super::now()));
  std::fs::create_dir_all(&scratch).unwrap();
  let (root,identity)=rooted(&scratch);
  let mut heavy=super::super::blank();
  put(&mut heavy,"busy",Bool(true));
  put(&mut heavy,"approval_root",s(&root));
  put(&mut heavy,"approval_root_identity",s(&identity));
  put(&mut heavy,"storage_access",obj(vec![("cloud",Bool(true))]));
  put(&mut heavy,"cloud_document",obj(vec![("uri",s("content://local.provider/doc")),("name",s("a.txt")),("mime",s("text/plain"))]));
  put(&mut heavy,"cloud_expected",obj(vec![("ticket",s("t1")),("phase",s("read")),("root",s(&root)),("identity",s(&identity))]));
  heavy=with_padding(heavy,PLATFORM_SAVE_LIMIT-256);
  assert!(heavy.string().len()+256>=PLATFORM_SAVE_LIMIT-64,"{}",heavy.string().len());
  let limit=provider_text_limit(&heavy,&root);
  assert!(limit>0&&limit<PROVIDER_TEXT_CAP,"limit {limit}");
  let hostile="\\".repeat(PROVIDER_TEXT_CAP);
  let encoded=slim_event(&heavy,&root,&hostile);
  assert!(encoded.len()>EVENT_INPUT_LIMIT,"escaped request {}",encoded.len());
  let rejected=super::super::operation(&encoded).unwrap_err();
  assert!(rejected.contains("8 MiB"),"{rejected}");
  let mut cleared=heavy.clone();
  let direct=event(&mut cleared,"cloud_text_result",&obj(vec![("ticket",s("t1")),("content",s(&hostile))]),&root).unwrap_err();
  assert!(direct.contains("event input limit"),"{direct}");
  assert_eq!(get(&cleared,"busy"),&Bool(false));
  assert_eq!(get(&cleared,"cloud_expected"),&Null);
  assert_eq!(get(get(&cleared,"cloud_document"),"content"),&Null);
  let bounded=std::iter::repeat('\u{0001}').take(limit).collect::<String>();
  let admitted=slim_event(&heavy,&root,&bounded);
  assert!(admitted.len()<=EVENT_INPUT_LIMIT,"bounded request {}",admitted.len());
  super::super::parse(&admitted).expect("bounded provider text stays inside the event input limit");
  let opened=super::super::operation(&admitted).expect("bounded read reaches the handler");
  let state=get(&opened,"state");
  assert_eq!(get(state,"busy"),&Bool(false));
  assert_eq!(get(state,"cloud_expected"),&Null);
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn provider_failure_text_stays_inside_the_save_limit() {
  let scratch=std::env::temp_dir().join(format!("ws-failure-bound-{}",super::super::now()));
  std::fs::create_dir_all(&scratch).unwrap();
  let (root,identity)=rooted(&scratch);
  let reason="\\".repeat(PROVIDER_TEXT_CAP);
  let inputs=obj(vec![("ticket",s("t1")),("cloud_response",obj(vec![("error",s(&reason))]))]);
  let failed=public_read(&root,&identity,vec![("padding",s(&"x".repeat(5*1024*1024+512*1024)))],"cloud_failure",inputs,true);
  let state=get(&failed,"state");
  assert_eq!(get(state,"busy"),&Bool(false));
  assert_eq!(get(state,"cloud_expected"),&Null);
  assert_eq!(get(get(state,"cloud_document"),"content"),&Null);
  assert_eq!(get(state,"notice_error"),&Bool(true));
  assert!(text(state,"notice").len()<=PROVIDER_REASON_LIMIT,"{}",text(state,"notice").len());
  assert!(state.string().len()<=PLATFORM_SAVE_LIMIT,"returned {}",state.string().len());
  assert!(persisted_bytes(state)<=PLATFORM_SAVE_LIMIT,"persisted {}",persisted_bytes(state));
  assert!(!text(state,"notice").chars().any(char::is_control));
 }
 fn pad_to(mut st:Json,target:usize)->Json{
  put(&mut st,"padding",s(""));
  let fill=target.saturating_sub(persisted_bytes(&st));
  put(&mut st,"padding",s(&"x".repeat(fill)));
  st
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn provider_failure_near_the_save_ceiling_stays_saved() {
  let scratch=std::env::temp_dir().join(format!("ws-ceiling-{}",super::super::now()));
  std::fs::create_dir_all(&scratch).unwrap();
  let (root,identity)=rooted(&scratch);
  let opened=super::super::event(&obj(vec![("state",super::super::blank()),("action",s("workspace_snapshots")),("inputs",obj(vec![])),("root",s(&root)),("defer_policy_commit",Bool(true))])).unwrap();
  let mut heavy=get(&opened,"state").clone();
  put(&mut heavy,"busy",Bool(true));
  put(&mut heavy,"notice",s("x"));
  put(&mut heavy,"notice_error",Bool(false));
  put(&mut heavy,"storage_access",obj(vec![("cloud",Bool(true))]));
  put(&mut heavy,"cloud_document",obj(vec![("uri",s("content://local.provider/doc")),("name",s("a.txt")),("mime",s("text/plain"))]));
  put(&mut heavy,"cloud_expected",obj(vec![("ticket",s("t1")),("phase",s("read")),("root",s(&root)),("identity",s(&identity)),("limit",Num("32".into()))]));
  let reason="\\".repeat(PROVIDER_REASON_LIMIT);
  let base=heavy;
  let mut heavy=Null;
  for slack in [400usize,320,240,160,120,80,48]{
   let candidate=pad_to(base.clone(),PLATFORM_SAVE_LIMIT.saturating_sub(slack));
   let mut naive=candidate.clone();
   put(&mut naive,"busy",Bool(false));
   put(&mut naive,"cloud_expected",Null);
   put(&mut naive,"notice",s(&reason));
   put(&mut naive,"notice_error",Bool(true));
   let mut emptied=candidate.clone();
   put(&mut emptied,"busy",Bool(false));
   put(&mut emptied,"cloud_expected",Null);
   put(&mut emptied,"notice",s(""));
   put(&mut emptied,"notice_error",Bool(true));
   if persisted_bytes(&naive)>PLATFORM_SAVE_LIMIT && persisted_bytes(&emptied).saturating_add(64)<=PLATFORM_SAVE_LIMIT{heavy=candidate;break;}
  }
  assert!(heavy!=Null,"near-limit failure must be able to cross the save ceiling");
  let inputs=obj(vec![("ticket",s("t1")),("cloud_response",obj(vec![("error",s(&reason))]))]);
  let failed=super::super::event(&obj(vec![("state",heavy),("action",s("cloud_failure")),("inputs",inputs),("root",s(&root)),("defer_policy_commit",Bool(true))])).unwrap();
  let state=get(&failed,"state");
  assert_eq!(get(state,"busy"),&Bool(false));
  assert_eq!(get(state,"cloud_expected"),&Null);
  assert_eq!(get(get(state,"cloud_document"),"content"),&Null);
  assert_eq!(get(state,"notice_error"),&Bool(true));
  assert!(persisted_bytes(state)<=PLATFORM_SAVE_LIMIT,"persisted {}",persisted_bytes(state));
  assert!(state.string().len()<=PLATFORM_SAVE_LIMIT,"encoded {}",state.string().len());
  let notice=text(state,"notice");
  assert!(!notice.contains("opened")&&!notice.contains("selected"),"{notice}");
  assert!(notice.contains("shortened")||notice.contains("failed")||notice.len()<reason.len(),"{notice}");
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn provider_read_rejects_text_over_the_published_limit() {
  let mut st=pad_to(provider_ready(),3*1024*1024);
  let limit=provider_text_limit(&st,"");
  assert!(limit>0&&limit<PROVIDER_TEXT_CAP,"limit {limit}");
  assert!(provider_event_base(&st,"").saturating_add(limit.saturating_mul(JSON_ESCAPE_MAX))<=EVENT_INPUT_LIMIT);
  let mut owner=get(&st,"cloud_expected").clone();
  put(&mut owner,"limit",Num(limit.to_string()));
  put(&mut st,"cloud_expected",owner);
  let mut over=st.clone();
  let err=event(&mut over,"cloud_text_result",&obj(vec![("ticket",s("t1")),("content",s(&"a".repeat(limit+1)))]),"").unwrap_err();
  assert!(err.contains("published read limit"),"{err}");
  assert_eq!(get(&over,"busy"),&Bool(false));
  assert_eq!(get(&over,"cloud_expected"),&Null);
  assert_eq!(get(get(&over,"cloud_document"),"content"),&Null);
  assert_eq!(get(&over,"notice_error"),&Bool(true));
  assert!(persisted_bytes(&over)<=PLATFORM_SAVE_LIMIT);
  let mut exact=st.clone();
  event(&mut exact,"cloud_text_result",&obj(vec![("ticket",s("t1")),("content",s(&"a".repeat(limit)))]),"").unwrap();
  assert_eq!(text(get(&exact,"cloud_document"),"content").len(),limit);
  assert_eq!(get(&exact,"busy"),&Bool(false));
  assert_eq!(get(&exact,"cloud_expected"),&Null);
  assert_eq!(text(&exact,"notice"),"Selected provider text opened");
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn snapshot_actions_round_trip_project_bytes() {
  let scratch=std::env::temp_dir().join(format!("ws-snapshot-{}",super::super::now()));
  let source=scratch.join("source");
  let restored=scratch.join("restored");
  std::fs::create_dir_all(&source).unwrap();
  std::fs::create_dir_all(&restored).unwrap();
  std::fs::write(source.join("notes.txt"),"snapshot-body").unwrap();
  let root=source.canonicalize().unwrap().to_string_lossy().into_owned();
  let modal=modal_for(&obj(vec![("dialog",s("workspace_snapshots"))])).unwrap().string();
  for action in ["project_export","project_import","project_save","project_save_drive","artifact"] {
   assert!(modal.contains(action),"{action} missing from snapshot actions");
  }
  let started=super::super::event(&obj(vec![("state",super::super::blank()),("action",s("project_export")),("inputs",obj(vec![])),("root",s(&root)),("defer_policy_commit",Bool(true))])).unwrap();
  assert_eq!(text(get(&started,"effect"),"kind"),"archive");
  assert_eq!(get(get(&started,"state"),"busy"),&Bool(true));
  assert!(!text(get(&started,"state"),"notice").contains("ready"),"export must not report completion before the archive exists");
  let packed=super::super::operation(&obj(vec![("op",s("project_archive")),("root",s(&root))]).string()).unwrap();
  let archive=text(&packed,"archive");
  assert!(!archive.is_empty());
  let preview=super::super::operation(&obj(vec![("op",s("archive_preview")),("archive",s(&archive))]).string()).unwrap();
  assert!(preview.string().contains("notes.txt"),"{}",preview.string());
  let imported=super::super::operation(&obj(vec![("op",s("archive_import")),("archive",s(&archive)),("root",s(restored.to_str().unwrap()))]).string()).expect("import");
  assert_ne!(text(&imported,"files"),"0","{}",imported.string());
  let body=std::fs::read_to_string(restored.join("notes.txt")).expect("restored notes");
  assert_eq!(body,"snapshot-body");
  let pick=super::super::event(&obj(vec![("state",super::super::blank()),("action",s("project_import")),("inputs",obj(vec![])),("root",s(&root)),("defer_policy_commit",Bool(true))])).unwrap();
  assert_eq!(text(get(&pick,"effect"),"kind"),"document_zip");
  assert_eq!(get(get(&pick,"state"),"busy"),&Bool(true));
  assert!(!text(get(&pick,"state"),"notice").contains("Created"));
  let unsaved=super::super::event(&obj(vec![("state",super::super::blank()),("action",s("project_save")),("inputs",obj(vec![])),("root",s(&root)),("defer_policy_commit",Bool(true))])).unwrap();
  assert_eq!(get(&unsaved,"effect"),&Null);
  let save_notice=text(get(&unsaved,"state"),"notice");
  assert!(!save_notice.contains("ready")&&!save_notice.is_empty(),"{save_notice}");
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn provider_selection_requires_text_metadata() {
  fn selecting()->Json{
   obj(vec![
    ("approval_root",s("")),("approval_root_identity",s("")),
    ("storage_access",obj(vec![("cloud",Bool(true))])),
    ("cloud_expected",obj(vec![("ticket",s("t1")),("phase",s("select")),("root",s("")),("identity",s(""))])),
    ("cloud_document",obj(vec![("uri",s("content://local.provider/kept")),("name",s("kept.txt")),("mime",s("text/plain"))])),
   ])
  }
  let uri=s("content://local.provider/doc");
  for response in [
   obj(vec![("uri",uri.clone()),("mime",s("text/plain"))]),
   obj(vec![("uri",uri.clone()),("name",Bool(true)),("mime",s("text/plain"))]),
   obj(vec![("uri",uri.clone()),("name",s("notes.txt")),("mime",Num("1".into()))]),
   obj(vec![("uri",uri.clone()),("name",Null),("mime",s("text/plain"))]),
  ] {
   let mut st=selecting();
   let err=event(&mut st,"cloud_result",&obj(vec![("ticket",s("t1")),("cloud_response",response)]),"").unwrap_err();
   assert!(err.contains("Invalid selected document metadata"),"{err}");
   assert_eq!(text(get(&st,"cloud_document"),"uri"),"content://local.provider/kept");
   assert_ne!(text(&st,"notice"),"Provider document selected");
   assert_eq!(get(&st,"cloud_expected"),&Null);
  }
  let mut ok=selecting();
  event(&mut ok,"cloud_result",&obj(vec![("ticket",s("t1")),("cloud_response",obj(vec![("uri",uri),("name",s("notes.txt")),("mime",s("text/plain"))]))]),"").unwrap();
  assert_eq!(text(&ok,"notice"),"Provider document selected");
  assert_eq!(text(get(&ok,"cloud_document"),"name"),"notes.txt");
  assert_eq!(text(get(&ok,"cloud_document"),"mime"),"text/plain");
  assert_eq!(get(&ok,"notice_error"),&Bool(false));
 }
 fn storage_alias(prefix:&str)->String{format!("{prefix}_drive")}
 #[cfg_attr(not(rllm_module_tests), test)]
 fn saved_retired_storage_route_opens_a_workspace_panel() {
  let retired=storage_alias("stored");
  assert_eq!(retired.len(),12);
  assert!(retired_provider_route(&retired));
  assert!(handles(&retired));
  assert!(!retired_provider_route("store_drive"));
  assert!(!retired_provider_route("document_providers"));
  assert!(!handles("retired_storage_route"));
  let mut saved=obj(vec![("dialog",s(&retired)),("workspace_stack",Arr((0..20).map(|_|s(&retired)).collect()))]);
  remember(&mut saved,"workspace_files");
  let expected=if provider_controls(){"document_providers"}else{"workspace_storage"};
  assert_eq!(text(&saved,"dialog"),expected);
  let stack=array(&saved,"workspace_stack");
  assert!(stack.len()<=12,"{}",stack.len());
  assert!(stack.iter().all(|item|matches!(item,Str(name) if name==expected)));
  let panel=modal_for(&obj(vec![("dialog",s(&retired))])).unwrap();
  assert!(text(&panel,"title")=="Storage"||text(&panel,"title")=="Document providers","{}",panel.string());
  let mut opened=obj(vec![]);
  event(&mut opened,&retired,&obj(vec![]),"").unwrap();
  assert_eq!(text(&opened,"dialog"),expected);
  assert!(modal_for(&opened).is_some());
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn provider_grants_stop_at_the_allowance() {
  let mut grants=Vec::new();
  for n in 0..4000{grants.push(s(&format!("content://local.provider/item{n}")));}
  let mut st=obj(vec![("cloud_grants",Arr(grants.clone())),("cloud_document",obj(vec![("uri",s("content://local.provider/item0"))]))]);
  let kept=document_grants(&st);
  assert_eq!(kept.len(),GRANT_CAP);
  assert!(matches!(&kept[0], Str(value) if value=="content://local.provider/item0"));
  assert!(matches!(kept.last(), Some(Str(value)) if value=="content://local.provider/item63"));
  assert!(!kept.iter().any(|uri|matches!(uri,Str(value) if value.ends_with("item64"))));
  let mut duplicates=Vec::new();
  for _ in 0..4000{duplicates.push(s("content://local.provider/same"));}
  st=obj(vec![("cloud_grants",Arr(duplicates))]);
  assert_eq!(document_grants(&st).len(),1);
  let mut released=Vec::new();
  for n in 0..4000{released.push(s(&format!("content://local.provider/item{n}")));}
  let mut owner=obj(vec![
   ("approval_root",s("")),("approval_root_identity",s("")),
   ("storage_access",obj(vec![("cloud",Bool(true))])),
   ("cloud_expected",obj(vec![("ticket",s("t1")),("phase",s("release")),("root",s("")),("identity",s(""))])),
   ("cloud_grants",Arr(grants[..80].to_vec())),
  ]);
  event(&mut owner,"cloud_release_result",&obj(vec![("ticket",s("t1")),("cloud_response",obj(vec![("released",Arr(released))]))]),"").unwrap();
  assert_eq!(get(&owner,"cloud_expected"),&Null);
  assert_eq!(array(&owner,"cloud_grants").len(),0);
  let mut full=selecting_grants(80);
  let err=event(&mut full,"cloud_result",&obj(vec![("ticket",s("t1")),("cloud_response",obj(vec![("uri",s("content://local.provider/fresh")),("name",s("n.txt")),("mime",s("text/plain"))]))]),"").unwrap_err();
  assert!(err.contains("allowance is full"),"{err}");
  assert_eq!(text(get(&full,"cloud_document"),"uri"),"content://local.provider/kept");
 }
 fn selecting_grants(count:usize)->Json{
  let mut grants=Vec::new();
  for n in 0..count{grants.push(s(&format!("content://local.provider/item{n}")));}
  obj(vec![
   ("approval_root",s("")),("approval_root_identity",s("")),
   ("storage_access",obj(vec![("cloud",Bool(true))])),
   ("cloud_expected",obj(vec![("ticket",s("t1")),("phase",s("select")),("root",s("")),("identity",s(""))])),
   ("cloud_document",obj(vec![("uri",s("content://local.provider/kept")),("name",s("kept.txt")),("mime",s("text/plain"))])),
   ("cloud_grants",Arr(grants)),
  ])
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn path_import_round_trips_without_a_host_chooser() {
  clear_pending_archive();
  let scratch=std::env::temp_dir().join(format!("ws-import-{}",super::super::now()));
  let source=scratch.join("source");
  let restored=scratch.join("restored");
  std::fs::create_dir_all(&source).unwrap();
  std::fs::create_dir_all(&restored).unwrap();
  std::fs::write(source.join("notes.txt"),"snapshot-body").unwrap();
  let root=source.canonicalize().unwrap().to_string_lossy().into_owned();
  let packed=super::super::operation(&obj(vec![("op",s("project_archive")),("root",s(&root))]).string()).unwrap();
  let encoded=text(&packed,"archive");
  let bytes=crate::network::unbase64(&encoded).unwrap();
  let archive_path=scratch.join("project.archive");
  std::fs::write(&archive_path,&bytes).unwrap();
  let refused=super::super::blank();
  let bad=super::super::event(&obj(vec![("state",refused.clone()),("action",s("project_import")),("inputs",obj(vec![("archive_path",s("bad\npath"))])),("root",s(&root)),("defer_policy_commit",Bool(true))])).unwrap();
  assert_eq!(get(&bad,"effect"),&Null);
  assert!(text(get(&bad,"state"),"notice").contains("path"),"{}",text(get(&bad,"state"),"notice"));
  assert_eq!(get(get(&bad,"state"),"notice_error"),&Bool(true));
  let long_path="a".repeat(4097);
  let long=super::super::event(&obj(vec![("state",refused.clone()),("action",s("project_import")),("inputs",obj(vec![("archive_path",s(&long_path))])),("root",s(&root)),("defer_policy_commit",Bool(true))])).unwrap();
  assert!(text(get(&long,"state"),"notice").contains("path"),"{}",text(get(&long,"state"),"notice"));
  assert_eq!(get(&long,"effect"),&Null);
  let directory=super::super::event(&obj(vec![("state",refused.clone()),("action",s("project_import")),("inputs",obj(vec![("archive_path",s(source.to_str().unwrap()))])),("root",s(&root)),("defer_policy_commit",Bool(true))])).unwrap();
  assert!(text(get(&directory,"state"),"notice").contains("file"),"{}",text(get(&directory,"state"),"notice"));
  let empty=super::super::event(&obj(vec![("state",refused),("action",s("project_import")),("inputs",obj(vec![])),("root",s(&root)),("defer_policy_commit",Bool(true))])).unwrap();
  if host_archive_chooser(){
   assert_eq!(text(get(&empty,"effect"),"kind"),"document_zip");
   assert_eq!(get(get(&empty,"state"),"busy"),&Bool(true));
   assert!(!text(get(&empty,"state"),"notice").contains("Created"));
  }else{
   assert_eq!(get(&empty,"effect"),&Null);
   assert!(text(get(&empty,"state"),"notice").contains("archive path"),"{}",text(get(&empty,"state"),"notice"));
   assert_ne!(get(get(&empty,"state"),"busy"),&Bool(true));
  }
  let staged=super::super::event(&obj(vec![("state",super::super::blank()),("action",s("project_import")),("inputs",obj(vec![("archive_path",s(archive_path.to_str().unwrap()))])),("root",s(&root)),("defer_policy_commit",Bool(true))])).unwrap();
  assert_eq!(get(&staged,"effect"),&Null);
  assert_eq!(text(get(&staged,"state"),"dialog"),"archive_review");
  assert_eq!(get(get(&staged,"state"),"busy"),&Bool(false));
  assert!(get(&staged,"state").string().contains("notes.txt"));
  assert!(!text(get(&staged,"state"),"notice").contains("Created"));
  let accepted=super::super::event(&obj(vec![("state",get(&staged,"state").clone()),("action",s("archive_accept")),("inputs",obj(vec![])),("root",s(restored.to_str().unwrap())),("defer_policy_commit",Bool(true))])).unwrap();
  assert_eq!(get(&accepted,"effect"),&Null);
  assert_eq!(get(get(&accepted,"state"),"busy"),&Bool(false));
  assert!(text(get(&accepted,"state"),"notice").contains("Created"),"{}",text(get(&accepted,"state"),"notice"));
  assert_eq!(std::fs::read_to_string(restored.join("notes.txt")).unwrap(),"snapshot-body");
  let mut review=get(&staged,"state").clone();
  let rejected=event(&mut review,"archive_reject",&obj(vec![]),&root).unwrap();
  assert_eq!(text(&rejected,"kind"),"archive_clear");
  assert_eq!(text(&review,"notice"),"Project import cancelled");
  clear_pending_archive();
 }
 struct ResetBudget;
 impl Drop for ResetBudget { fn drop(&mut self) { crate::archive::configure_project_budget(0); } }
 fn fill_bytes(len:usize)->Vec<u8>{
  let mut out=vec![0u8;len];
  let mut x=0xA5A5_1234u32;
  for (i,b) in out.iter_mut().enumerate(){
   x=x.wrapping_mul(1664525).wrapping_add(1013904223);
   *b=(x>>24) as u8;
   if i==0{*b=0x11;}
   if i+1==len{*b=0x22;}
  }
  out
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn archive_larger_than_five_mib_round_trips_under_the_memory_budget() {
  clear_pending_archive();
  assert!(archive_byte_limit()>5*1024*1024,"limit {}",archive_byte_limit());
  let scratch=std::env::temp_dir().join(format!("ws-wide-{}",super::super::now()));
  let source=scratch.join("source");
  let restored=scratch.join("restored");
  std::fs::create_dir_all(&source).unwrap();
  std::fs::create_dir_all(&restored).unwrap();
  let payload=fill_bytes(5*1024*1024+64);
  std::fs::write(source.join("wide.bin"),&payload).unwrap();
  let (bytes,_files,_folders)=crate::archive::archive(&source).expect("project save");
  assert!(bytes.len()>5*1024*1024,"saved archive {}",bytes.len());
  assert!(bytes.len() as u64<=archive_byte_limit(),"saved {} limit {}",bytes.len(),archive_byte_limit());
  let archive_path=scratch.join("project.archive");
  std::fs::write(&archive_path,&bytes).unwrap();
  drop(bytes);
  let root=source.canonicalize().unwrap().to_string_lossy().into_owned();
  let staged=super::super::event(&obj(vec![("state",super::super::blank()),("action",s("project_import")),("inputs",obj(vec![("archive_path",s(archive_path.to_str().unwrap()))])),("root",s(&root)),("defer_policy_commit",Bool(true))])).unwrap();
  assert_eq!(text(get(&staged,"state"),"dialog"),"archive_review");
  assert!(!text(get(&staged,"state"),"notice").contains("Created"));
  assert!(get(&staged,"state").string().contains("wide.bin"));
  let accepted=super::super::event(&obj(vec![("state",get(&staged,"state").clone()),("action",s("archive_accept")),("inputs",obj(vec![])),("root",s(restored.to_str().unwrap())),("defer_policy_commit",Bool(true))])).unwrap();
  assert_eq!(get(&accepted,"effect"),&Null);
  assert!(text(&accepted,"effect").is_empty()||get(&accepted,"effect")==&Null);
  assert_eq!(get(get(&accepted,"state"),"busy"),&Bool(false));
  let notice=text(get(&accepted,"state"),"notice");
  assert!(notice.contains("Created")&&!notice.contains("partial"),"{notice}");
  assert_eq!(std::fs::read(restored.join("wide.bin")).unwrap(),payload);
  clear_pending_archive();
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn archive_over_the_memory_budget_is_refused() {
  let _reset=ResetBudget;
  crate::archive::configure_project_budget(2*1024*1024);
  let limit=archive_byte_limit();
  assert!(limit>0&&limit<2*1024*1024,"limit {limit}");
  let scratch=std::env::temp_dir().join(format!("ws-budget-{}",super::super::now()));
  std::fs::create_dir_all(&scratch).unwrap();
  let root=scratch.canonicalize().unwrap().to_string_lossy().into_owned();
  let too_big=scratch.join("too-big.archive");
  std::fs::write(&too_big,vec![0u8;usize::try_from(limit).unwrap()+1]).unwrap();
  let huge=super::super::event(&obj(vec![("state",super::super::blank()),("action",s("project_import")),("inputs",obj(vec![("archive_path",s(too_big.to_str().unwrap()))])),("root",s(&root)),("defer_policy_commit",Bool(true))])).unwrap();
  let notice=text(get(&huge,"state"),"notice");
  assert!(notice.contains("memory budget"),"{notice}");
  assert!(!notice.contains("5 MiB")&&!notice.contains("Created"),"{notice}");
  assert_eq!(get(&huge,"effect"),&Null);
  assert_eq!(get(get(&huge,"state"),"notice_error"),&Bool(true));
  let edge=scratch.join("edge.archive");
  std::fs::write(&edge,vec![0u8;usize::try_from(limit).unwrap()]).unwrap();
  let admitted=super::super::event(&obj(vec![("state",super::super::blank()),("action",s("project_import")),("inputs",obj(vec![("archive_path",s(edge.to_str().unwrap()))])),("root",s(&root)),("defer_policy_commit",Bool(true))])).unwrap();
  let edge_notice=text(get(&admitted,"state"),"notice");
  assert!(!edge_notice.contains("5 MiB")&&!edge_notice.contains("memory budget")&&!edge_notice.contains("Created"),"{edge_notice}");
  assert_eq!(get(&admitted,"effect"),&Null);
  assert_ne!(text(get(&admitted,"state"),"dialog"),"archive_review");
  clear_pending_archive();
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn chooser_accept_keeps_the_archive_on_the_host() {
  clear_pending_archive();
  let mut st=obj(vec![("dialog",s("archive_review")),("archive_names",Arr(vec![s("notes.txt")])),("notice",s("Review the project filenames before creating files"))]);
  let effect=event(&mut st,"archive_accept",&obj(vec![]),"root").unwrap();
  assert_eq!(text(&effect,"kind"),"archive_import");
  assert!(text(&effect,"archive").is_empty());
  assert_eq!(get(&st,"busy"),&Bool(true));
  assert_eq!(get(&st,"dialog"),&Null);
  assert!(!text(&st,"notice").contains("Created"));
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn oversized_callback_does_not_parse_and_a_bounded_failure_clears_the_read() {
  let over="x".repeat(EVENT_INPUT_LIMIT+1);
  let err=super::super::parse(&over).unwrap_err();
  assert!(err.contains("8 MiB"),"{err}");
  let mut st=provider_ready();
  let inputs=obj(vec![("ticket",s("t1")),("cloud_response",obj(vec![("error",s(&"\\".repeat(PROVIDER_REASON_LIMIT+40)))]))]);
  let err=event(&mut st,"cloud_failure",&inputs,"").unwrap_err();
  assert!(err.len()<=PROVIDER_REASON_LIMIT,"{}",err.len());
  assert!(!err.contains("opened"));
  assert_eq!(get(&st,"busy"),&Bool(false));
  assert_eq!(get(&st,"cloud_expected"),&Null);
  assert_eq!(get(get(&st,"cloud_document"),"content"),&Null);
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn parsed_oversized_failure_clears_the_read() {
  let scratch=std::env::temp_dir().join(format!("ws-large-fail-{}",super::super::now()));
  std::fs::create_dir_all(&scratch).unwrap();
  let (root,identity)=rooted(&scratch);
  let reason=format!("\u{0001}{}", "\\".repeat(1024*1024));
  let inputs=obj(vec![("ticket",s("t1")),("cloud_response",obj(vec![("error",s(&reason))]))]);
  let failed=public_read(&root,&identity,vec![],"cloud_failure",inputs,true);
  let state=get(&failed,"state");
  assert_eq!(get(state,"busy"),&Bool(false));
  assert_eq!(get(state,"cloud_expected"),&Null);
  assert_eq!(get(get(state,"cloud_document"),"content"),&Null);
  assert_eq!(get(state,"notice_error"),&Bool(true));
  let notice=text(state,"notice");
  assert!(notice.len()<=PROVIDER_REASON_LIMIT,"{}",notice.len());
  assert!(!notice.chars().any(char::is_control));
  assert!(!notice.contains("opened")&&!notice.contains("selected"),"{notice}");
  assert!(text(get(&failed,"ui"),"visible_status").contains(&notice),"{notice}");
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn workspace_list_over_the_allowance_is_partial() {
  let mut listed=Vec::new();
  for n in 0..40{listed.push(obj(vec![("id",s(&format!("id{n}"))),("name",s(&format!("Place {n}")))]));}
  let panel=modal_for(&obj(vec![("dialog",s("workspace_locations")),("workspaces",Arr(listed))])).unwrap().string();
  assert!(panel.contains("partial: workspace list stopped at 32 locations"),"{panel}");
  assert!(panel.contains("workspace_select:id31"));
  assert!(!panel.contains("workspace_select:id32"),"{panel}");
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn published_read_effect_carries_the_callback_limits() {
  let doc=obj(vec![("uri",s("content://local.provider/doc")),("mime",s("text/plain"))]);
  let effect=cloud_read_effect(&doc,"t1",32);
  assert_eq!(text(&effect,"kind"),"cloud_read");
  assert_eq!(number(&effect,"limit"),32);
  assert_eq!(number(&effect,"reason_limit"),PROVIDER_REASON_LIMIT);
  let mut heavy=pad_to(provider_ready(),3*1024*1024);
  let limit=provider_text_limit(&heavy,"");
  let mut owner=get(&heavy,"cloud_expected").clone();
  put(&mut owner,"limit",Num(limit.to_string()));
  put(&mut owner,"reason_limit",Num(PROVIDER_REASON_LIMIT.to_string()));
  put(&mut heavy,"cloud_expected",owner);
  let mut over=heavy.clone();
  let err=event(&mut over,"cloud_text_result",&obj(vec![("ticket",s("t1")),("content",s(&"a".repeat(limit+1)))]),"").unwrap_err();
  assert!(err.contains("published read limit"),"{err}");
  assert_eq!(get(&over,"busy"),&Bool(false));
  assert_eq!(get(&over,"cloud_expected"),&Null);
 }
}
