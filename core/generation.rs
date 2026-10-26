//! Explicit per-connection generation capabilities and non-token context budgets.
use super::*;
fn value<'a>(p:&'a Json,k:&str)->Result<&'a str>{match p.get(k){Err(_)|Ok(Null)=>Ok(""),Ok(Str(v))|Ok(Num(v))=>Ok(v.trim()),_=>Err(format!("Invalid {k}"))}}
pub fn integer(p:&Json,k:&str,max:usize)->Result<Option<usize>>{
 let v=value(p,k)?;if v.is_empty(){return Ok(None)}
 if !v.bytes().all(|b|b.is_ascii_digit()){return Err(format!("{k}: enter a positive whole number or leave blank"))}
 let n=v.parse::<usize>().map_err(|_|format!("{k} exceeds limit"))?;
 if n==0||n>max{return Err(format!("{k} must be 1–{max}"))}Ok(Some(n))
}
pub fn validate(p:&Json,protocol:&str)->Result<()> {
 if !matches!(p,Null|Obj(_)){return Err("Generation settings must be an object".into())}
 let r=value(p,"reasoning")?;
 let valid=match protocol{
 "chat"=>["","chat_none","chat_minimal","chat_low","chat_medium","chat_high","chat_xhigh","chat_max"].contains(&r),
 "local"=>["","local_on","local_off"].contains(&r),
 "messages"=>["","messages_adaptive","messages_adaptive_low","messages_adaptive_medium","messages_adaptive_high"].contains(&r),
 "content"=>["","content_low","content_high"].contains(&r),_=>false};
 if !valid{return Err("Reasoning selection is not supported by this API type. Return to Thinking and context and choose Server default or a supported capability.".into())}
 let output=integer(p,"output_tokens",i32::MAX as usize)?;
 let parameter=value(p,"output_parameter")?;
 if protocol=="chat" {
  if !["","max_tokens","max_completion_tokens"].contains(&parameter){return Err("Choose the output parameter supported by this chat server".into())}
  if output.is_some()&&parameter.is_empty(){return Err("Confirm the chat server's supported output parameter before setting an output limit".into())}
 }else if !parameter.is_empty(){return Err("Clear the chat-only output parameter after changing API type".into())}
 let context=integer(p,"context_bytes",8*1024*1024)?;
 let threshold=integer(p,"compaction_bytes",8*1024*1024)?;
 if let (Some(c),Some(t))=(context,threshold){if t>=c{return Err("Compaction reminder must be below the context byte budget".into())}}
 if integer(p,"local_context_tokens",i32::MAX as usize)?.is_some()&&protocol!="local"{return Err("Requested server context is only supported by the Ollama chat adapter".into())}
 Ok(())
}
fn set(p:&mut Json,k:&str,v:Json){if let Obj(fields)=p{fields.insert(k.into(),v);}}
pub fn content_bytes(messages:&[Json])->Result<usize>{messages.iter().try_fold(0usize,|n,m|n.checked_add(field(m,"content")?.len()).ok_or_else(||"Context size overflow".into()))}
pub fn apply(body:&mut Json,p:&Json,protocol:&str,messages:&[Json])->Result<()> {
 validate(p,protocol)?;
 let bytes=content_bytes(messages)?;
 if let Some(limit)=integer(p,"context_bytes",8*1024*1024)?{if bytes>limit{return Err(format!("Request has {bytes} UTF-8 content bytes, above this connection's {limit}-byte context budget. No messages were trimmed. Compact context explicitly or adjust this connection; the server's token capacity is unchanged."))}}
 let output=integer(p,"output_tokens",i32::MAX as usize)?;
 let r=value(p,"reasoning")?;
 match protocol{
 "chat"=>{if let Some(n)=output{set(body,value(p,"output_parameter")?,Num(n.to_string()));}if let Some(effort)=r.strip_prefix("chat_"){set(body,"reasoning_effort",s(effort));}},
 "local"=>{let mut options=obj(vec![]);if let Some(n)=output{set(&mut options,"num_predict",Num(n.to_string()));}if let Some(n)=integer(p,"local_context_tokens",i32::MAX as usize)?{set(&mut options,"num_ctx",Num(n.to_string()));}if options!=obj(vec![]){set(body,"options",options);}if !r.is_empty(){set(body,"think",Bool(r=="local_on"));}},
 "messages"=>{if let Some(n)=output{set(body,"max_tokens",Num(n.to_string()));}if !r.is_empty(){set(body,"thinking",obj(vec![("type",s("adaptive"))]));if let Some(effort)=r.strip_prefix("messages_adaptive_"){set(body,"output_config",obj(vec![("effort",s(effort))]));}}},
 "content"=>{let mut config=obj(vec![]);if let Some(n)=output{set(&mut config,"maxOutputTokens",Num(n.to_string()));}if let Some(level)=r.strip_prefix("content_"){set(&mut config,"thinkingConfig",obj(vec![("thinkingLevel",s(level))]));}if config!=obj(vec![]){set(body,"generationConfig",config);}},_=>{}
 }Ok(())
}
#[cfg(test)] mod checks{
 use super::*;
 #[test]fn capability_defaults_and_exact_mappings(){
  let messages=vec![obj(vec![("role",s("user")),("content",s("é"))])];
  for protocol in ["chat","local","messages","content"]{let mut b=obj(vec![]);apply(&mut b,&obj(vec![]),protocol,&messages).unwrap();assert_eq!(b,obj(vec![]));}
  for (protocol,reasoning,key) in [("chat","chat_low","reasoning_effort"),("local","local_off","think"),("messages","messages_adaptive_high","thinking"),("content","content_high","generationConfig")]{let p=obj(vec![("reasoning",s(reasoning))]);let mut b=obj(vec![]);apply(&mut b,&p,protocol,&messages).unwrap();assert!(b.get(key).is_ok());for wrong in ["chat","local","messages","content"]{if wrong!=protocol{assert!(validate(&p,wrong).is_err());}}}
  assert!(validate(&obj(vec![("output_tokens",s("20"))]),"chat").is_err());
  assert!(validate(&obj(vec![("context_bytes",Bool(true))]),"local").is_err());
  assert!(validate(&obj(vec![("context_bytes",s("20")),("compaction_bytes",s("20"))]),"local").is_err());
 }
 #[test]fn budgets_use_bytes_and_never_mutate_messages(){let m=vec![obj(vec![("role",s("system")),("content",s("é"))])];let copy=m.clone();assert_eq!(content_bytes(&m).unwrap(),2);assert!(apply(&mut obj(vec![]),&obj(vec![("context_bytes",s("1"))]),"chat",&m).is_err());assert_eq!(m,copy);}
}
