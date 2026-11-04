//! Typed encoding relationships, not an executable graph or recovered program.
use super::*;
const ROWS:usize=48;
const BYTES:usize=12*1024;
fn integer(v:&Json)->Result<u64>{match v{Num(n)=>n.parse().map_err(|_|"Invalid encoded-flow integer".into()),_=>Err("Encoded-flow integer missing".into())}}
fn maybe(v:Option<u64>)->Json{v.map(|v|Num(v.to_string())).unwrap_or(Null)}

/// Derive only relationships encoded by a captured branch word. Targets are
/// source coordinates; resolution names an already-retained word, never a
/// function, runtime address, reachable block or dynamically bound callee.
pub(super) fn derive(nodes:&[Json],coordinate:&str,cancelled:&dyn Fn()->bool)->Result<Json>{derive_version(nodes,coordinate,3,cancelled)}
pub(super) fn derive_version(nodes:&[Json],coordinate:&str,version:u64,cancelled:&dyn Fn()->bool)->Result<Json>{
 if ![1,2,3].contains(&version){return Err("Unsupported binary decoder version".into());}
 if nodes.len()>500||!["ELF declared VA","PE RVA"].contains(&coordinate){return Err("Invalid encoded-flow source scope".into());}
 let mut targets=BTreeMap::<u64,Option<u64>>::new();
 for node in nodes.iter().skip(1){
  let encoding=get(node,"encoding");let address=integer(get(encoding,"address"))?;let id=integer(get(node,"id"))?;targets.entry(address).and_modify(|v|*v=None).or_insert(Some(id));
 }
 let mut rows=vec![];let mut observed=0usize;let mut cost=0usize;let mut unknown_words=0usize;
 for node in nodes.iter().skip(1){
  if cancelled(){return Err("Encoded-flow projection cancelled".into());}
  let encoding=get(node,"encoding");let bits=u32::try_from(integer(get(encoding,"bits"))?).map_err(|_|"Invalid encoding word")?;
  let address=integer(get(encoding,"address"))?;let source=integer(get(node,"id"))?;
  let Some(branch)=control_flow::decode_version(bits,version) else{if get(encoding,"recognized")!=&Bool(true){unknown_words+=1;}continue};
  let mut retain=|relation:&str,target:Option<u64>,operand:Json|{
   observed+=1;
   let in_width=target.filter(|t|coordinate!="PE RVA"||*t<=u32::MAX as u64);
   let resolved=in_width.and_then(|t|targets.get(&t).copied().flatten());
   let row=obj(vec![("source",Num(source.to_string())),("word_bits",Num(bits.to_string())),("relation",s(relation)),("coordinate_kind",s(coordinate)),("target_address",maybe(target)),("target_node",maybe(resolved)),("resolution",s(if target.is_none(){"unresolved indirect target or address overflow"}else if in_width.is_none(){"outside declared coordinate width"}else if in_width.is_some_and(|t|targets.get(&t)==Some(&None)){"ambiguous retained coordinate"}else if resolved.is_some(){"retained encoding word; execution unobserved"}else{"target not present in retained encoding nodes"})),("operand",operand)]);
   let size=super::super::mcp_schema::state_bytes(&row);
   if rows.len()<ROWS&&cost.saturating_add(size)<=BYTES{cost+=size;rows.push(row);}
  };
  let next=address.checked_add(4);
  use control_flow::Branch::*;
  match branch{
   Direct{link,offset}=>{
    retain(if link{"encoded_call_target"}else{"encoded_branch_target"},control_flow::displaced(address,offset),obj(vec![("displacement",Num(offset.to_string()))]));
    if link{retain("encoded_link_return_address",next,obj(vec![]));}
   },
   Conditional{condition,offset,consistent}=>{
    let mut operand=obj(vec![("condition",Num(condition.to_string())),("displacement",Num(offset.to_string()))]);if consistent{put(&mut operand,"consistency_hint",Bool(true));put(&mut operand,"required_feature",s("FEAT_HBC"));}
    retain("encoded_conditional_target",control_flow::displaced(address,offset),operand.clone());
    // AL/NV encodings have no conditional false alternative. No NZCV or
    // execution condition is guessed for the other encoded condition values.
    if condition<14{retain("candidate_not_taken_address",next,if consistent{let mut fallthrough=obj(vec![("condition",Num(condition.to_string()))]);put(&mut fallthrough,"consistency_hint",Bool(true));put(&mut fallthrough,"required_feature",s("FEAT_HBC"));fallthrough}else{obj(vec![("condition",Num(condition.to_string()))])});}
   },
   Compare{nonzero,wide,register,offset}=>{
    let operand=obj(vec![("register",Num(register.to_string())),("register_width",Num(if wide{"64"}else{"32"}.into())),("nonzero",Bool(nonzero)),("displacement",Num(offset.to_string()))]);
    retain("encoded_compare_target",control_flow::displaced(address,offset),operand.clone());if register!=31||nonzero{retain("candidate_not_taken_address",next,operand);}
   },
   Test{nonzero,bit,register,offset}=>{
    let operand=obj(vec![("register",Num(register.to_string())),("bit",Num(bit.to_string())),("nonzero",Bool(nonzero)),("displacement",Num(offset.to_string()))]);
    retain("encoded_test_target",control_flow::displaced(address,offset),operand.clone());if register!=31||nonzero{retain("candidate_not_taken_address",next,operand);}
   },
   Authenticated{kind,register,modifier,key_b}=>{
    let mut operand=obj(vec![("register",Num(register.to_string())),("modifier_kind",s(match modifier{None=>"zero",Some(31)=>"SP",_=>"register"})),("modifier_register",modifier.map(|r|Num(r.to_string())).unwrap_or(Null)),("authentication_key",s(if key_b{"B"}else{"A"})),("required_feature",s("FEAT_PAuth"))]);
    if kind==2{put(&mut operand,"second_modifier",obj(vec![("register",Num("16".into())),("condition",s("FEAT_PAuth_LR and PSTATE.PACM = 1"))]));}
    retain(["encoded_authenticated_indirect_branch","encoded_authenticated_indirect_call","encoded_authenticated_indirect_return"][kind as usize],None,operand);
    if kind==1{retain("encoded_link_return_address",next,obj(vec![]));}
   },
   Register{kind,register}=>{
    retain(match kind{0=>"encoded_indirect_branch",1=>"encoded_indirect_call",_=>"encoded_indirect_return"},None,obj(vec![("register",Num(register.to_string()))]));
    if kind==1{retain("encoded_link_return_address",next,obj(vec![]));}
   }
  }
 }
 Ok(obj(vec![("schema",Num("1".into())),("kind",s("encoded_control_flow")),("coordinate_kind",s(coordinate)),("coverage",s("Partial")),("observed_relations",Num(observed.to_string())),("retained_relations",Num(rows.len().to_string())),("omitted_relations",Num((observed-rows.len()).to_string())),("unclassified_words",Num(unknown_words.to_string())),("relations",Arr(rows)),("meaning",s("Relationships derived from retained AArch64 encoding bits. Target nodes identify retained words only. Link return addresses do not prove returns. Register targets, execution, reachability, relocation effects, function identity, types and behavior remain unresolved."))]))
}
pub(super) fn page(flow:&Json,chosen:&[usize])->Json{
 let mut out=flow.clone();let relations=array(flow,"relations").into_iter().filter(|r|matches!(get(r,"source"),Num(n) if n.parse::<usize>().ok().is_some_and(|i|chosen.contains(&0)||chosen.contains(&i)))).collect::<Vec<_>>();
 put(&mut out,"page_relations",Num(relations.len().to_string()));put(&mut out,"relations",Arr(relations));out
}

