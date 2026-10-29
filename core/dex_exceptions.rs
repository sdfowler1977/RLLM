//! Original bounded DEX encoded exception relationships; never executes code.
use super::*;
use std::collections::BTreeMap;
pub(super) struct Exceptions{pub tries:usize,pub handlers:usize,pub edges:usize,pub rows:Vec<String>,pub abbreviated:bool}
struct Target{ty:Option<usize>,address:usize}
fn check(cancelled:&dyn Fn()->bool)->Result<()>{if cancelled(){Err("DEX exception observation cancelled".into())}else{Ok(())}}
pub(super) fn observe(r:&mut Reader<'_>,code:usize,units:usize,insns:&instructions::Code,kinds:&[Kind],type_name:&dyn Fn(usize)->String,cancelled:&dyn Fn()->bool)->Result<Exceptions>{
 check(cancelled)?;let tries=r.u16(code+6)?as usize;let mut out=Exceptions{tries,handlers:0,edges:0,rows:vec![],abbreviated:false};if tries==0{return Ok(out);}if tries>4096{return Err("DEX exception try count exceeds 4096 inspection bound".into());}
 let mut at=code.checked_add(16).and_then(|v|units.checked_mul(2).and_then(|n|v.checked_add(n))).ok_or("DEX exception offset overflow")?;
 if units%2!=0{r.data_range(at,2,1)?;if r.u16(at)?!=0{return Err("DEX exception padding is nonzero".into());}at+=2;}
 r.data_range(at,tries*8,4)?;let list=at.checked_add(tries*8).ok_or("DEX exception list offset overflow")?;let mut cursor=list;let count=r.leb(&mut cursor)?as usize;if count==0||count>4096{return Err("DEX encoded handler list count is empty or exceeds 4096 inspection bound".into());}
 let mut handlers=BTreeMap::new();let mut target_budget=4096usize;
 for _ in 0..count{check(cancelled)?;let offset=cursor-list;let size=r.sleb(&mut cursor)?;let n=size.unsigned_abs()as usize;let total=n.checked_add(usize::from(size<=0)).ok_or("DEX catch count overflow")?;target_budget=target_budget.checked_sub(total).ok_or("DEX exception targets exceed 4096 inspection bound")?;let mut targets=Vec::with_capacity(total);
  for _ in 0..n{check(cancelled)?;let ty=index(r.leb(&mut cursor)?,kinds.len(),"exception type")?;if kinds[ty]!=Kind::Class{return Err("DEX catch type is not a class descriptor".into());}let address=r.leb(&mut cursor)?as usize;if !insns.is_instruction(address){return Err("DEX typed exception destination is not an instruction boundary".into());}targets.push(Target{ty:Some(ty),address});}
  if size<=0{check(cancelled)?;let address=r.leb(&mut cursor)?as usize;if !insns.is_instruction(address){return Err("DEX catch-all destination is not an instruction boundary".into());}targets.push(Target{ty:None,address});}
  handlers.insert(offset,targets);
 }
 out.handlers=count;let mut previous_end=0;
 for i in 0..tries{check(cancelled)?;let item=at+i*8;let start=r.u32(item)?as usize;let length=r.u16(item+4)?as usize;let end=start.checked_add(length).ok_or("DEX protected range overflow")?;let offset=r.u16(item+6)?as usize;
  if length==0||start<previous_end||end>units||!insns.is_instruction(start)||!insns.is_boundary(end){return Err("DEX protected ranges are empty, overlapping, unordered or not on code-unit boundaries".into());}previous_end=end;
  let targets=handlers.get(&offset).ok_or("DEX try handler offset is not an encoded handler start")?;
  for (order,target)in targets.iter().enumerate(){check(cancelled)?;r.debit(1)?;out.edges=out.edges.checked_add(1).ok_or("DEX declared exception edge count overflow")?;
   if out.rows.len()<32{let catch=match target.ty{Some(ty)=>format!("type#{ty} {}",type_name(ty)),None=>"catch-all".into()};out.rows.push(format!("Declared exception edge protected units {start}..{end} (end exclusive) -> handler unit {}; catch order#{order} {catch}; encoded handler byte offset {offset}; not runtime reachability",target.address));}else{out.abbreviated=true;}
  }
 }
 check(cancelled)?;Ok(out)
}
