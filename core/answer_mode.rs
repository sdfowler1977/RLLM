//! User-selected answer depth, conservative automatic routing, and local clock answers.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode { Adaptive, Direct, Lookup, Investigate }

impl Mode {
 pub(super) fn key(self)->&'static str{match self{
  Self::Adaptive=>"auto",Self::Direct=>"direct",Self::Lookup=>"lookup",Self::Investigate=>"investigate",
 }}
 fn from_key(value:&str)->Option<Self>{Some(match value{
  "auto"=>Self::Adaptive,"direct"=>Self::Direct,"lookup"=>Self::Lookup,"investigate"=>Self::Investigate,_=>return None,
 })}
 fn label(self)->&'static str{match self{
  Self::Adaptive=>"Auto",Self::Direct=>"Direct answer",Self::Lookup=>"Quick lookup",Self::Investigate=>"Investigation",
 }}
 pub(super) fn guidance(self)->&'static str{match self{
  Self::Adaptive=>"Choose the smallest useful response for this request. Answer familiar questions immediately. For a current fact, use a focused lookup and give the result first, with its source and date when relevant. Use investigation, workspace tools or delegation when the user asks for them or a problem needs them. For ordinary replies skip methodology, evidence IDs, observations-versus-interpretations headings and tool diagnostics.",
  Self::Direct=>"Answer the question directly in plain text. Lead with the answer. Keep routine answers short. This request uses no tools or investigation reports.",
  Self::Lookup=>"Quick lookup. Use only the permitted web tools and stop when you have the requested fact. Return the result first in one or two sentences, with the observation time or date and a source. Use a page only when the search results lack the answer. After an unavailable source, try at most one suitable alternative, then give the best verified result or say briefly what is unavailable. Omit internal evidence IDs, tool logs and investigation headings.",
  Self::Investigate=>"Use the investigation workflow when the task needs it. Lead with the finding and include evidence, uncertainty and methods in proportion to the request.",
 }}
}

const MODES:[Mode;4]=[Mode::Adaptive,Mode::Direct,Mode::Lookup,Mode::Investigate];
fn preference(st:&Json)->Mode{Mode::from_key(&text(&chat(st),"answer_mode")).unwrap_or(Mode::Adaptive)}
pub(super) fn label(st:&Json)->&'static str{preference(st).label()}
pub(super) fn dialog(st:&Json)->Json{
 modal("Answer mode",vec![field_ui("current","Current mode",label(st),"readonly"),
  field_ui("description","Conversation preference","Auto chooses a direct reply, quick lookup, or investigation from the request. Pick a mode to set the default for new messages in this conversation.","readonly")],
  MODES.into_iter().map(|mode|button(mode.label(),&format!("answer_mode:{}",mode.key()))).collect())
}
pub(super) fn save(st:&mut Json,key:&str)->Result<()>{
 if boolean(st,"busy")||get(st,"task_run")!=&Null||get(st,"pending")!=&Null{return Err("Finish the current request before changing answer mode".into());}
 let mode=Mode::from_key(key).ok_or("Choose a supported answer mode")?;
 if chat(st)==Null{new_chat(st)?;}
 let mut current=chat(st);put(&mut current,"answer_mode",s(mode.key()));replace_chat(st,current);
 put(st,"dialog",Null);put(st,"notice",s(&format!("Answer mode set to {}",mode.label())));Ok(())
}