#[cfg(test)]mod tests{
 use super::*;
 fn node(id:u64,address:u64,bits:u32)->Json{obj(vec![("id",Num(id.to_string())),("encoding",obj(vec![("address",Num(address.to_string())),("bits",Num(bits.to_string())),("recognized",Bool(true))]))])}
 #[test]fn forward_backward_indirect_and_link_addresses_are_distinct(){
  let nodes=vec![Null,node(1,0x1000,0x94000002),node(2,0x1004,0x17ffffff),node(3,0x1008,0xd65f03c0)];
  let graph=derive(&nodes,"ELF declared VA",&||false).unwrap();let rows=array(&graph,"relations");assert_eq!(rows.len(),4);
  assert_eq!(text(&rows[0],"relation"),"encoded_call_target");assert_eq!(get(&rows[0],"target_node"),&Num("3".into()));
  assert_eq!(text(&rows[1],"relation"),"encoded_link_return_address");assert_eq!(get(&rows[1],"target_node"),&Num("2".into()));
  assert_eq!(get(&rows[2],"target_node"),&Num("1".into()));assert_eq!(get(&rows[3],"target_address"),&Null);assert_eq!(text(&rows[3],"relation"),"encoded_indirect_return");
 }
 #[test]fn missing_targets_and_width_overflow_are_not_resolved(){
  let nodes=vec![Null,node(1,0xfffffffc,0x14000001),node(2,0,0x17ffffff)];let graph=derive(&nodes,"PE RVA",&||false).unwrap();let rows=array(&graph,"relations");
  assert_eq!(text(&rows[0],"resolution"),"outside declared coordinate width");assert_eq!(get(&rows[0],"target_node"),&Null);assert_eq!(get(&rows[1],"target_address"),&Null);
 }
 #[test]fn conditional_rows_keep_operands_and_do_not_invent_unknown_fallthrough(){
  let nodes=vec![Null,node(1,0x1000,0x54000040),node(2,0x1004,0xffffffff),node(3,0x1008,0x5400004e)];
  let graph=derive(&nodes,"ELF declared VA",&||false).unwrap();let rows=array(&graph,"relations");assert_eq!(rows.len(),3);assert_eq!(get(&rows[0],"target_node"),&Num("3".into()));assert_eq!(text(&rows[1],"relation"),"candidate_not_taken_address");assert_eq!(get(&rows[2],"operand").get("condition").unwrap(),&Num("14".into()));
  assert_eq!(array(&page(&graph,&[3]),"relations").len(),1);
 }
 #[test]fn decoder_versions_preserve_conditional_history(){
  let nodes=vec![Null,node(1,0x1000,0x54000050),node(2,0x1004,0xd503201f),node(3,0x1008,0xd65f03c0)];
  let legacy=derive_version(&nodes,"ELF declared VA",1,&||false).unwrap();assert_eq!(array(&legacy,"relations").len(),1);
  let current=derive(&nodes,"ELF declared VA",&||false).unwrap();let rows=array(&current,"relations");assert_eq!(rows.len(),3);assert_eq!(get(&rows[0],"target_node"),&Num("3".into()));assert_eq!(get(get(&rows[0],"operand"),"consistency_hint"),&Bool(true));assert_eq!(text(get(&rows[0],"operand"),"required_feature"),"FEAT_HBC");assert_eq!(text(&rows[1],"relation"),"candidate_not_taken_address");assert!(derive_version(&nodes,"ELF declared VA",4,&||false).is_err());
 }
 #[test]fn authenticated_reserved_encodings_and_older_revisions_stay_unclassified(){
  for word in [0xd61f0800,0xd63f0800,0xd65f0bfe]{assert!(control_flow::decode_version(word,3).is_none());}
  for word in [0xd71f0864,0xd61f08bf,0xd71f0cdf,0xd61f0cff,0xd73f0909,0xd63f095f,0xd73f0d7f,0xd63f0d9f,0xd65f0bff,0xd65f0fff]{assert!(control_flow::decode_version(word,1).is_none());assert!(control_flow::decode_version(word,2).is_none());assert!(matches!(control_flow::decode_version(word,3),Some(control_flow::Branch::Authenticated{..})));}
 }
 #[test]fn row_bounds_and_cancellation_preserve_explicit_omissions(){
  let mut nodes=vec![Null];for i in 1..=100{nodes.push(node(i,i*4,0x94000000));}
  let graph=derive(&nodes,"ELF declared VA",&||false).unwrap();assert_eq!(get(&graph,"observed_relations"),&Num("200".into()));let kept=array(&graph,"relations").len();assert!(kept<=48);assert_eq!(get(&graph,"omitted_relations"),&Num((200-kept).to_string()));assert!(super::super::super::mcp_schema::state_bytes(&graph)<16*1024);
  assert!(derive(&nodes,"ELF declared VA",&||true).unwrap_err().contains("cancelled"));
 }
 #[test]fn duplicate_coordinates_and_constant_zero_registers_remain_explicit(){
  let nodes=vec![Null,node(1,0x1000,0x14000001),node(2,0x1004,0xd503201f),node(3,0x1004,0xd65f03c0)];
  let graph=derive(&nodes,"ELF declared VA",&||false).unwrap();let row=&array(&graph,"relations")[0];assert_eq!(text(row,"resolution"),"ambiguous retained coordinate");assert_eq!(get(row,"target_node"),&Null);
  let nodes=vec![Null,node(1,0x1000,0xb400005f),node(2,0x1004,0xb500003f)];
  let graph=derive(&nodes,"ELF declared VA",&||false).unwrap();let rows=array(&graph,"relations");assert_eq!(rows.len(),3);assert_eq!(text(&rows[2],"relation"),"candidate_not_taken_address");
 }
}
