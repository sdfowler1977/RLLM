//! Original byte comparison over held local files. No external executables.
use super::*;
const LIMIT:usize=1024*1024;
fn sample(path:&Path,cancelled:&impl Fn()->bool)->Result<(Vec<u8>,u64,bool)>{
 let mut f=fs::File::open(path).map_err(|e|e.to_string())?;let before=f.metadata().map_err(|e|e.to_string())?;
 if !before.is_file(){return Err("Comparison requires regular files".into());}
 let mut bytes=Vec::new();let mut block=[0u8;65536];
 while bytes.len()<=LIMIT {
  if cancelled(){return Err("Comparison cancelled".into());}
  let remaining=(LIMIT+1-bytes.len()).min(block.len());let n=f.read(&mut block[..remaining]).map_err(|e|e.to_string())?;
  if n==0{break;}bytes.extend_from_slice(&block[..n]);
 }
 let after=f.metadata().map_err(|e|e.to_string())?;
 if before.len()!=after.len()||before.modified().ok()!=after.modified().ok(){return Err("A comparison input changed during reading".into());}
 let complete=bytes.len()<=LIMIT;if complete&&bytes.len() as u64!=after.len(){return Err("Comparison input length changed during reading".into());}bytes.truncate(LIMIT);Ok((bytes,before.len(),complete))
}
pub(super) fn compare(left:&Path,right:&Path)->Result<String>{compare_using(left,right,&super::super::execution::cancelled)}
pub(super) fn compare_using(left:&Path,right:&Path,cancelled:&impl Fn()->bool)->Result<String>{
 let (a,alen,ac)=sample(left,cancelled)?;let(b,blen,bc)=sample(right,cancelled)?;
 let overlap=a.len().min(b.len());let mut changed=0;let mut locations=Vec::new();
 for i in 0..overlap {if i%65536==0&&cancelled(){return Err("Comparison cancelled".into());}if a[i]!=b[i]{changed+=1;if locations.len()<32{locations.push(format!("0x{i:x}"));}}}
 let conclusion=if alen!=blen||changed>0{"Different bytes observed"}else if ac&&bc{"Complete captured contents are byte-identical"}else{"Compared prefixes match; whole-file equality is unknown"};
 if cancelled(){return Err("Comparison cancelled before publication".into());}
 Ok(format!("Local byte comparison\nLeft: {}\nRight: {}\nObserved file lengths: {alen} and {blen} bytes\nCaptured: {} and {} bytes; complete captures: {ac} and {bc}\n{conclusion}\nCompared common prefix: {overlap} bytes\nDiffering positions within common prefix: {changed}\nFirst differing offsets (at most 32): {}\nCaptured bytes outside common prefix: left {}, right {}\nComparison is positional, not semantic; an insertion may shift subsequent byte positions. Each file is read independently; the pair is not an atomic filesystem snapshot.",left.display(),right.display(),a.len(),b.len(),locations.join(", "),a.len()-overlap,b.len()-overlap))
}
#[cfg(test)]mod checks{use super::*;
 #[test]fn positional_comparison_distinguishes_full_and_prefix(){
  let root=std::env::temp_dir().join(format!("rllm-compare-{}-{}",std::process::id(),now()));fs::create_dir_all(&root).unwrap();let a=root.join("a");let b=root.join("b");
  fs::write(&a,b"hello").unwrap();fs::write(&b,b"hello").unwrap();assert!(compare(&a,&b).unwrap().contains("byte-identical"));
  fs::write(&b,b"hallo!").unwrap();let r=compare(&a,&b).unwrap();assert!(r.contains("Different bytes observed"));assert!(r.contains("0x1"));assert!(r.contains("right 1"));
  fs::write(&a,vec![0;LIMIT+2]).unwrap();let mut data=vec![0;LIMIT+2];data[LIMIT+1]=1;fs::write(&b,data).unwrap();let r=compare(&a,&b).unwrap();assert!(r.contains("whole-file equality is unknown"));assert!(!r.contains("byte-identical"));
  // Test inputs intentionally retained for reproducibility.
 }
}
