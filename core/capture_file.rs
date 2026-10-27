//! Workspace binding for the original static capture engine.
use super::*;
use std::os::unix::fs::{OpenOptionsExt,MetadataExt};
fn read_held(path:&Path,limit:usize)->Result<Vec<u8>>{
 let mut file=fs::OpenOptions::new().read(true).custom_flags(0x800|0x20000).open(path).map_err(|e|format!("Cannot open capture input: {e}"))?;
 let before=file.metadata().map_err(|e|e.to_string())?;if !before.is_file()||before.len()>limit as u64{return Err("Capture input must be a regular file within its size bound".into());}
 let mut bytes=Vec::with_capacity(before.len() as usize);let mut buffer=[0;65536];loop{if super::super::execution::cancelled(){return Err("Capture input read cancelled".into());}let n=file.read(&mut buffer).map_err(|e|e.to_string())?;if n==0{break;}if bytes.len()+n>limit{return Err("Capture input grew beyond its bound".into());}bytes.extend_from_slice(&buffer[..n]);}
 let after=file.metadata().map_err(|e|e.to_string())?;if before.dev()!=after.dev()||before.ino()!=after.ino()||before.len()!=after.len()||after.len()!=bytes.len() as u64||before.mtime()!=after.mtime()||before.mtime_nsec()!=after.mtime_nsec()||before.ctime()!=after.ctime()||before.ctime_nsec()!=after.ctime_nsec(){return Err("Capture input changed while reading".into());}Ok(bytes)
}
fn font_source(root:&Path,name:&str)->Result<(PathBuf,Vec<u8>)>{
 if !name.trim().is_empty(){let p=path(root,name)?;let bytes=read_held(&p,16*1024*1024)?;return Ok((p,bytes));}
 #[cfg(target_os="android")]
 {for name in ["/system/fonts/Roboto-Regular.ttf","/system/fonts/NotoSans-Regular.ttf","/system/fonts/DroidSans.ttf"]{if let Ok(p)=Path::new(name).canonicalize(){if let Ok(bytes)=read_held(&p,16*1024*1024){if font_outline::Face::parse(&bytes).is_ok(){return Ok((p,bytes));}}}}}
 Err("No supported platform TrueType face was available; select a workspace TrueType font for this capture".into())
}

pub(super) fn validate(args:&Json)->Result<()> {
 for key in ["font","output"]{if key=="font"&&(get(args,key)==&Null||text(args,key).trim().is_empty()){continue;}let value=get(args,key).text()?;if value.is_empty()||value.len()>1024||value.starts_with('/')||value.contains('\\')||value.contains('\0')||value.split('/').any(|p|p==".."||p.is_empty()){return Err(format!("Invalid capture {key} path"));}}
 let output=text(args,"output");if output.contains('/')||output=="."||!output.to_ascii_lowercase().ends_with(".png"){return Err("Capture output must be a new workspace-root PNG filename".into());}
 for key in ["width","height"]{if get(args,key)!=&Null {let v=get(args,key);if matches!(v,Str(s) if s.trim().is_empty()){continue;}let n=match v{Num(s)=>s.parse::<u32>().ok(),Str(s)=>s.parse::<u32>().ok(),_=>None}.ok_or("Capture dimensions must be positive integers")?;if n==0||n>2048{return Err("Capture dimensions must be 1 through 2048".into());}}}Ok(())
}
pub(super) fn capture(root:&Path,input:&Path,args:&Json)->Result<String>{
 validate(args)?;let name=text(args,"output");let output=root.join(&name);
 if output.exists(){return Err("Capture output already exists; choose a new PNG filename".into());}
 let html=read_held(input,262144)?;let (font_path,font)=font_source(root,&text(args,"font"))?;
 let dimension=|key:&str,default:u32|->u32{match get(args,key){Num(s)|Str(s)=>s.parse().unwrap_or(default),_=>default}};
 let result=page_capture::render(&html,&font,dimension("width",800),dimension("height",600),super::super::execution::cancelled)?;
 if super::super::execution::cancelled(){return Err("Capture cancelled before output creation".into());}
 let mut file=fs::OpenOptions::new().read(true).write(true).create_new(true).custom_flags(0x20000).open(&output).map_err(|e|format!("Cannot create capture output: {e}"))?;
 for block in result.png.chunks(65536){if super::super::execution::cancelled(){return Err(format!("Capture cancelled; incomplete new file {name} may remain"));}file.write_all(block).map_err(|e|format!("Capture output incomplete ({name}): {e}"))?;}
 file.sync_all().map_err(|e|format!("Capture output not verified ({name}): {e}"))?;
 use std::io::{Seek,SeekFrom};file.seek(SeekFrom::Start(0)).map_err(|e|e.to_string())?;let mut buffer=[0;65536];let mut offset=0usize;loop{let n=file.read(&mut buffer).map_err(|e|e.to_string())?;if n==0{break;}if offset+n>result.png.len()||result.png[offset..offset+n]!=buffer[..n]{return Err(format!("Capture output readback mismatch; new file {name} retained"));}offset+=n;}if offset!=result.png.len(){return Err(format!("Capture output readback truncated; new file {name} retained"));}
 let held=file.metadata().map_err(|e|e.to_string())?;let current=fs::symlink_metadata(&output).map_err(|e|e.to_string())?;if !current.is_file()||current.dev()!=held.dev()||current.ino()!=held.ino(){return Err(format!("Capture output name changed; created file {name} requires inspection"));}
 Ok(format!("{}\nHTML source: {} ({} bytes). Font source: {}. Saved new workspace PNG: {name}; all {} bytes read back exactly. Existing files were not replaced.",result.summary,input.display(),html.len(),font_path.display(),offset))
}