fn words(prompt:&str)->Vec<&str>{prompt.split(|c:char|!c.is_alphanumeric()).filter(|part|!part.is_empty()).collect()}
fn clock_kind(parts:&[&str])->Option<bool>{
 let parts=parts.strip_prefix(&["please"]).unwrap_or(parts);
 let parts=parts.strip_suffix(&["please"]).unwrap_or(parts);
 match parts{
  ["what","time","is","it"]|["what","is","the","time"]|["what","is","the","current","time"]|
  ["what","s","the","time"]|["tell","me","the","time"]|["current","time"]=>Some(false),
  ["what","is","the","date"]|["what","s","the","date"]|["what","is","today","s","date"]|
  ["what","s","today","s","date"]|["what","date","is","it"]|["today","s","date"]=>Some(true),
  _=>None,
 }
}
fn dow(parts:&[&str])->bool{parts.contains(&"djia")||parts.windows(2).any(|p|p==["dow","jones"]||p==["the","dow"])}
fn classify(prompt:&str,attachments:bool)->Mode{
 if attachments{return Mode::Adaptive;}
 let lower=prompt.to_lowercase();let parts=words(&lower);
 let command=parts.strip_prefix(&["please"]).unwrap_or(&parts);
 if matches!(command,["investigate"|"diagnose"|"debug"|"troubleshoot",..]|["reverse","engineer",..]|
  ["find","the","root","cause",..]|["figure","out","why",..]|["use"|"run","rea",..]){return Mode::Investigate;}
 let current=parts.iter().any(|v|matches!(*v,"current"|"currently"|"today"|"latest"|"now"|"doing"|"trading"|"close"|"closed"|"quote"));
 let question=matches!(command,["what","is"|"s"|"are",..]|["how","is"|"s"|"are",..]);
 let search_directive=question && (parts.windows(3).any(|p|p==["search","the","web"]||p==["search","the","internet"])
     ||parts.windows(2).any(|p|p==["search","online"]||p==["look","online"]));
 let implied_current=question&&parts.len()<=10&&parts.iter().any(|word|matches!(*word,"score"|"worth"|"trading"));

 if search_directive||implied_current||(dow(&parts)&&current&&parts.len()<=18&&!parts.contains(&"why"))
    || matches!(command,["quick","lookup",..]|["quickly","look","up",..]|["what","time","is","it","in",..])
    || (current&&parts.len()<=14&&parts.iter().any(|v|matches!(*v,"weather"|"temperature"|"score"|"forecast"|"exchange"|"stock"))){
  return Mode::Lookup;
 }
 let one_question=!lower.trim_end_matches(|c:char|c.is_whitespace()||matches!(c,'?'|'!'|'.')).contains(['?','!','.',';','\n']);
 let simple_definition=one_question&&(matches!(command,["what","is"|"s",term @ ..]
    if (1..=5).contains(&term.len())&&!term.iter().any(|word|matches!(*word,
       "this"|"that"|"these"|"those"|"my"|"our"|"your"|"in"|"on"|"from"|"at"|"here"|
       "file"|"link"|"current"|"latest"|"today"|"new"|"price"|"population"|"weather"|"stock"|"next"|"last"|"worth"|"score")))
    || matches!(command,["what","does",term @ ..,"mean"] if (1..=4).contains(&term.len())
       && !term.iter().any(|word|matches!(*word,"this"|"that"|"these"|"those"|"my"|"our")))
    || matches!(command,["how","many","grams"|"milligrams"|"milliliters"|"centimeters","are","in","a"|"an","kilogram"|"gram"|"liter"|"meter"]));
 if clock_kind(&parts).is_some()||simple_definition
    || matches!(command,["hi"|"hello"|"hey"|"thanks"]|["thank","you"])
    || (matches!(command,["define",term] if !term.is_empty())){
  return Mode::Direct;
 }
 Mode::Adaptive
}
pub(super) fn current(st:&Json)->Mode{
 if get(st,"task_run")!=&Null{return Mode::Investigate;}
 let turn=get(st,"answer_turn");
 let origin=get(st,"harness_native_source");
 if get(turn,"chat")!=get(st,"active")||get(turn,"source")!=get(origin,"id"){return Mode::Adaptive;}
 Mode::from_key(&text(turn,"mode")).unwrap_or(Mode::Adaptive)
}
pub(super) fn capture(st:&mut Json,prompt:&str,inputs:&Json,attachments:bool){
 let selected=preference(st);
 let mode=if get(st,"pending")!=&Null{Mode::Adaptive}
 else if selected==Mode::Adaptive{classify(prompt,attachments)}else{selected};
 put(st,"answer_turn",obj(vec![("chat",get(st,"active").clone()),("source",get(get(st,"harness_native_source"),"id").clone()),
  ("mode",s(mode.key())),("offset_minutes",Num(offset(inputs).to_string())),("dow",Bool(mode==Mode::Lookup&&dow(&words(&prompt.to_lowercase()))))]));
}
pub(super) fn local_clock(st:&Json,prompt:&str,attachments:bool,inputs:&Json)->Option<String>{
 if attachments||!matches!(preference(st),Mode::Adaptive|Mode::Direct){return None;}
 let date_only=clock_kind(&words(&prompt.to_lowercase()))?;
 let offset=if matches!(get(inputs,"offset_minutes"),Num(_)){offset(inputs)}else{offset(get(st,"answer_turn"))};
 let (day,minutes)=local_day(offset,0);
 if date_only{return Some(format!("Today is {day}."));}
 let hour=minutes/60;
 let meridiem=if hour<12{"AM"}else{"PM"};
 Some(format!("It’s {}:{:02} {meridiem} {}.",(hour+11)%12+1,minutes%60,offset_label(offset)))
}
fn offset(v:&Json)->i64{
 match get(v,"offset_minutes"){Num(n)=>n.parse::<i64>().ok().filter(|n|(-840..=840).contains(n)).unwrap_or(0),_=>0}
}
fn offset_label(offset:i64)->String{
 if offset==0{return "UTC".into();}
 let minutes=offset.abs();
 format!("UTC{}{:02}:{:02}",if offset<0{"−"}else{"+"},minutes/60,minutes%60)
}
fn local_day(offset:i64,ago:i64)->(String,i64){
 let seconds=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64+offset*60-ago*86400;
 let (year,month,day)=civil_date(seconds.div_euclid(86400));
 (format!("{year:04}-{month:02}-{day:02}"),seconds.rem_euclid(86400)/60)
}
pub(super) fn lookup_context(st:&Json)->String{
 let turn=get(st,"answer_turn");
 let tz=offset(turn);
 let (today,minute)=local_day(tz,0);
 let mut result=format!("\nCurrent clock when preparing this lookup: {today} {:02}:{:02} {}. A source observation date is separate from the current date.",minute/60,minute%60,offset_label(tz));
 if boolean(turn,"dow"){
  result.push_str(" The DJIA index is distinct from the DIA ETF. Match the requested date and measure.");
  if latest_market_question(&harness_rules::native_source(st)){
   let (since,_)=local_day(tz,14);
   result.push_str(&format!(" Prefer a timestamped index value and move. If only the most recent daily close can be verified, label it as the close and include its date. A bounded FRED daily-close CSV is https://fred.stlouisfed.org/graph/fredgraph.csv?id=DJIA&cosd={since} . Read its last two numeric observations if useful. A page without an actual index value is insufficient."));
  }
 }
 result
}
fn valid_market_date(date:&str)->bool{
 if !date.bytes().all(|b|b.is_ascii_digit()||b==b'-'){return false;}
 let mut parts=date.split('-');
 let Some(year)=parts.next().filter(|v|v.len()==4).and_then(|v|v.parse::<u32>().ok())else{return false;};
 let Some(month)=parts.next().filter(|v|v.len()==2).and_then(|v|v.parse::<usize>().ok())else{return false;};
 let Some(day)=parts.next().filter(|v|v.len()==2).and_then(|v|v.parse::<u32>().ok())else{return false;};
 let february=if year%4==0&&(year%100!=0||year%400==0){29}else{28};
 let months=[31,february,31,30,31,30,31,31,30,31,30,31];
 parts.next().is_none()&&month>0&&months.get(month-1).is_some_and(|limit|(1..=*limit).contains(&day))
}
pub(super) fn native_started(st:&Json)->bool{current(st)==Mode::Lookup&&boolean(get(st,"answer_turn"),"native_started")}
pub(super) fn mark_native_started(st:&mut Json){
 if current(st)==Mode::Lookup{let mut turn=get(st,"answer_turn").clone();put(&mut turn,"native_started",Bool(true));put(st,"answer_turn",turn);}
}
fn latest_market_question(prompt:&str)->bool{
 let lower=prompt.to_lowercase();
 let parts=words(&lower);
 let parts=parts.strip_prefix(&["please"]).unwrap_or(&parts);
 let parts=parts.strip_suffix(&["please"]).unwrap_or(parts);
 let ["what"|"how","is"|"s"|"was",rest @ ..]=parts else{return false;};
 let rest=rest.strip_prefix(&["the"]).unwrap_or(rest);
 const NAMES:[&[&str];4]=[&["djia"],&["dow","jones","industrial","average"],&["dow","jones"],&["dow"]];
 if let Some(suffix)=NAMES.iter().find_map(|name|rest.strip_prefix(*name)){
  let suffix=suffix.strip_prefix(&["s"]).unwrap_or(suffix);
  return matches!(suffix,["doing"]|["doing","today"|"now"]|["last"|"latest","close"]|["current","level"]);
 }
 if let ["last"|"latest","close","for"|"of",index @ ..]=rest{
  return NAMES.contains(&index.strip_prefix(&["the"]).unwrap_or(index));
 }
 false
}
pub(super) fn market_close(st:&Json,context:&Json,output:&str)->Option<String>{
 if current(st)!=Mode::Lookup||!boolean(get(st,"answer_turn"),"dow"){return None;}
 if !latest_market_question(&harness_rules::native_source(st)){return None;}
 let url=network::url(&text(context,"source"),false).ok()?;
 let (path,query)=url.path.split_once('?')?;
 if url.host!="fred.stlouisfed.org"||url.port!=443||path!="/graph/fredgraph.csv"||!query.split('&').any(|field|field=="id=DJIA"){return None;}
 let mut lines=output.lines().skip_while(|line|line.trim()!="observation_date,DJIA");
 lines.next()?;
 let mut latest=None;
 let mut previous=None;
 let mut prior_date="";
 let (today,_)=local_day(offset(get(st,"answer_turn")),0);
 let (earliest,_)=local_day(offset(get(st,"answer_turn")),14);
 for line in lines.take(32){
  let Some((date,value))=line.trim().split_once(',')else{break;};
  if !valid_market_date(date)||date<=prior_date||date>today.as_str(){return None;}
  prior_date=date;
  if value=="."||value.is_empty(){continue;}
  let level=value.parse::<f64>().ok().filter(|v|v.is_finite()&&*v>0.0&&*v<1e7)?;
  previous=latest;latest=Some((date,level));
 }
 let (date,level)=latest.filter(|(date,_)|*date>=earliest.as_str())?;
 let move_text=previous.map(|(_,prior)|format!(", {:+.2}% from the previous close",100.0*(level-prior)/prior)).unwrap_or_default();
 Some(format!("The DJIA closed at {level:.2} on {date}{move_text} (FRED daily close)."))
}
pub(super) fn project_lightweight(st:&Json,messages:&mut Vec<Json>){
 let mode=current(st);
 if !matches!(mode,Mode::Direct|Mode::Lookup){return;}
 let native=get(get(st,"answer_turn"),"source");
 let mut in_turn=false;
 messages.retain(|message|{
  if native!=&Null&&get(message,"harness_user_id")==native{in_turn=true;}
  in_turn||(!boolean(message,"harness_outcome")&&text(message,"web_result_label").is_empty())
 });
 if mode!=Mode::Lookup{return;}
 for message in messages{
  if text(message,"web_result_label").is_empty(){continue;}
  let body=text(message,"content");
  if body.len()<=12288{continue;}
  let mut end=12288;
  while !body.is_char_boundary(end){end-=1;}
  put(message,"content",s(&format!("{}\n[Additional source details are available in conversation history.]",&body[..end])));
 }
}
pub(super) fn permits(st:&Json,tool:&str,args:&Json)->bool{
 match current(st){
  Mode::Direct=>false,
  Mode::Lookup=>tool=="web_search"||(tool=="web_read"&&get(args,"capture")==&Null&&page_program::enabled(args)==Ok(false)),
  _=>true,
 }
}
