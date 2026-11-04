//! Workspace navigation, source access and app-owned project directories.
use super::*;
const SECTIONS:&[&str]=&["cloud_selected","workspace_projects","workspace_storage","workspace_setup","workspace_permissions","workspace_defaults","workspace_new","workspace_locations","phone_locations","network_locations","google_drive","workspace_programs","workspace_research","workspace_snapshots","workspace_files"];
pub(super) fn rows()->Vec<Json>{vec![button("Projects","workspace_projects"),button("Storage","workspace_storage"),button("Tools and servers","mcp_client_menu"),button("Investigation","workspace_research"),button("Programs","workspace_programs"),button("Setup","workspace_setup")]}
pub(super) fn set_access(st:&mut Json,provider:&str,value:bool){let mut access=get(st,"storage_access").clone();if access==Null{access=obj(vec![]);}put(&mut access,provider,Bool(value));put(st,"storage_access",access);}
pub(super) fn allowed(st:&Json,provider:&str)->bool{get(get(st,"storage_access"),provider)!=&Bool(false)}
pub(super) fn check_tool(st:&Json,tool:&str)->Result<()>{
 let provider=if tool.starts_with("phone_"){"phone"}else if tool.starts_with("drive_"){"network"}else if tool=="mcp"{"mcp"}else{return Ok(())};
 if !allowed(st,provider){return Err(format!("{} access is off in Workspace → Setup → Access permissions",match provider{"phone"=>"Phone storage","network"=>"Network storage",_=>"MCP server"}));}Ok(())
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
fn title(st:&Json)->String{let id=text(st,"workspace_selected");array(st,"workspaces").into_iter().find(|v|text(v,"id")==id).map(|v|text(&v,"name")).unwrap_or("Workspace".into())}
fn popup(title:&str,fields:Vec<Json>,buttons:Vec<Json>)->Json{let mut value=modal(title,fields,buttons);put(&mut value,"close_label",s("Back"));value}
fn setting(st:&Json,key:&str)->String{if let Bool(value)=get(get(st,"edit"),key){if *value{"true".into()}else{"false".into()}}else{allowed(st,key).to_string()}}
pub(super) fn modal_for(st:&Json)->Option<Json>{
 let name=text(st,"dialog");Some(match name.as_str(){
 "workspace_projects"=>popup("Projects",vec![field_ui("current","Current workspace",&title(st),"readonly")],vec![button("New workspace","workspace_new"),button("Switch workspace","workspace_locations"),button("Project files","workspace_files"),button("Project snapshots","workspace_snapshots")]),
 "workspace_files"=>popup("Project files",vec![],vec![button("Browse files","files"),button("New or edited text file","editor"),button("Read text","reader"),button("Search files","search")]),
 "workspace_storage"=>popup("Storage",vec![],vec![button("Phone locations","phone_locations"),button("Network locations","network_locations"),button("Google Drive","google_drive")]),
 "phone_locations"=>popup("Phone locations",vec![field_ui("location","Selected folder",&text(st,"phone_uri"),"readonly")],vec![button("Browse phone files","phone_files"),button("Read phone text","phone_reader"),button("New phone file","phone_editor"),button("Phone text revision","phone_update_editor"),button("Last revision","phone_revision_status"),button("Attach selected file to chat","phone_file")]),
 "network_locations"=>popup("Network locations",vec![field_ui("location","Network folder",&endpoint_summary(&text(get(st,"drive"),"url")),"readonly")],vec![button("Browse network files","drive_list"),button("Read network text","drive_reader"),button("New network file","drive_editor"),button("Network text revision","drive_update_editor"),button("New network folder","drive_folder"),button("Copy text to workspace","drive_copy")]),
 "google_drive"=>popup("Google Drive",vec![field_ui("account","File access","Choose Google Drive in Android's file locations, then select a document.","readonly")],vec![button("Browse Google Drive files","cloud_browse")]),
 "workspace_setup"=>popup("Setup",vec![],vec![button("Phone folders and scope","phone_access"),button("Network drive connection","drive_edit"),button("Google Drive account","cloud_setup"),button("MCP server connection","mcp_client_settings"),button("Access permissions","workspace_permissions"),button("Default saves","workspace_defaults")]),
 "workspace_permissions"=>popup("Access permissions",vec![field_ui("phone","Phone storage access",&setting(st,"phone"),"check"),field_ui("network","Network storage access",&setting(st,"network"),"check"),field_ui("cloud","Google Drive access",&setting(st,"cloud"),"check"),field_ui("mcp","MCP server access",&setting(st,"mcp"),"check")],vec![button("Apply permissions","workspace_access_save")]),
 "workspace_defaults"=>popup("Default saves",vec![field_ui("phone_default","Use selected phone folder",if boolean(get(st,"drive"),"phone_default"){"true"}else{"false"},"check"),field_ui("autosave","Save completed replies",if boolean(get(st,"drive"),"autosave"){"true"}else{"false"},"check")],vec![button("Apply default saves","workspace_defaults_save")]),
 "workspace_new"=>popup("New workspace",vec![field_ui("name","Workspace name",&text(get(st,"edit"),"name"),"text")],vec![button("Create workspace","workspace_create")]),
 "workspace_locations"=>{let mut buttons=vec![button("Original workspace","workspace_select:default")];for v in array(st,"workspaces"){buttons.push(button(&text(&v,"name"),&format!("workspace_select:{}",text(&v,"id"))));}popup("Workspaces",vec![],buttons)},
 "workspace_snapshots"=>popup("Project snapshots",vec![],vec![button("Export project ZIP","project_export"),button("Import project ZIP","project_import"),button("Save project to default storage","project_save"),button("Save project to network storage","project_save_drive"),button("Save a project file","artifact")]),
 "workspace_programs"=>popup("Programs",vec![],vec![button("Shell program check","script_check"),button("Shell program execution","script_run")]),
 "workspace_research"=>popup("Investigation",vec![],vec![button("Question and design process","inquiry"),button("File investigation","investigation"),button("Web search","web_search"),button("Observations","investigation_records"),button("Hypotheses","investigation_claims"),button("Experiments","investigation_experiments"),button("Reconstruction","reconstruction_new")]),
 "cloud_setup"=>popup("Google Drive account",vec![field_ui("access","Allow Google Drive access",if allowed(st,"cloud"){"true"}else{"false"},"check")],vec![button("Apply access","workspace_source_access:cloud"),button("Sign in / add Google account","cloud_sign_in"),button("Access permissions","workspace_permissions"),button("Remove Google Drive file grants","cloud_disconnect")]),
 "cloud_selected"=>{if !allowed(st,"cloud"){return Some(popup("Google Drive",vec![field_ui("access","Source access","Off","readonly")],vec![button("Access permissions","workspace_permissions")]));}let doc=get(st,"cloud_document");let mut buttons=vec![button("Open selected file","cloud_open"),button("Read selected text","cloud_read")];if get(doc,"content")!=&Null{buttons.push(button("Copy text to workspace","cloud_copy"));}popup("Google Drive file",vec![field_ui("name","File",&text(doc,"name"),"readonly"),field_ui("mime","Type",&text(doc,"mime"),"readonly"),field_ui("preview","Text",&text(doc,"content"),"readonly")],buttons)},
 _=>return None,
 })
}
pub(super) fn remember(st:&mut Json,action:&str){
 if ["workspace","chat","history","config","settings","navigation","agent_settings"].contains(&action){put(st,"workspace_stack",Arr(vec![]));return;}
 let navigation=SECTIONS.contains(&action)||action.starts_with("mcp_tool:")||["cloud_setup","cloud_browse","cloud_read","mcp_client_menu","mcp_tools","mcp_streams","mcp_client_settings","mcp_client_call","mcp_client_review","phone_access","drive_edit","phone_files","phone_reader","phone_editor","phone_update_editor","phone_revision_status","drive_list","drive_reader","drive_editor","drive_update_editor","drive_folder","drive_copy","files","editor","reader","search","investigation","web_search","script_check","script_run","project_export","project_import","project_save","project_save_drive","artifact","inquiry","investigation_records","investigation_claims","investigation_experiments","reconstruction_new"].contains(&action);
 if !navigation{return;}let current=text(st,"dialog");if current.is_empty(){put(st,"workspace_stack",Arr(vec![]));return;}
 if !SECTIONS.contains(&current.as_str())&&!["cloud_setup","mcp_client_menu","mcp_tools","mcp_streams","mcp_client_call","storage_settings"].contains(&current.as_str()){return;}if current==action{return;}
 let mut stack=array(st,"workspace_stack");if stack.len()<12&&stack.last()!=Some(&s(&current)){stack.push(s(&current));put(st,"workspace_stack",Arr(stack));}
}

pub(super) fn back(st:&mut Json)->bool{
 let current=text(st,"dialog");if current=="mcp_client_review"{put(st,"mcp_client_pending",Null);}let mut stack=array(st,"workspace_stack");if let Some(previous)=stack.pop(){put(st,"workspace_stack",Arr(stack));put(st,"dialog",previous);return true;}
 if SECTIONS.contains(&current.as_str())||current=="cloud_setup"{put(st,"dialog",Null);return true;}false
}
pub(super) fn handles(action:&str)->bool{SECTIONS.contains(&action)||["workspace_access_save","workspace_defaults_save","workspace_create","cloud_setup","cloud_browse","cloud_sign_in","cloud_result","cloud_text_result","cloud_read","cloud_open","cloud_copy","cloud_disconnect","cloud_cancel","cloud_failure"].contains(&action)||action.starts_with("workspace_source_access:")||action.starts_with("workspace_select:")}
fn root_for(st:&Json,root:&str,id:&str)->Result<PathBuf>{
 let current=Path::new(root).canonicalize().map_err(|e|e.to_string())?;let parent=current.parent().ok_or("Workspace parent unavailable")?;
 let base=if text(st,"workspace_selected").is_empty(){current.file_name().ok_or("Workspace name unavailable")?.to_string_lossy().into_owned()}else{text(st,"workspace_original")};
 if base.is_empty()||base.contains('/')||base==".."{return Err("Original workspace location changed".into());}
 Ok(parent.join(if id.is_empty(){base}else{format!("workspace-{id}")}))
}
fn select(st:&mut Json,root:&str,id:&str)->Result<()>{
 if interaction::locked(st)||get(st,"pending")!=&Null||get(st,"mcp_client_pending")!=&Null||get(st,"project_transfer")!=&Null{return Err("Finish the current operation before switching workspaces".into());}
 let target=root_for(st,root,id)?;let info=fs::symlink_metadata(&target).map_err(|e|e.to_string())?;if !info.is_dir()||info.file_type().is_symlink(){return Err("Choose an existing app workspace directory".into());}
 if text(st,"workspace_selected").is_empty(){let name=Path::new(root).file_name().ok_or("Workspace name unavailable")?.to_string_lossy().into_owned();put(st,"workspace_original",s(&name));}
 put(st,"workspace_selected",s(id));approval_rules::namespace(st,&target.to_string_lossy());harness_rules::identity_refresh(st);put(st,"dialog",s("workspace_projects"));put(st,"page",s("workspace"));put(st,"workspace_stack",Arr(vec![]));put(st,"cloud_expected",Null);inquiry::forget(st);Ok(())
}
fn cloud_owner(st:&mut Json,phase:&str)->String{let ticket=now();let owner=obj(vec![("ticket",s(&ticket)),("phase",s(phase)),("root",get(st,"approval_root").clone()),("identity",get(st,"approval_root_identity").clone())]);put(st,"cloud_expected",owner);ticket}
pub(super) fn cloud_callback(action:&str)->bool{matches!(action,"cloud_result"|"cloud_text_result"|"cloud_cancel"|"cloud_failure")}
pub(super) fn cloud_response(inputs:&Json)->&Json{if get(inputs,"cloud_response")==&Null{inputs}else{get(inputs,"cloud_response")}}
pub(super) fn cloud_expected(st:&Json,action:&str,inputs:&Json)->bool{
 let owner=get(st,"cloud_expected");let phase=text(owner,"phase");!text(owner,"ticket").is_empty()&&text(owner,"ticket")==text(inputs,"ticket")&&get(owner,"root")==get(st,"approval_root")&&get(owner,"identity")==get(st,"approval_root_identity")&&matches!(phase.as_str(),"select"|"read")&&match action{"cloud_result"=>phase=="select","cloud_text_result"=>phase=="read","cloud_cancel"|"cloud_failure"=>true,_=>false}
}
pub(super) fn event(st:&mut Json,action:&str,inputs:&Json,root:&str)->Result<Json>{
 if let Some(provider)=action.strip_prefix("workspace_source_access:"){if !["phone","cloud"].contains(&provider){return Err("Choose a storage source".into());}let required=if provider=="phone"{"phone_access"}else{"cloud_setup"};if text(st,"dialog")!=required{return Err("Open the source setup first".into());}let Bool(value)=get(inputs,"access")else{return Err("Choose the source access toggle".into());};set_access(st,provider,*value);return Ok(obj(vec![("kind",s("storage_policy"))]));}
 if SECTIONS.contains(&action)||action=="cloud_setup"{put(st,"edit",obj(vec![]));put(st,"dialog",s(action));return Ok(Null);}
 match action{
 "workspace_access_save"=>{if text(st,"dialog")!="workspace_permissions"{return Err("Open source access permissions first".into());}let mut access=obj(vec![]);for key in ["phone","network","cloud","mcp"]{let Bool(value)=get(inputs,key)else{return Err("Choose each source access toggle".into());};put(&mut access,key,Bool(*value));}put(st,"storage_access",access);put(st,"notice",s("Source access permissions applied; connection settings retained"));return Ok(obj(vec![("kind",s("storage_policy"))]));},
 "workspace_defaults_save"=>{if text(st,"dialog")!="workspace_defaults"{return Err("Open Default saves first".into());}let phone=boolean(inputs,"phone_default");if phone&&text(st,"phone_uri").is_empty(){return Err("Select a phone folder in Setup first".into());}if !phone{drive_root(st)?;}let mut drive=get(st,"drive").clone();if drive==Null{drive=obj(vec![]);}put(&mut drive,"phone_default",Bool(phone));put(&mut drive,"autosave",Bool(boolean(inputs,"autosave")));put(st,"drive",drive);put(st,"notice",s("Default saves applied"));},
 "workspace_create"=>{
 if text(st,"dialog")!="workspace_new"||interaction::locked(st)||get(st,"pending")!=&Null||get(st,"mcp_client_pending")!=&Null||get(st,"project_transfer")!=&Null{return Err("Open New workspace after the current operation finishes".into());}let name=text(inputs,"name").trim().to_string();if name.is_empty()||name.len()>128||name.chars().any(char::is_control){return Err("Enter a workspace name of1–128 UTF-8 bytes".into());}let mut list=array(st,"workspaces");if list.len()>=32{return Err("The workspace list holds32 locations".into());}
 let id=format!("{}-{}",now(),list.len());let target=root_for(st,root,&id)?;list.push(obj(vec![("id",s(&id)),("name",s(&name))]));let mut candidate=st.clone();put(&mut candidate,"workspaces",Arr(list.clone()));if candidate.string().len()>6*1024*1024-8192{return Err("Workspace settings exceed the current saved-state allowance".into());}fs::create_dir(&target).map_err(|e|e.to_string())?;put(st,"workspaces",Arr(list));select(st,root,&id)?;put(st,"notice",s(&format!("Workspace {name} created")));},
 "cloud_sign_in"=>return Ok(obj(vec![("kind",s("cloud_account"))])),
 "cloud_browse"=>{if !allowed(st,"cloud"){put(st,"edit",obj(vec![]));put(st,"dialog",s("workspace_permissions"));return Ok(Null);}let ticket=cloud_owner(st,"select");return Ok(obj(vec![("kind",s("cloud_document")),("ticket",s(&ticket))]));},
 "cloud_open"|"cloud_read"=>{if !allowed(st,"cloud"){return Err("Google Drive access is off in Setup".into());}let doc=get(st,"cloud_document").clone();if !text(&doc,"uri").starts_with("content://"){return Err("Select a Google Drive file first".into());}let ticket=cloud_owner(st,"read");if action=="cloud_read"{put(st,"busy",Bool(true));}return Ok(obj(vec![("kind",s(if action=="cloud_open"{"cloud_open"}else{"cloud_read"})),("uri",get(&doc,"uri").clone()),("mime",get(&doc,"mime").clone()),("ticket",s(&ticket))]));},
 "cloud_copy"=>{if !allowed(st,"cloud"){return Err("Google Drive access is off in Setup".into());}let doc=get(st,"cloud_document");let Str(content)=get(doc,"content")else{return Err("Read the selected text first".into());};let name=text(doc,"name");let name=if name.is_empty()||name.contains('/')||name.contains('\\')||name==".."{"selected.txt"}else{&name};let edit=obj(vec![("path",s(name)),("content",s(content))]);put(st,"edit",edit);put(st,"dialog",s("editor"));},
 "cloud_disconnect"=>{put(st,"cloud_document",Null);put(st,"cloud_expected",Null);let mut access=get(st,"storage_access").clone();if access==Null{access=obj(vec![]);}put(&mut access,"cloud",Bool(false));put(st,"storage_access",access);return Ok(obj(vec![("kind",s("cloud_release"))]));},
 "cloud_result"|"cloud_text_result"|"cloud_cancel"|"cloud_failure"=>{
 if !cloud_expected(st,action,inputs){return Ok(Null);}let phase=text(get(st,"cloud_expected"),"phase");put(st,"cloud_expected",Null);if phase=="read"{put(st,"busy",Bool(false));}if !allowed(st,"cloud"){put(st,"notice",s("Google Drive access is off"));return Ok(Null);}
 let response=cloud_response(inputs);
 if action=="cloud_result"{let uri=text(response,"uri");let name=text(response,"name");let mime=text(response,"mime");if !uri.starts_with("content://")||uri.len()>8192||name.len()>512||mime.len()>256{return Err("Invalid selected document metadata".into());}put(st,"cloud_document",obj(vec![("uri",s(&uri)),("name",s(&name)),("mime",s(&mime))]));put(st,"dialog",s("cloud_selected"));put(st,"notice",s("Google Drive file selected"));}
 else if action=="cloud_text_result"{let content=text(response,"content");if content.len()>1024*1024{return Err("Selected text exceeds1MiB".into());}let mut doc=get(st,"cloud_document").clone();put(&mut doc,"content",s(&content));put(st,"cloud_document",doc);put(st,"dialog",s("cloud_selected"));put(st,"notice",s("Selected Google Drive text opened"));}else if action=="cloud_failure"{return Err(text(response,"error"));}},
 _=>{if let Some(id)=action.strip_prefix("workspace_select:"){let id=if id=="default"{""}else{id};if !id.is_empty()&&!array(st,"workspaces").iter().any(|v|text(v,"id")==id){return Err("Workspace location unavailable".into());}select(st,root,id)?;}}
 }Ok(if action=="workspace_create"||action.starts_with("workspace_source_access:")||action.starts_with("workspace_select:"){obj(vec![("kind",s("workspace_selected"))])}else{Null})
}
