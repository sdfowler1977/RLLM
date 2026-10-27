//! Explicit per-connection generation controls; defaults never add provider flags.
use super::*;
pub(super) fn migrated(p:&Json)->Json {
 let mut out=p.clone();
 if get(p,"thinking")==&Null {let old=text(p,"reasoning");let value=old.strip_prefix("chat_").or_else(||old.strip_prefix("local_")).or_else(||old.strip_prefix("messages_")).or_else(||old.strip_prefix("content_")).unwrap_or("");put(&mut out,"thinking",s(value));}
 if get(p,"output_field")==&Null {put(&mut out,"output_field",s(&text(p,"output_parameter")));}
 if get(p,"context_tokens")==&Null && text(p,"protocol")=="local" {put(&mut out,"context_tokens",get(p,"local_context_tokens").clone());}
 out
}
fn canonical(p:&Json)->Json {
 let mut out=p.clone();let mode=text(p,"thinking");let protocol=text(p,"protocol");
 let reason=match protocol.as_str(){"chat" if !["","default"].contains(&mode.as_str())=>format!("chat_{mode}"),"local" if ["on","off"].contains(&mode.as_str())=>format!("local_{mode}"),"messages" if mode.starts_with("adaptive")=>format!("messages_{mode}"),"content" if ["low","high"].contains(&mode.as_str())=>format!("content_{mode}"),_=>String::new()};
 put(&mut out,"reasoning",s(&reason));put(&mut out,"output_parameter",s(if protocol=="chat"{get(p,"output_field").text().unwrap_or("")}else{""}));
 put(&mut out,"local_context_tokens",if protocol=="local"{get(p,"context_tokens").clone()}else{Null});out
}
pub(super) fn validate_bytes(p:&Json)->Result<()> {crate::generation::validate(&canonical(&migrated(p)),&text(p,"protocol"))}
fn integer(v:&Json,key:&str,max:usize)->Result<usize>{let raw=match get(v,key){Null=>String::new(),Str(x)|Num(x)=>x.trim().into(),_=>return Err(format!("Invalid {key}"))};if raw.trim().is_empty(){return Ok(0);}let n=raw.parse::<usize>().map_err(|_|format!("{key}: enter a whole number"))?;if n>max{return Err(format!("{key} exceeds {max}"));}Ok(n)}
pub(crate) fn validate(p:&Json)->Result<()> {
    if get(p,"thinking")==&Null {crate::generation::validate(p,&text(p,"protocol"))?;}
    let migrated=migrated(p);let p=&migrated;validate_bytes(p)?;
    let mode=text(p,"thinking");let protocol=text(p,"protocol");
    let allowed=match protocol.as_str(){"chat"=>vec!["","default","none","minimal","low","medium","high","xhigh","max"],"local"=>vec!["","default","off","on","low","medium","high","max"],"messages"=>vec!["","default","off","adaptive","adaptive_low","adaptive_medium","adaptive_high","budget"],"content"=>vec!["","default","low","high","budget"],_=>vec![""]};
    if !allowed.contains(&mode.as_str()){return Err("Thinking setting does not match this API type; select Server default".into());}
    let output=integer(p,"output_tokens",i32::MAX as usize)?;let context=integer(p,"context_tokens",i32::MAX as usize)?;let budget=integer(p,"thinking_tokens",1_000_000)?;
    if protocol!="local" && context>0 && (context<256 || output>=context){return Err("Context must be at least 256 and larger than the output limit".into());}
    if protocol=="messages"&&mode=="budget"&&(budget<1024||budget>=if output==0{4096}else{output}){return Err("Thinking budget must be at least 1024 and below the output limit".into());}
    if !["","max_tokens","max_completion_tokens"].contains(&text(p,"output_field").as_str()){return Err("Choose the output parameter".into());}Ok(())
}
pub(super) fn apply(p:&Json,body:&mut Json,messages:&[Json])->Result<()> {
    validate(p)?;let migrated=migrated(p);let p=&migrated;crate::generation::apply(body,&canonical(p),&text(p,"protocol"),messages)?;
    validate(p)?;let mode=text(p,"thinking");let protocol=text(p,"protocol");let output=integer(p,"output_tokens",i32::MAX as usize)?;let context=integer(p,"context_tokens",i32::MAX as usize)?;let budget=integer(p,"thinking_tokens",1_000_000)?;
    // Advisory user budget, not an assertion about a remote tokenizer or capacity.
    let estimated=(body.string().len()+3)/4;
    if protocol!="local" && context>0 && estimated+output>context{return Err(format!("Estimated context budget exceeded ({estimated} input + {output} reserved output). Compact this conversation or adjust this connection's context budget."));}
    match protocol.as_str(){
        "chat"=>{if output>0{let key=if text(p,"output_field")=="max_tokens"{"max_tokens"}else{"max_completion_tokens"};put(body,key,Num(output.to_string()));}if !mode.is_empty()&&mode!="default"{put(body,"reasoning_effort",s(&mode));}},
        "local"=>{let mut options=obj(vec![]);if context>0{put(&mut options,"num_ctx",Num(context.to_string()));}if output>0{put(&mut options,"num_predict",Num(output.to_string()));}if context>0||output>0{put(body,"options",options);}match mode.as_str(){"on"=>put(body,"think",Bool(true)),"off"=>put(body,"think",Bool(false)),""|"default"=>{},_=>put(body,"think",s(&mode))}},
        "messages"=>{if output>0{put(body,"max_tokens",Num(output.to_string()));}if mode=="budget"{put(body,"thinking",obj(vec![("type",s("enabled")),("budget_tokens",Num(budget.to_string()))]));}else if let Some(effort)=mode.strip_prefix("adaptive_"){put(body,"thinking",obj(vec![("type",s("adaptive"))]));put(body,"output_config",obj(vec![("effort",s(effort))]));}else if mode=="off"{put(body,"thinking",obj(vec![("type",s("disabled"))]));}},
        "content"=>{let mut config=obj(vec![]);if output>0{put(&mut config,"maxOutputTokens",Num(output.to_string()));}if mode=="budget"{put(&mut config,"thinkingConfig",obj(vec![("thinkingBudget",Num(budget.to_string()))]));}else if ["low","high"].contains(&mode.as_str()){put(&mut config,"thinkingConfig",obj(vec![("thinkingLevel",s(&mode))]));}if output>0||(!mode.is_empty()&&mode!="default"){put(body,"generationConfig",config);}},
        _=>{}
    }Ok(())
}
pub(super) fn modal_for(st:&Json)->Json {
    let migrated=migrated(get(st,"edit"));let p=&migrated;let protocol=text(p,"protocol");let mode=text(p,"thinking");let mut options=vec![("Server default","default")];
    match protocol.as_str(){"chat"=>options.extend([("None","none"),("Minimal","minimal"),("Low","low"),("Medium","medium"),("High","high"),("Extra high","xhigh"),("Maximum","max")]),"local"=>options.extend([("Off","off"),("On","on"),("Low","low"),("Medium","medium"),("High","high"),("Maximum","max")]),"messages"=>options.extend([("Off","off"),("Adaptive","adaptive"),("Adaptive · low","adaptive_low"),("Adaptive · medium","adaptive_medium"),("Adaptive · high","adaptive_high"),("Manual token budget","budget")]),"content"=>options.extend([("Low","low"),("High","high"),("Token budget","budget")]),_=>{}}
    let mut fields=vec![interaction::choice("thinking","Thinking level",if mode.is_empty(){"default"}else{&mode},&options),field_ui("context_tokens",if protocol=="local"{"Context size (tokens; blank = server default)"}else{"Context budget (estimated tokens; blank = no app limit)"},&text(p,"context_tokens"),"text"),field_ui("output_tokens","Maximum output tokens (blank = default)",&text(p,"output_tokens"),"text")];
    if protocol=="chat"{fields.push(interaction::choice("output_field","Output parameter",if text(p,"output_field").is_empty(){""}else{p.get("output_field").unwrap().text().unwrap_or("")},&[("Choose supported field",""),("max_completion_tokens","max_completion_tokens"),("max_tokens (compatible servers)","max_tokens")]));}
    if ["messages","content"].contains(&protocol.as_str()){fields.push(field_ui("thinking_tokens","Thinking token budget (manual budget mode)",&text(p,"thinking_tokens"),"text"));}
    fields.push(field_ui("context_bytes","Context budget (text + encoded image bytes; optional)",&text(p,"context_bytes"),"text"));
    fields.push(field_ui("compaction_bytes","Compaction reminder (text + encoded image bytes; optional)",&text(p,"compaction_bytes"),"text"));
    fields.push(field_ui("support","Provider support","Choose only settings supported by this model/server. Server default sends no thinking override. Estimated token budgets are approximate. Byte budgets count exact UTF-8 message content. Neither increases server capacity.","readonly"));
    fields.push(field_ui("model_metadata","Model-reported capabilities",&model_metadata::display(st),"readonly"));
    let mut buttons=vec![button("Apply to connection","generation_apply"),button("Reset overrides","profile_generation_reset")];
    if ["content","local"].contains(&protocol.as_str()) {buttons.insert(0,button("Read model capabilities","model_metadata"));}
    if protocol=="local" {buttons.insert(1,button("Read server version","model_metadata_version"));}
    modal("Thinking and context",fields,buttons)
}
