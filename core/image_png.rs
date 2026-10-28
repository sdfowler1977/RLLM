//! Original bounded static PNG sample decoder. Color transforms and animation
//! playback are not implemented; sample precision and straight alpha are retained.
use std::fmt;
#[derive(Clone,Copy,Debug)]pub(super) struct Limits{pub input_bytes:usize,pub pixels:usize,pub decoded_bytes:usize,pub chunk_count:usize,pub work:usize}
impl Default for Limits{fn default()->Self{Self{input_bytes:8*1024*1024,pixels:4*1024*1024,decoded_bytes:64*1024*1024,chunk_count:4096,work:128*1024*1024}}}
#[derive(Debug,Clone,PartialEq,Eq)]pub(super) struct ImageError{pub code:&'static str,pub offset:usize}
impl fmt::Display for ImageError{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{write!(f,"{} at byte {}",self.code,self.offset)}}
type Result<T>=std::result::Result<T,ImageError>;
fn fail(code:&'static str,offset:usize)->ImageError{ImageError{code,offset}}
#[derive(Debug,Clone,PartialEq,Eq)]pub(super) struct Image{pub width:u32,pub height:u32,pub rgba:Vec<u16>,pub source_depth:u8,pub color_status:&'static str,pub animation_status:&'static str,pub notes_count:usize}
impl Image{pub fn rgba8(&self)->Result<Vec<u8>>{let mut out=Vec::new();out.try_reserve_exact(self.rgba.len()).map_err(|_|fail("png-allocation",0))?;out.extend(self.rgba.iter().map(|v|((u32::from(*v)+128)/257)as u8));Ok(out)}}
fn be32(b:&[u8])->u32{u32::from_be_bytes([b[0],b[1],b[2],b[3]])}
fn be16(b:&[u8])->u16{u16::from_be_bytes([b[0],b[1]])}
fn crc(parts:&[&[u8]])->u32{let mut c=0xffffffffu32;for p in parts{for b in *p{c^=u32::from(*b);for _ in 0..8{c=(c>>1)^if c&1!=0{0xedb88320}else{0};}}}!c}
fn spend(left:&mut usize,n:usize,at:usize)->Result<()>{if n>*left{*left=0;return Err(fail("png-work-bound",at));}*left-=n;Ok(())}
#[derive(Clone,Copy)]struct Pass{x:usize,y:usize,dx:usize,dy:usize,w:usize,h:usize,row:usize}
fn passes(w:usize,h:usize,bits:usize,interlace:u8)->Result<Vec<Pass>>{let specs:&[(usize,usize,usize,usize)]=if interlace==0{&[(0,0,1,1)]}else{&[(0,0,8,8),(4,0,8,8),(0,4,4,8),(2,0,4,4),(0,2,2,4),(1,0,2,2),(0,1,1,2)]};let mut out=vec![];for &(x,y,dx,dy)in specs{let pw=if w<=x{0}else{(w-x).div_ceil(dx)};let ph=if h<=y{0}else{(h-y).div_ceil(dy)};if pw==0||ph==0{continue;}let row=pw.checked_mul(bits).and_then(|v|v.checked_add(7)).ok_or(fail("png-dimension-overflow",0))?/8;out.push(Pass{x,y,dx,dy,w:pw,h:ph,row});}Ok(out)}
pub(super) fn decode_png(bytes:&[u8],limits:&Limits)->Result<Image>{let mut work=limits.work;decode_png_with_budget(bytes,limits,&mut work)}
pub(super) fn decode_png_with_budget(bytes:&[u8],limits:&Limits,remaining_work:&mut usize)->Result<Image>{
 let allowance=limits.work.min(*remaining_work);let mut local=allowance;let result=decode_inner(bytes,limits,&mut local);*remaining_work-=allowance-local;result
}
fn decode_inner(bytes:&[u8],limits:&Limits,work:&mut usize)->Result<Image>{
 let hard=Limits::default();if limits.input_bytes>hard.input_bytes||limits.pixels>hard.pixels||limits.decoded_bytes>hard.decoded_bytes||limits.chunk_count>hard.chunk_count||limits.work>hard.work{return Err(fail("png-limit-exceeds-maximum",0));}let input=limits.input_bytes;let pixels=limits.pixels;let memory=limits.decoded_bytes;let chunks=limits.chunk_count;
 if bytes.len()>input{return Err(fail("png-input-bound",0));}if bytes.get(..8)!=Some(b"\x89PNG\r\n\x1a\n"){return Err(fail("png-signature",0));}
 let(mut at,mut count)=(8usize,0usize);let(mut width,mut height,mut depth,mut color,mut interlace)=(0,0,0,0,0);let mut palette:Option<&[u8]>=None;let mut transparency:Option<&[u8]>=None;let mut parts=vec![];let mut ended=false;let mut data_ended=false;let mut color_metadata=false;let mut animated=false;let mut notes_count=0usize;
 while at<bytes.len(){count+=1;if count>chunks{return Err(fail("png-chunk-bound",at));}let header=bytes.get(at..at+8).ok_or(fail("png-truncated-chunk",at))?;let length=be32(header)as usize;if length>0x7fffffff{return Err(fail("png-invalid-chunk-length",at));}let end=at.checked_add(12).and_then(|v|v.checked_add(length)).ok_or(fail("png-chunk-overflow",at))?;let chunk=bytes.get(at..end).ok_or(fail("png-truncated-chunk",at))?;let tag=&chunk[4..8];let data=&chunk[8..8+length];if !tag.iter().all(u8::is_ascii_alphabetic)||tag[2]&32!=0{return Err(fail("png-invalid-chunk-type",at));}spend(work,(length+4).checked_mul(8).ok_or(fail("png-work-bound",at))?,at)?;if crc(&[tag,data])!=be32(&chunk[8+length..]){return Err(fail("png-crc-mismatch",at));}if count==1&&tag!=b"IHDR"{return Err(fail("png-header-not-first",at));}if !parts.is_empty()&&tag!=b"IDAT"{data_ended=true;}
 match tag{
 b"IHDR"=>{if count!=1||length!=13{return Err(fail("png-invalid-header",at));}width=be32(data);height=be32(&data[4..]);depth=data[8];color=data[9];interlace=data[12];let valid=match color{0=>[1,2,4,8,16].contains(&depth),2|4|6=>[8,16].contains(&depth),3=>[1,2,4,8].contains(&depth),_=>false};if width==0||height==0||width>0x7fffffff||height>0x7fffffff||!valid||data[10]!=0||data[11]!=0||interlace>1{return Err(fail("png-invalid-header-values",at));}if (width as usize).checked_mul(height as usize).filter(|n|*n<=pixels).is_none(){return Err(fail("png-pixel-bound",at));}}
 b"PLTE"=>{if palette.is_some()||!parts.is_empty()||transparency.is_some()||length==0||length%3!=0||length>768||color==0||color==4||color==3&&length/3>(1usize<<depth){return Err(fail("png-invalid-palette",at));}palette=Some(data);}
 b"tRNS"=>{if transparency.is_some()||!parts.is_empty(){return Err(fail("png-invalid-transparency-order",at));}let valid=match color{0=>length==2,2=>length==6,3=>palette.map(|p|length<=p.len()/3).unwrap_or(false),_=>false};if !valid{return Err(fail("png-invalid-transparency",at));}transparency=Some(data);}
 b"IDAT"=>{if data_ended||color==3&&palette.is_none(){return Err(fail("png-invalid-data-order",at));}parts.push(data);}
 b"IEND"=>{if length!=0||parts.is_empty()||end!=bytes.len(){return Err(fail("png-invalid-end",at));}ended=true;break;}
 b"acTL"=>{if animated||!parts.is_empty()||length!=8||be32(data)==0{return Err(fail("png-invalid-animation-header",at));}animated=true;notes_count+=1;}
 b"sRGB"|b"iCCP"|b"gAMA"|b"cHRM"|b"cICP"|b"mDCV"|b"cLLI"=>{color_metadata=true;notes_count+=1;}
 _=>{notes_count+=1;if tag[0]&32==0{return Err(fail("png-unknown-critical-chunk",at));}}
 }at=end;
 }
 if !ended{return Err(fail("png-missing-end",at));}let channels=match color{0|3=>1,2=>3,4=>2,6=>4,_=>unreachable!()};let bits=channels*depth as usize;let pass=passes(width as usize,height as usize,bits,interlace)?;let mut expected=0usize;for p in &pass{expected=expected.checked_add((p.row+1).checked_mul(p.h).ok_or(fail("png-dimension-overflow",0))?).ok_or(fail("png-dimension-overflow",0))?;}
 let samples=(width as usize).checked_mul(height as usize).and_then(|v|v.checked_mul(4)).ok_or(fail("png-dimension-overflow",0))?;if samples.checked_mul(3).and_then(|v|v.checked_add(expected)).and_then(|v|v.checked_add(pass.iter().map(|p|p.row).max().unwrap_or(0)*2)).filter(|v|*v<=memory).is_none(){return Err(fail("png-memory-bound",0));}
 // Reserve downstream pixel/filter work before allowing decompression work.
 spend(work,expected.checked_mul(2).and_then(|v|v.checked_add(samples*2)).ok_or(fail("png-work-bound",0))?,0)?;
 let raw=super::png_inflate::decode_zlib_exact_with_budget(&parts,expected,work).map_err(|_|fail("png-zlib-invalid-or-bound",0))?;
 if raw.len()!=expected{return Err(fail("png-decoded-size-mismatch",0));}let mut rgba=Vec::new();rgba.try_reserve_exact(samples).map_err(|_|fail("png-allocation",0))?;rgba.resize(samples,0);let bpp=bits.div_ceil(8);let mut cursor=0;
 for p in pass{let mut previous=zeroes(p.row)?;let mut row=zeroes(p.row)?;for y in 0..p.h{let filter=raw[cursor];cursor+=1;if filter>4{return Err(fail("png-invalid-filter",cursor-1));}for i in 0..p.row{let a=if i>=bpp{row[i-bpp]}else{0};let b=previous[i];let c=if i>=bpp{previous[i-bpp]}else{0};let prediction=match filter{0=>0,1=>a,2=>b,3=>((u16::from(a)+u16::from(b))/2)as u8,4=>paeth(a,b,c),_=>unreachable!()};row[i]=raw[cursor+i].wrapping_add(prediction);}cursor+=p.row;
 for x in 0..p.w{let mut sample=[0u16;4];for c in 0..channels{let index=x*channels+c;sample[c]=if depth==16{be16(&row[index*2..])}else if depth==8{u16::from(row[index])}else{let bit=index*depth as usize;u16::from((row[bit/8]>>(8-depth as usize-bit%8))&((1<<depth)-1))};}let max=(1u32<<depth)-1;let scale=|v:u16|->u16{((u32::from(v)*65535)/max)as u16};let pixel=match color{0=>{let g=scale(sample[0]);let a=if transparency.map(|t|u32::from(be16(t))&max==u32::from(sample[0])).unwrap_or(false){0}else{65535};[g,g,g,a]},2=>{let a=if transparency.map(|t|(0..3).all(|i|u32::from(be16(&t[i*2..]))&max==u32::from(sample[i]))).unwrap_or(false){0}else{65535};[scale(sample[0]),scale(sample[1]),scale(sample[2]),a]},3=>{let pal=palette.unwrap();let i=sample[0]as usize;if i>=pal.len()/3{return Err(fail("png-palette-index",cursor));}let a=transparency.and_then(|t|t.get(i)).copied().unwrap_or(255);[u16::from(pal[i*3])*257,u16::from(pal[i*3+1])*257,u16::from(pal[i*3+2])*257,u16::from(a)*257]},4=>{let g=scale(sample[0]);[g,g,g,scale(sample[1])]},6=>[scale(sample[0]),scale(sample[1]),scale(sample[2]),scale(sample[3])],_=>unreachable!()};let dst=((p.y+y*p.dy)*width as usize+p.x+x*p.dx)*4;rgba[dst..dst+4].copy_from_slice(&pixel);}
 std::mem::swap(&mut previous,&mut row);
 }}Ok(Image{width,height,rgba,source_depth:depth,color_status:if color_metadata{"encoded-samples-color-metadata-unapplied"}else{"encoded-samples-color-unspecified"},animation_status:if animated{"default-image-only-animation-not-played"}else{"static-image"},notes_count})
}
fn paeth(a:u8,b:u8,c:u8)->u8{let p=i32::from(a)+i32::from(b)-i32::from(c);let da=(p-i32::from(a)).abs();let db=(p-i32::from(b)).abs();let dc=(p-i32::from(c)).abs();if da<=db&&da<=dc{a}else if db<=dc{b}else{c}}

fn zeroes(n:usize)->Result<Vec<u8>>{let mut v=Vec::new();v.try_reserve_exact(n).map_err(|_|fail("png-allocation",0))?;v.resize(n,0);Ok(v)}
