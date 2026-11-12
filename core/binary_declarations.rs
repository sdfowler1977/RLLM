//! Bounded typed declarations from the same held binary observation.
use super::*;
const ROWS:usize=64;
const CAP:usize=32*1024;
fn n(v:u64)->Json{Num(v.to_string())}
fn maybe(v:Option<bool>)->Json{v.map(Bool).unwrap_or(Null)}
fn integer(v:&Json)->Result<u64>{match v{Num(v)=>v.parse().map_err(|_|"Invalid binary declaration integer".into()),_=>Err("Missing binary declaration integer".into())}}
fn bit(v:&Json)->Result<bool>{match v{Bool(v)=>Ok(*v),_=>Err("Invalid binary declaration flag".into())}}
fn optional_bit(v:&Json)->bool{matches!(v,Null|Bool(_))}
fn extent(address:u64,size:u64,coordinate:&str)->bool{address.checked_add(size).is_some_and(|end|coordinate!="PE RVA"||end<=1u64<<32)}

pub(super) fn trim_optional_export(bound:&mut Json,cap:usize){
 let mut changed=false;
 for projection in ["declarations","control_flow"]{
  if super::super::mcp_schema::state_bytes(bound)>cap{if let Obj(fields)=bound{if let Some(Obj(structure))=fields.get_mut("structure"){if structure.remove(projection).is_some(){changed=true;structure.insert("projection_note".into(),s("Optional typed projections omitted under the existing export bound; original nodes and contains relationships retained. Full observations remain queryable."));}}}}
 }
 if changed&&super::super::mcp_schema::state_bytes(bound)>cap{if let Obj(fields)=bound{if let Some(Obj(structure))=fields.get_mut("structure"){structure.remove("projection_note");}}}
}

