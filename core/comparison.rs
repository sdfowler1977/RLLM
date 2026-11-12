//! Original byte comparison over held local files. No external executables.
use super::*;
const LIMIT:usize=1024*1024;
/// Capture the bounded byte sample of one reviewed workspace file. The opened
/// source is the reviewed file, and its binding is re-checked after the read.
fn sample(source:&file_evidence::Source<'_>,cancelled:&impl Fn()->bool)->Result<(Vec<u8>,u64,bool)>{
 let value=sample_file(source.file()?,cancelled)?;source.check()?;Ok(value)
}
fn sample_file(mut f:fs::File,cancelled:&impl Fn()->bool)->Result<(Vec<u8>,u64,bool)>{
 let before=f.metadata().map_err(|e|e.to_string())?;
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
/// Compare two captured file samples and describe the observed byte differences.
/// The comparison is positional, not semantic: an insertion may shift later
/// byte positions. Each sample is read independently; the pair is not an
/// atomic filesystem snapshot.
pub(super) fn compare(left:&Path,right:&Path,a:Vec<u8>,alen:u64,ac:bool,b:Vec<u8>,blen:u64,bc:bool,cancelled:&impl Fn()->bool)->Result<String>{
 let overlap=a.len().min(b.len());let mut changed=0;let mut locations=Vec::new();
 for i in 0..overlap {if i%65536==0&&cancelled(){return Err("Comparison cancelled".into());}if a[i]!=b[i]{changed+=1;if locations.len()<32{locations.push(format!("0x{i:x}"));}}}
 if cancelled(){return Err("Comparison cancelled before publication".into());}
 Ok(describe(left,right,&a,&b,alen,blen,ac,bc,overlap,changed,&locations))
}
/// Compare two reviewed workspace files and describe the observed bytes.
pub(super) fn compare_using(left:&Path,right:&Path,cancelled:&impl Fn()->bool,scope:(&Path,&Json,&operation::InspectionAccess))->Result<String>{
 let (root,args,access)=scope;
 let a=file_evidence::Source::open(root,&text(args,"path"),"compare",args,access)?;
 let b=file_evidence::Source::open(root,&text(args,"other"),"compare",args,access)?;
 let (av,alen,ac)=sample(&a,cancelled)?;a.check()?;b.check()?;
 let (bv,blen,bc)=sample(&b,cancelled)?;a.check()?;b.check()?;
 let output=compare(left,right,av,alen,ac,bv,blen,bc,cancelled)?;
 a.check()?;b.check()?;Ok(output)
}
pub(super) fn compare_scoped(root:&Path,left:&Path,right:&Path,args:&Json,access:&operation::InspectionAccess)->Result<String>{
 let cancelled=||access.cancelled();
 compare_using(left,right,&cancelled,(root,args,access))
}
/// Format the observed byte comparison of two captured samples.
fn describe(left:&Path,right:&Path,a:&[u8],b:&[u8],alen:u64,blen:u64,ac:bool,bc:bool,overlap:usize,changed:usize,locations:&[String])->String{
 let conclusion=if alen!=blen||changed>0{"Different bytes observed"}else if ac&&bc{"Complete captured contents are byte-identical"}else{"Compared prefixes match; whole-file equality is unknown"};
 format!("Local byte comparison\nLeft: {}\nRight: {}\nObserved file lengths: {alen} and {blen} bytes\nCaptured: {} and {} bytes; complete captures: {ac} and {bc}\n{conclusion}\nCompared common prefix: {overlap} bytes\nDiffering positions within common prefix: {changed}\nFirst differing offsets (at most 32): {}\nCaptured bytes outside common prefix: left {}, right {}\nComparison is positional, not semantic; an insertion may shift subsequent byte positions. Each file is read independently; the pair is not an atomic filesystem snapshot.",left.display(),right.display(),a.len(),b.len(),locations.join(", "),a.len()-overlap,b.len()-overlap)
}
#[cfg(test)]mod checks{use super::*;
 fn sample_path(path:&Path)->(Vec<u8>,u64,bool){use std::os::unix::fs::OpenOptionsExt;let f=fs::OpenOptions::new().read(true).custom_flags(0x20000|0x800).open(path).unwrap();sample_file(f,&||false).unwrap()}
 fn pair(a:&Path,b:&Path)->String{let(av,alen,ac)=sample_path(a);let(bv,blen,bc)=sample_path(b);compare(a,b,av,alen,ac,bv,blen,bc,&||false).unwrap()}
 #[test]fn positional_comparison_distinguishes_full_and_prefix(){
  let root=std::env::temp_dir().join(format!("rllm-compare-{}-{}",std::process::id(),now()));fs::create_dir_all(&root).unwrap();let a=root.join("a");let b=root.join("b");
  fs::write(&a,b"hello").unwrap();fs::write(&b,b"hello").unwrap();assert!(pair(&a,&b).contains("byte-identical"));
  fs::write(&b,b"hallo!").unwrap();let r=pair(&a,&b);assert!(r.contains("Different bytes observed"));assert!(r.contains("0x1"));assert!(r.contains("right 1"));
  fs::write(&a,vec![0;LIMIT+2]).unwrap();let mut data=vec![0;LIMIT+2];data[LIMIT+1]=1;fs::write(&b,data).unwrap();let r=pair(&a,&b);assert!(r.contains("whole-file equality is unknown"));assert!(!r.contains("byte-identical"));
  // Test inputs intentionally retained for reproducibility.
 }
}