fn mapping(index:usize,r:&control_flow::Region,total:u64,sampled:u64,coordinate:&str)->Json{
 obj(vec![("index",n(index as u64)),("file_offset",n(r.offset)),("address",n(r.address)),("file_size",n(r.file_size)),("memory_size",n(r.memory_size)),("executable_declaration",Bool(r.executable)),("file_extent_valid",Bool(r.offset.checked_add(r.file_size).is_some_and(|end|end<=total))),("address_extent_valid",Bool(extent(r.address,r.memory_size,coordinate))),("sizes_consistent",Bool(r.file_size<=r.memory_size)),("captured_file_bytes",n(r.file_size.min(sampled.saturating_sub(r.offset))))])
}
fn classification(index:usize,r:&control_flow::Classification)->Json{
 obj(vec![("index",n(index as u64)),("section",n(r.section)),("start",n(r.start)),("end",n(r.end)),("code_declaration",Bool(r.code)),("mapping_agrees",Bool(r.mapping_agrees)),("contradicted",Bool(r.contradicted)),("excludes_whole_data_words",Bool(r.excludes_whole_data_words))])
}
fn relocation(index:usize,r:&control_flow::Relocation)->Json{
 let symbol=r.symbol.as_ref().map(|v|obj(vec![("name_display",s(&v.name_display)),("info",n(v.info as u64)),("binding",n((v.info>>4)as u64)),("type",n((v.info&15)as u64)),("section",n(v.section as u64)),("value",n(v.value))])).unwrap_or(Null);
 obj(vec![("index",n(index as u64)),("section",n(r.section)),("record",n(r.record)),("file_offset",n(r.file_offset)),("position",n(r.position)),("position_kind",s(if r.section_relative{"ELF section-relative offset"}else{"ELF declared VA"})),("target_section",n(r.target_section as u64)),("code",n(r.code as u64)),("name",s(r.name)),("symbol_index",n(r.symbol_index)),("symbol",symbol),("addend",r.addend.map(|v|Num(v.to_string())).unwrap_or(Null)),("addend_kind",s(if r.addend.is_some(){"explicit signed declaration"}else{"implicit; not decoded"}))])
}
fn pe_relocation(index:usize,r:&control_flow::PeRelocation)->Json{
 let(name,width)=pe_observation::relocation_type(r.code);let target=(r.page as u64).checked_add(r.offset as u64).filter(|v|r.code!=0&&*v<=u32::MAX as u64);
 obj(vec![("index",n(index as u64)),("format",s("PE base relocation")),("block",n(r.block as u64)),("slot",n(r.slot as u64)),("file_offset",n(r.file_offset)),("page_rva",n(r.page as u64)),("page_aligned",Bool(r.page%4096==0)),("offset",n(r.offset as u64)),("code",n(r.code as u64)),("name",s(name)),("record_complete",Bool(r.code!=4||r.adjustment.is_some())),("adjustment_word",r.adjustment.map(|v|n(v.0 as u64)).unwrap_or(Null)),("adjustment_file_offset",r.adjustment.map(|v|n(v.1)).unwrap_or(Null)),("target_rva",target.map(n).unwrap_or(Null)),("target_field_bytes",width.map(|v|n(v as u64)).unwrap_or(Null)),("target_file_offset",r.target_file_offset.map(n).unwrap_or(Null)),("meaning",s("Image base-relocation declaration with raw type/page/offset fields and captured target mapping. Coordinates are source RVAs. Padding and machine-dependent types retain their explicit classification."))])
}
fn validate_pe_relocation(row:&Json,sampled:u64)->Result<()>{
 let fail=||"Malformed PE base-relocation declaration".to_string();let code=integer(get(row,"code"))?;if code>15{return Err(fail());}let(name,width)=pe_observation::relocation_type(code as u8);
 let page=integer(get(row,"page_rva"))?;let offset=integer(get(row,"offset"))?;let at=integer(get(row,"file_offset"))?;
 if text(row,"format")!="PE base relocation"||integer(get(row,"block"))?>=32||integer(get(row,"slot"))?>=128||page>u32::MAX as u64||offset>4095||at.checked_add(2).is_none_or(|end|end>sampled)||text(row,"name")!=name||bit(get(row,"page_aligned"))?!=(page%4096==0){return Err(fail());}
 let adjustment=get(row,"adjustment_word");let adjustment_at=get(row,"adjustment_file_offset");let has_adjustment=adjustment!=&Null;
 if has_adjustment{if code!=4||integer(adjustment)?>u16::MAX as u64||integer(adjustment_at)?.checked_add(2).is_none_or(|end|end>sampled)||integer(get(row,"slot"))?>=127{return Err(fail());}}else if adjustment_at!=&Null{return Err(fail());}
 if bit(get(row,"record_complete"))?!=(code!=4||has_adjustment)||get(row,"target_field_bytes")!=&width.map(|v|n(v as u64)).unwrap_or(Null){return Err(fail());}
 let target=page.checked_add(offset).filter(|v|code!=0&&*v<=u32::MAX as u64);if get(row,"target_rva")!=&target.map(n).unwrap_or(Null){return Err(fail());}
 if get(row,"target_file_offset")!=&Null{let Some(width)=width else{return Err(fail());};if page%4096!=0||target.is_none()||integer(get(row,"target_file_offset"))?.checked_add(width as u64).is_none_or(|end|end>sampled){return Err(fail());}}
 if !matches!(get(row,"meaning"),Str(v)if v.len()<=512){return Err(fail());}Ok(())
}
fn xref_row(index:usize,xrefs:&control_flow::CrossReferences,coordinate:&str)->Json{
 let edges=&xrefs.edges;
 if index<edges.len(){let edge=&edges[index];obj(vec![("index",n(index as u64)),("kind",s(edge.kind.name())),("coordinate_kind",s(coordinate)),("source",n(edge.from_pc)),("target",n(edge.to_pc)),("resolved",Bool(true)),("reason",Null),("region",s(&edge.region))])}
 else{let unknown=&xrefs.unknowns[index-edges.len()];obj(vec![("index",n(index as u64)),("kind",s(unknown.kind.name())),("coordinate_kind",s(coordinate)),("source",n(unknown.from_pc)),("target",Null),("resolved",Bool(false)),("reason",s(unknown.reason)),("region",Null)])}
}
fn validate_cross_reference(row:&Json,coordinate:&str)->Result<()>{
 let fail=||"Malformed binary cross-reference declaration".to_string();let kind=text(row,"kind");
 if !["branch","call","conditional_branch","return"].contains(&kind.as_str())||text(row,"coordinate_kind")!=coordinate{return Err(fail());}
 let source=integer(get(row,"source"))?;if coordinate=="PE RVA"&&source>u32::MAX as u64{return Err(fail());}
 let resolved=bit(get(row,"resolved"))?;
 if resolved{let target=integer(get(row,"target"))?;if coordinate=="PE RVA"&&target>u32::MAX as u64||get(row,"reason")!=&Null||!matches!(get(row,"region"),Str(v) if !v.is_empty()&&v.len()<=512){return Err(fail());}}
 else if get(row,"target")!=&Null||get(row,"region")!=&Null||!["authenticated indirect target unresolved (runtime authentication context unobserved)","indirect branch unresolved (register state unobserved)","indirect call unresolved (register state unobserved)","return target unresolved (register state unobserved)","target unresolved (displacement and coordinate arithmetic disagree)","target unresolved (address arithmetic overflow)"].contains(&text(row,"reason").as_str()){return Err(fail());}
 Ok(())
}
pub(super) fn derive(observation:&control_flow::Observation,total:u64,sampled:u64,coordinate:&str,budget:usize,cancelled:&dyn Fn()->bool)->Result<Json>{
 let budget=budget.min(CAP);let mut cost=1024usize;let mut groups=[Vec::new(),Vec::new(),Vec::new(),Vec::new()];
 let relocation_count=if coordinate=="PE RVA"{observation.pe_relocations.len()}else{observation.relocations.len()};
 let xref_count=observation.xrefs.edges.len()+observation.xrefs.unknowns.len();
 for (group,count) in [(0,observation.regions.len()),(1,observation.classifications.len()),(2,relocation_count),(3,xref_count)]{
  for i in 0..count.min(ROWS){
   if cancelled(){return Err("Binary declaration projection cancelled".into());}
   let row=match group{0=>mapping(i,&observation.regions[i],total,sampled,coordinate),1=>classification(i,&observation.classifications[i]),2=>if coordinate=="PE RVA"{pe_relocation(i,&observation.pe_relocations[i])}else{relocation(i,&observation.relocations[i])},_=>xref_row(i,&observation.xrefs,coordinate)};
   let size=super::super::mcp_schema::state_bytes(&row)+1;
   if groups[group].len()<ROWS&&cost.saturating_add(size)<=budget{cost+=size;groups[group].push(row);}
  }
 }
 let counts=[observation.regions.len(),observation.classifications.len(),relocation_count,xref_count];
 let mut out=obj(vec![("schema",n(1)),("kind",s("binary_declarations")),("coordinate_kind",s(coordinate)),("coverage",s("Partial")),("mapping_inventory_complete",Bool(observation.mappings_complete)),("classification_inventory_complete",maybe(observation.classifications_complete)),("relocation_inventory_complete",maybe(observation.relocations_complete)),("cross_reference_inventory_complete",maybe(observation.xrefs_complete)),("meaning",s("Declarations from the held source: mappings, code/data labels and relocation records. Inventory and retained/omitted counts describe observed coverage. Symbol names use bounded escaped display text."))]);
 for (i,name) in ["mappings","classifications","relocations","cross_references"].iter().enumerate(){let kept=groups[i].len();put(&mut out,name,obj(vec![("observed",n(counts[i]as u64)),("retained",n(kept as u64)),("omitted",n((counts[i]-kept)as u64)),("rows",Arr(std::mem::take(&mut groups[i])))]));}
 if super::super::mcp_schema::state_bytes(&out)>budget{return Ok(Null);}validate(&out,total,sampled,coordinate)?;Ok(out)
}
pub(super) fn validate(v:&Json,total:u64,sampled:u64,coordinate:&str)->Result<()>{
 let fail=||"Malformed binary declarations; textual observation remains available".to_string();
 if get(v,"schema")!=&n(1)||text(v,"kind")!="binary_declarations"||text(v,"coordinate_kind")!=coordinate||text(v,"coverage")!="Partial"||super::super::mcp_schema::state_bytes(v)>CAP||!optional_bit(get(v,"classification_inventory_complete"))||!optional_bit(get(v,"relocation_inventory_complete"))||!optional_bit(get(v,"cross_reference_inventory_complete"))||!matches!(get(v,"meaning"),Str(s0)if s0.len()<=512){return Err(fail());}
 bit(get(v,"mapping_inventory_complete"))?;
 if coordinate=="PE RVA"&&(get(v,"classification_inventory_complete")!=&Null||get(get(v,"classifications"),"observed")!=&n(0)){return Err(fail());}
 for (name,limit) in [("mappings",97u64),("classifications",2048),("relocations",128),("cross_references",128)]{
  let group=get(v,name);let observed=integer(get(group,"observed"))?;let kept=integer(get(group,"retained"))?;let omitted=integer(get(group,"omitted"))?;let rows=get(group,"rows").array()?;
  if observed>limit||kept!=rows.len()as u64||rows.len()>ROWS||kept>observed||omitted!=observed-kept{return Err(fail());}
  let mut previous=None;
  for row in rows{
   let index=integer(get(row,"index"))?;if index>=observed||previous.is_some_and(|p|p>=index){return Err(fail());}previous=Some(index);
   match name{
    "mappings"=>{
     let offset=integer(get(row,"file_offset"))?;let address=integer(get(row,"address"))?;let file=integer(get(row,"file_size"))?;let memory=integer(get(row,"memory_size"))?;
     bit(get(row,"executable_declaration"))?;
     if bit(get(row,"file_extent_valid"))?!=offset.checked_add(file).is_some_and(|end|end<=total)||bit(get(row,"address_extent_valid"))?!=extent(address,memory,coordinate)||bit(get(row,"sizes_consistent"))?!=(file<=memory)||integer(get(row,"captured_file_bytes"))?!=file.min(sampled.saturating_sub(offset)){return Err(fail());}
    },
    "classifications"=>{
     let start=integer(get(row,"start"))?;let end=integer(get(row,"end"))?;if coordinate!="ELF declared VA"||integer(get(row,"section"))?>=64||start>=end{return Err(fail());}
     let code=bit(get(row,"code_declaration"))?;let agrees=bit(get(row,"mapping_agrees"))?;let contradicted=bit(get(row,"contradicted"))?;
     if bit(get(row,"excludes_whole_data_words"))?!=(!code&&agrees&&!contradicted){return Err(fail());}
    },
    "cross_references"=>validate_cross_reference(row,coordinate)?,
    _ if coordinate=="PE RVA"=>{validate_pe_relocation(row,sampled)?;},
    _=>{
     let at=integer(get(row,"file_offset"))?;let explicit=match get(row,"addend"){Null=>false,Num(v)=>{v.parse::<i64>().map_err(|_|fail())?;true},_=>return Err(fail())};
     if coordinate!="ELF declared VA"||integer(get(row,"section"))?>=64||integer(get(row,"record"))?>=128||at.checked_add(if explicit{24}else{16}).is_none_or(|end|end>sampled)||!["ELF declared VA","ELF section-relative offset"].contains(&text(row,"position_kind").as_str())||integer(get(row,"target_section"))?>u32::MAX as u64||integer(get(row,"code"))?>u32::MAX as u64||integer(get(row,"symbol_index"))?>u32::MAX as u64||text(row,"name").len()>64||text(row,"addend_kind")!=if explicit{"explicit signed declaration"}else{"implicit; not decoded"}{return Err(fail());}
     integer(get(row,"position"))?;let symbol=get(row,"symbol");if symbol!=&Null{let info=integer(get(symbol,"info"))?;if info>255||integer(get(symbol,"binding"))?!=info>>4||integer(get(symbol,"type"))?!=info&15||integer(get(symbol,"section"))?>u16::MAX as u64||!matches!(get(symbol,"name_display"),Str(s0) if s0.len()<=131){return Err(fail());}integer(get(symbol,"value"))?;}
    }
   }
  }
 }Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn prefix_coverage_does_not_become_complete_mapping_coverage(){
  let observation=control_flow::Observation{regions:vec![control_flow::Region{offset:64,address:4096,file_size:80,memory_size:128,executable:true}],mappings_complete:true,..Default::default()};
  let value=derive(&observation,200,100,"ELF declared VA",4096,&||false).unwrap();let row=&array(get(&value,"mappings"),"rows")[0];assert_eq!(get(row,"captured_file_bytes"),&n(36));assert_eq!(get(row,"file_extent_valid"),&Bool(true));
  let mut bad=value.clone();if let Obj(m)=&mut bad{if let Some(Obj(g))=m.get_mut("mappings"){if let Some(Arr(rows))=g.get_mut("rows"){put(&mut rows[0],"captured_file_bytes",n(80));}}}assert!(validate(&bad,200,100,"ELF declared VA").is_err());
 }
 #[test]fn omission_and_cancellation_remain_explicit_and_local(){
  let observation=control_flow::Observation{regions:(0..97).map(|i|control_flow::Region{offset:i*4,address:i*4,file_size:4,memory_size:4,executable:false}).collect(),..Default::default()};
  let value=derive(&observation,400,400,"PE RVA",4096,&||false).unwrap();let group=get(&value,"mappings");assert_eq!(get(group,"observed"),&n(97));assert!(integer(get(group,"retained")).unwrap()<97);assert_eq!(integer(get(group,"retained")).unwrap()+integer(get(group,"omitted")).unwrap(),97);assert_eq!(derive(&observation,400,400,"PE RVA",1,&||false).unwrap(),Null);assert!(derive(&observation,400,400,"PE RVA",4096,&||true).is_err());
 }
 #[test]fn overflow_does_not_wrap_into_a_valid_mapping(){
  let observation=control_flow::Observation{regions:vec![control_flow::Region{offset:u64::MAX,address:0xfffffff0,file_size:64,memory_size:64,executable:true}],..Default::default()};
  let value=derive(&observation,100,100,"PE RVA",4096,&||false).unwrap();let row=&array(get(&value,"mappings"),"rows")[0];assert_eq!(get(row,"file_extent_valid"),&Bool(false));assert_eq!(get(row,"address_extent_valid"),&Bool(false));assert_eq!(get(row,"captured_file_bytes"),&n(0));
 }
 #[test]fn relocation_addends_and_unknown_implementations_are_not_invented(){
  let r=control_flow::Relocation{section:1,record:0,file_offset:16,position:4096,section_relative:false,target_section:2,code:283,name:"CALL26",symbol_index:3,symbol:None,addend:Some(-8)};
  let observation=control_flow::Observation{relocations:vec![r],relocations_complete:Some(false),..Default::default()};let value=derive(&observation,100,100,"ELF declared VA",4096,&||false).unwrap();let row=&array(get(&value,"relocations"),"rows")[0];assert_eq!(get(row,"addend"),&Num("-8".into()));assert_eq!(get(row,"symbol"),&Null);assert!(validate(&value,100,20,"ELF declared VA").is_err());
  let mut pe=derive(&Default::default(),100,100,"PE RVA",4096,&||false).unwrap();put(&mut pe,"relocation_inventory_complete",Bool(true));assert!(validate(&pe,100,100,"PE RVA").is_ok());put(&mut pe,"classification_inventory_complete",Bool(true));assert!(validate(&pe,100,100,"PE RVA").is_err());
 }
 fn region()->control_flow::Region{control_flow::Region{offset:0,address:0x1000,file_size:0x100,memory_size:0x100,executable:true}}
 #[test]fn resolved_call_edge_is_a_typed_cross_reference_row(){
  let mut data=vec![0u8;16];data[..4].copy_from_slice(&0x94000002u32.to_le_bytes());
  let observation=control_flow::observe(&data,&[region()],true,"declared VA ",&||false,&[]).unwrap();
  let value=derive(&observation,0x100,16,"ELF declared VA",4096,&||false).unwrap();
  assert_eq!(text(&value,"coverage"),"Partial");assert_eq!(get(&value,"cross_reference_inventory_complete"),&Bool(true));
  let group=get(&value,"cross_references");assert_eq!(get(group,"observed"),&n(1));assert_eq!(get(group,"retained"),&n(1));assert_eq!(get(group,"omitted"),&n(0));
  let row=&array(group,"rows")[0];
  assert_eq!(row,&obj(vec![("index",n(0)),("kind",s("call")),("coordinate_kind",s("ELF declared VA")),("source",n(0x1000)),("target",n(0x1008)),("resolved",Bool(true)),("reason",Null),("region",s("region 0 address 0x1000 file offset 8 executable declaration"))]));
  assert!(validate(&value,0x100,16,"ELF declared VA").is_ok());assert!(derive(&observation,0x100,16,"ELF declared VA",4096,&||true).is_err());
 }
 #[test]fn unknown_cross_reference_keeps_its_reason(){
  let mut data=vec![0u8;4];data.copy_from_slice(&0xd73f0909u32.to_le_bytes());
  let observation=control_flow::observe(&data,&[region()],true,"declared VA ",&||false,&[]).unwrap();
  let value=derive(&observation,0x100,4,"ELF declared VA",4096,&||false).unwrap();
  let row=&array(get(&value,"cross_references"),"rows")[0];
  assert_eq!(get(row,"kind"),&s("call"));assert_eq!(get(row,"resolved"),&Bool(false));assert_eq!(get(row,"target"),&Null);assert_eq!(get(row,"region"),&Null);
  assert_eq!(text(row,"reason"),"authenticated indirect target unresolved (runtime authentication context unobserved)");
  assert_eq!(text(&value,"coverage"),"Partial");assert!(observation.xrefs.edges.is_empty());assert!(validate(&value,0x100,4,"ELF declared VA").is_ok());
 }
 #[test]fn cross_reference_rows_keep_the_rows_cap_and_omitted_count(){
  let edges=(0..ROWS+1).map(|i|control_flow::XrefEdge{from_pc:0x1000+i as u64*4,to_pc:0x2000+i as u64*4,kind:control_flow::XrefKind::Call,region:"outside every mapped region".into()}).collect();
  let observation=control_flow::Observation{xrefs:control_flow::CrossReferences{edges,..Default::default()},xrefs_complete:Some(true),..Default::default()};
  let value=derive(&observation,0x10000,0x10000,"ELF declared VA",CAP,&||false).unwrap();let group=get(&value,"cross_references");
  assert_eq!(get(group,"observed"),&n((ROWS+1)as u64));assert_eq!(get(group,"retained"),&n(ROWS as u64));assert_eq!(get(group,"omitted"),&n(1));assert_eq!(array(group,"rows").len(),ROWS);
  assert_eq!(get(&array(group,"rows")[ROWS-1],"source"),&n(0x1000+(ROWS as u64-1)*4));assert_eq!(get(&value,"cross_reference_inventory_complete"),&Bool(true));assert_eq!(text(&value,"coverage"),"Partial");
 }
 #[test]fn cross_reference_projection_returns_null_when_the_budget_is_too_small(){
  let observation=control_flow::Observation{xrefs:control_flow::CrossReferences{edges:vec![control_flow::XrefEdge{from_pc:0x1000,to_pc:0x1008,kind:control_flow::XrefKind::Call,region:"outside every mapped region".into()}],..Default::default()},xrefs_complete:Some(true),..Default::default()};
  let fitted=derive(&observation,0x100,0x100,"ELF declared VA",4096,&||false).unwrap();assert_eq!(get(get(&fitted,"cross_references"),"observed"),&n(1));assert_eq!(get(get(&fitted,"cross_references"),"retained"),&n(1));
  assert_eq!(derive(&observation,0x100,0x100,"ELF declared VA",1,&||false).unwrap(),Null);
  let size=super::super::super::mcp_schema::state_bytes(&fitted);let tight=derive(&observation,0x100,0x100,"ELF declared VA",size-1,&||false).unwrap();
  assert_eq!(get(get(&tight,"cross_references"),"observed"),&n(1));assert_eq!(get(get(&tight,"cross_references"),"retained"),&n(0));assert_eq!(get(get(&tight,"cross_references"),"omitted"),&n(1));
  let shell=super::super::super::mcp_schema::state_bytes(&tight);assert!(shell<size);assert_eq!(derive(&observation,0x100,0x100,"ELF declared VA",shell-1,&||false).unwrap(),Null);
 }
 #[test]fn invented_cross_reference_target_is_rejected(){
  let mut data=vec![0u8;4];data.copy_from_slice(&0xd73f0909u32.to_le_bytes());
  let observation=control_flow::observe(&data,&[region()],true,"declared VA ",&||false,&[]).unwrap();
  let value=derive(&observation,0x100,4,"ELF declared VA",4096,&||false).unwrap();assert!(validate(&value,0x100,4,"ELF declared VA").is_ok());
  let mut bad=value.clone();if let Obj(m)=&mut bad{if let Some(Obj(g))=m.get_mut("cross_references"){if let Some(Arr(rows))=g.get_mut("rows"){put(&mut rows[0],"target",n(0x1008));}}}
  assert_eq!(text(&array(get(&bad,"cross_references"),"rows")[0],"reason"),"authenticated indirect target unresolved (runtime authentication context unobserved)");assert!(validate(&bad,0x100,4,"ELF declared VA").is_err());
  let edge=control_flow::XrefEdge{from_pc:0x1000,to_pc:0x1008,kind:control_flow::XrefKind::Call,region:"region 0 address 0x1000 file offset 8 executable declaration".into()};
  let resolved=control_flow::Observation{xrefs:control_flow::CrossReferences{edges:vec![edge],..Default::default()},xrefs_complete:Some(true),mappings_complete:true,..Default::default()};
  let pe=derive(&resolved,0x100,0x100,"PE RVA",4096,&||false).unwrap();assert!(validate(&pe,0x100,0x100,"PE RVA").is_ok());
  let mut wide=pe.clone();if let Obj(m)=&mut wide{if let Some(Obj(g))=m.get_mut("cross_references"){if let Some(Arr(rows))=g.get_mut("rows"){put(&mut rows[0],"target",n(1u64<<32));}}}
  assert!(validate(&wide,0x100,0x100,"PE RVA").is_err());
  let elf=derive(&resolved,0x100,0x100,"ELF declared VA",4096,&||false).unwrap();let mut elf_wide=elf.clone();if let Obj(m)=&mut elf_wide{if let Some(Obj(g))=m.get_mut("cross_references"){if let Some(Arr(rows))=g.get_mut("rows"){put(&mut rows[0],"target",n(1u64<<32));}}}
  assert!(validate(&elf_wide,0x100,0x100,"ELF declared VA").is_ok());
 }
 #[test]fn optional_metadata_and_its_note_cannot_displace_a_fitting_export(){
  let original=obj(vec![("structure",obj(vec![("nodes",Arr(vec![s(&"x".repeat(1024))])),("edges",Arr(vec![]))]))]);let cap=super::super::super::mcp_schema::state_bytes(&original);let mut extended=original.clone();if let Obj(m)=&mut extended{if let Some(structure)=m.get_mut("structure"){put(structure,"declarations",s(&"d".repeat(4096)));put(structure,"control_flow",s(&"f".repeat(4096)));}}trim_optional_export(&mut extended,cap);assert_eq!(extended,original);
 }
}
