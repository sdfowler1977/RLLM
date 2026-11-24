//! Original bounded TrueType outline/metric reader. No hint interpreter,
//! shaping, kerning, variation processing, font download or rasterizer.
use std::fmt;
const FONT_BYTES:usize=16*1024*1024;const POINTS:usize=65536;const CONTOURS:usize=4096;const COMPONENTS:usize=1024;const DEPTH:usize=32;
#[derive(Clone,Debug,PartialEq,Eq)]pub(super) struct FontError{pub code:&'static str,pub offset:usize}
impl fmt::Display for FontError{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{write!(f,"{} at byte {}",self.code,self.offset)}}
type Result<T>=std::result::Result<T,FontError>;
fn err(code:&'static str,offset:usize)->FontError{FontError{code,offset}}
fn range(data:&[u8],at:usize,len:usize)->Result<&[u8]>{data.get(at..at.checked_add(len).ok_or(err("font-offset-overflow",at))?).ok_or(err("font-data-truncated",at))}
fn u16at(data:&[u8],at:usize)->Result<u16>{let b=range(data,at,2)?;Ok(u16::from_be_bytes([b[0],b[1]]))}
fn i16at(data:&[u8],at:usize)->Result<i16>{Ok(u16at(data,at)? as i16)}
fn u32at(data:&[u8],at:usize)->Result<u32>{let b=range(data,at,4)?;Ok(u32::from_be_bytes([b[0],b[1],b[2],b[3]]))}
struct Reader<'a>{bytes:&'a[u8],at:usize}
impl<'a>Reader<'a>{fn byte(&mut self)->Result<u8>{let v=*self.bytes.get(self.at).ok_or(err("glyph-data-truncated",self.at))?;self.at+=1;Ok(v)}fn word(&mut self)->Result<u16>{let n=u16at(self.bytes,self.at)?;self.at+=2;Ok(n)}fn signed(&mut self)->Result<i16>{Ok(self.word()?as i16)}fn skip(&mut self,n:usize)->Result<()>{range(self.bytes,self.at,n)?;self.at+=n;Ok(())}fn scale(&mut self)->Result<f64>{Ok(f64::from(self.signed()?)/16384.0)}}
#[derive(Clone,Copy,Debug,PartialEq)]pub(super) struct Point{pub x:f64,pub y:f64,pub on_curve:bool}
#[derive(Clone,Debug,PartialEq)]pub(super) struct Outline{pub glyph_id:u16,pub advance:f64,pub left_bearing:f64,pub origin_x:f64,pub units_per_em:u16,pub contours:Vec<Vec<Point>>,pub unhinted:bool}
#[derive(Clone,Debug)]pub(super) struct Face<'a>{pub units_per_em:u16,pub num_glyphs:u16,pub ascender:i16,pub descender:i16,pub line_gap:i16,glyf:&'a[u8],loca:&'a[u8],long_loca:bool,hmtx:&'a[u8],metrics:u16,cmap:&'a[u8],cmap_format:u16}
impl<'a>Face<'a>{
 pub fn parse(data:&'a[u8])->Result<Self>{
  if data.len()>FONT_BYTES{return Err(err("font-byte-bound",0));}if u32at(data,0)?!=0x00010000{return Err(err("unsupported-font-container-or-outline-format",0));}
  let count=u16at(data,4)?as usize;if count==0||count>128{return Err(err("font-table-count-bound",4));}range(data,12,count*16)?;
  let mut tables=Vec::new();for i in 0..count{let pos=12+i*16;let tag=range(data,pos,4)?;let offset=u32at(data,pos+8)?as usize;let length=u32at(data,pos+12)?as usize;
   if offset%4!=0||offset<12+count*16{return Err(err("invalid-font-table-offset",pos+8));}let bytes=range(data,offset,length)?;
   if tables.iter().any(|(t,_,_,_)|*t==tag){return Err(err("duplicate-font-table",pos));}
   if length>0&&tables.iter().any(|(_,o,l,_)|*l>0&&offset<*o+*l&&*o<offset+length){return Err(err("overlapping-font-tables",pos));}
   tables.push((tag,offset,length,bytes));
  }
  if tables.iter().any(|(tag,_,_,_)|*tag==b"fvar"||*tag==b"gvar"){return Err(err("variable-font-not-supported",0));}
  let table=|name:&[u8]|->Result<&'a[u8]>{tables.iter().find(|(t,_,_,_)|*t==name).map(|(_,_,_,b)|*b).ok_or(err("required-outline-table-missing",0))};
  let head=table(b"head")?;range(head,0,54)?;if u16at(head,0)?!=1||u32at(head,12)?!=0x5f0f3cf5||i16at(head,52)?!=0{return Err(err("unsupported-font-head",0));}
  let units_per_em=u16at(head,18)?;if !(16..=16384).contains(&units_per_em){return Err(err("invalid-units-per-em",18));}
  let long_loca=match i16at(head,50)?{0=>false,1=>true,_=>return Err(err("unsupported-location-format",50))};
  let maxp=table(b"maxp")?;if u32at(maxp,0)?!=0x00010000{return Err(err("unsupported-outline-maxp-version",0));}range(maxp,0,32)?;let num_glyphs=u16at(maxp,4)?;if num_glyphs==0{return Err(err("font-has-no-glyphs",4));}
  let hhea=table(b"hhea")?;range(hhea,0,36)?;if u32at(hhea,0)?!=0x00010000||i16at(hhea,32)?!=0{return Err(err("unsupported-horizontal-header",0));}let metrics=u16at(hhea,34)?;if metrics==0||metrics>num_glyphs{return Err(err("invalid-horizontal-metric-count",34));}
  let hmtx=table(b"hmtx")?;range(hmtx,0,metrics as usize*4+(num_glyphs-metrics)as usize*2)?;
  let glyf=table(b"glyf")?;let loca=table(b"loca")?;range(loca,0,(num_glyphs as usize+1)*if long_loca{4}else{2})?;let mut previous=0;
  for id in 0..=num_glyphs as usize{let offset=if long_loca{u32at(loca,id*4)?as usize}else{u16at(loca,id*2)?as usize*2};if offset<previous||offset>glyf.len(){return Err(err("invalid-glyph-location-range",id));}previous=offset;}
  let cmap_all=table(b"cmap")?;if u16at(cmap_all,0)?!=0{return Err(err("unsupported-cmap-version",0));}let n=u16at(cmap_all,2)?as usize;if n==0||n>128{return Err(err("cmap-record-bound",2));}range(cmap_all,4,n*8)?;
  let mut selected=None;for i in 0..n{let at=4+i*8;let platform=u16at(cmap_all,at)?;let encoding=u16at(cmap_all,at+2)?;let offset=u32at(cmap_all,at+4)?as usize;let format=u16at(cmap_all,offset)?;
   let unicode=platform==0&&encoding<=4||platform==3&&(encoding==1&&format==4||encoding==10&&format==12);if !unicode||![4,12].contains(&format){continue;}
   let length=if format==4{u16at(cmap_all,offset+2)?as usize}else{u32at(cmap_all,offset+4)?as usize};let cmap=range(cmap_all,offset,length)?;validate_cmap(cmap,format,num_glyphs)?;
   let score=(if format==12{4}else{0})+if platform==0{2}else{1};if selected.as_ref().map(|(old,_,_)|score>*old).unwrap_or(true){selected=Some((score,cmap,format));}
  }
  let(_,cmap,cmap_format)=selected.ok_or(err("supported-unicode-cmap-missing",0))?;
  Ok(Self{units_per_em,num_glyphs,ascender:i16at(hhea,4)?,descender:i16at(hhea,6)?,line_gap:i16at(hhea,8)?,glyf,loca,long_loca,hmtx,metrics,cmap,cmap_format})
 }
 pub fn glyph_id(&self,ch:char)->Result<u16>{let code=ch as u32;let id=if self.cmap_format==12{
  let n=u32at(self.cmap,12)?as usize;let mut lo=0;let mut hi=n;let mut id=0;while lo<hi{let mid=(lo+hi)/2;let at=16+mid*12;let start=u32at(self.cmap,at)?;let end=u32at(self.cmap,at+4)?;if code<start{hi=mid}else if code>end{lo=mid+1}else{id=u32at(self.cmap,at+8)?.checked_add(code-start).ok_or(err("cmap-glyph-overflow",at))?;break;}}id
 }else if code>65535{0}else{
  let n=u16at(self.cmap,6)?as usize/2;let end_base=14;let start_base=16+n*2;let delta_base=16+n*4;let offset_base=16+n*6;let mut id=0;
  for i in 0..n{let end=u16at(self.cmap,end_base+2*i)?as u32;if code>end{continue;}let start=u16at(self.cmap,start_base+2*i)?as u32;if code<start{break;}let delta=u16at(self.cmap,delta_base+2*i)?;let offset=u16at(self.cmap,offset_base+2*i)?as usize;
   id=if offset==0{(code as u16).wrapping_add(delta)as u32}else{let word=offset_base+2*i+offset+(code-start)as usize*2;let raw=u16at(self.cmap,word)?;if raw==0{0}else{raw.wrapping_add(delta)as u32}};break;}id
 };if id>=u32::from(self.num_glyphs){return Err(err("mapped-glyph-out-of-range",0));}Ok(id as u16)}
 pub fn glyph(&self,ch:char)->Result<Outline>{self.outline(self.glyph_id(ch)?)}
 pub fn outline(&self,id:u16)->Result<Outline>{let mut budget=Budget{points:0,contours:0,components:0};self.decode(id,&mut Vec::new(),&mut budget)}
 fn metrics(&self,id:u16)->Result<(f64,f64)>{let index=id.min(self.metrics-1)as usize;let advance=u16at(self.hmtx,index*4)?;let lsb=if id<self.metrics{i16at(self.hmtx,id as usize*4+2)?}else{i16at(self.hmtx,self.metrics as usize*4+(id-self.metrics)as usize*2)?};Ok((f64::from(advance),f64::from(lsb)))}
 fn offset(&self,id:usize)->Result<usize>{if self.long_loca{Ok(u32at(self.loca,id*4)?as usize)}else{Ok(u16at(self.loca,id*2)?as usize*2)}}
 fn decode(&self,id:u16,path:&mut Vec<u16>,budget:&mut Budget)->Result<Outline>{
  if id>=self.num_glyphs{return Err(err("component-glyph-out-of-range",0));}if path.len()>=DEPTH{return Err(err("composite-depth-bound",0));}if path.contains(&id){return Err(err("composite-cycle",0));}budget.components+=1;if budget.components>COMPONENTS{return Err(err("glyph-component-bound",0));}
  let(advance,left_bearing)=self.metrics(id)?;let mut out=Outline{glyph_id:id,advance,left_bearing,origin_x:0.,units_per_em:self.units_per_em,contours:Vec::new(),unhinted:true};
  let start=self.offset(id as usize)?;let end=self.offset(id as usize+1)?;if start==end{return Ok(out);}let bytes=range(self.glyf,start,end-start)?;range(bytes,0,10)?;let count=i16at(bytes,0)?;out.origin_x=left_bearing-f64::from(i16at(bytes,2)?);let mut r=Reader{bytes,at:10};path.push(id);
  let result=if count>=0{self.simple(count as usize,&mut r,&mut out,budget)}else{self.composite(&mut r,&mut out,path,budget)};path.pop();result?;Ok(out)
 }
 fn simple(&self,count:usize,r:&mut Reader<'_>,out:&mut Outline,budget:&mut Budget)->Result<()>{
  if count>CONTOURS||budget.contours+count>CONTOURS{return Err(err("glyph-contour-bound",r.at));}budget.contours+=count;
  if count==0{if r.at<r.bytes.len(){let n=r.word()?as usize;r.skip(n)?;}return Ok(());}
  let mut ends=Vec::with_capacity(count);for _ in 0..count{let end=r.word()?as usize;if ends.last().map(|old|end<=*old).unwrap_or(false){return Err(err("invalid-contour-endpoints",r.at));}ends.push(end);}
  let points=ends[count-1]+1;if budget.points+points>POINTS{return Err(err("glyph-point-bound",r.at));}budget.points+=points;let instructions=r.word()?as usize;r.skip(instructions)?;
  let mut flags=Vec::with_capacity(points);while flags.len()<points{let flag=r.byte()?;if flag&128!=0{return Err(err("reserved-simple-glyph-flag",r.at-1));}let repeat=if flag&8!=0{r.byte()?as usize+1}else{1};if repeat>points-flags.len(){return Err(err("glyph-flag-repeat-overflow",r.at));}flags.extend(std::iter::repeat_n(flag,repeat));}
  let mut coordinates=vec![Point{x:0.,y:0.,on_curve:false};points];for axis in 0..2{let short=if axis==0{2}else{4};let same=if axis==0{16}else{32};let mut value=0i32;
   for(i,flag)in flags.iter().enumerate(){let delta=if flag&short!=0{let n=r.byte()?as i32;if flag&same!=0{n}else{-n}}else if flag&same!=0{0}else{r.signed()?as i32};value=value.checked_add(delta).ok_or(err("glyph-coordinate-overflow",r.at))?;if value.abs()>1_000_000_000{return Err(err("glyph-coordinate-bound",r.at));}if axis==0{coordinates[i].x=f64::from(value);coordinates[i].on_curve=flag&1!=0;}else{coordinates[i].y=f64::from(value);}}
  }let mut begin=0;for end in ends{out.contours.push(coordinates[begin..=end].to_vec());begin=end+1;}Ok(())
 }
 fn composite(&self,r:&mut Reader<'_>,out:&mut Outline,path:&mut Vec<u16>,budget:&mut Budget)->Result<()>{
  let mut instructions=false;let mut first=true;let mut own_metrics=false;
  loop{let flags=r.word()?;let glyph=r.word()?;if flags&0xe010!=0{return Err(err("reserved-composite-flags",r.at-4));}instructions|=flags&256!=0;
   let xy=flags&2!=0;let(a,b)=if flags&1!=0{if xy{(i32::from(r.signed()?),i32::from(r.signed()?))}else{(i32::from(r.word()?),i32::from(r.word()?))}}else if xy{(i32::from(r.byte()?as i8),i32::from(r.byte()?as i8))}else{(i32::from(r.byte()?),i32::from(r.byte()?))};
   if first&&!xy{return Err(err("first-composite-component-needs-offset",r.at));}first=false;
   if flags&4!=0&&xy{return Err(err("grid-rounded-composite-offset-needs-raster-size",r.at));}
   let transform_flags=(flags&8!=0)as u8+(flags&64!=0)as u8+(flags&128!=0)as u8;if transform_flags>1{return Err(err("conflicting-composite-transforms",r.at));}
   let(mut xx,mut xy_scale,mut yx_scale,mut yy)=(1.,0.,0.,1.);if flags&8!=0{xx=r.scale()?;yy=xx;}else if flags&64!=0{xx=r.scale()?;yy=r.scale()?;}else if flags&128!=0{xx=r.scale()?;xy_scale=r.scale()?;yx_scale=r.scale()?;yy=r.scale()?;}
   if flags&0x1800==0x1800{return Err(err("ambiguous-composite-offset-scaling",r.at));}
   let mut child=self.decode(glyph,path,budget)?;
   for contour in &mut child.contours{for p in contour{let(x,y)=(xx*p.x+yx_scale*p.y,xy_scale*p.x+yy*p.y);p.x=x;p.y=y;}}
   let(dx,dy)=if xy{let(x,y)=(f64::from(a),f64::from(b));if flags&0x0800!=0{(xx*x+yx_scale*y,xy_scale*x+yy*y)}else{(x,y)}}else{
    let parent=out.contours.iter().flatten().nth(a as usize).ok_or(err("composite-parent-point-or-phantom-unsupported",r.at))?;let child=child.contours.iter().flatten().nth(b as usize).ok_or(err("composite-child-point-or-phantom-unsupported",r.at))?;(parent.x-child.x,parent.y-child.y)
   };
   if flags&512!=0{if own_metrics||xx!=1.||yy!=1.||xy_scale!=0.||yx_scale!=0.||dx!=0.||dy!=0.{return Err(err("transformed-or-repeated-component-metrics-unsupported",r.at));}own_metrics=true;out.advance=child.advance;let parent_x_min=out.left_bearing-out.origin_x;out.origin_x=child.origin_x;out.left_bearing=parent_x_min+out.origin_x;}
   for contour in &mut child.contours{for p in contour{p.x+=dx;p.y+=dy;if !p.x.is_finite()||!p.y.is_finite()||p.x.abs()>1e9||p.y.abs()>1e9{return Err(err("transformed-glyph-coordinate-bound",r.at));}}}
   out.contours.extend(child.contours);if flags&32==0{break;}
  }if instructions{let length=r.word()?as usize;r.skip(length)?;}Ok(())
 }
}
struct Budget{points:usize,contours:usize,components:usize}
fn validate_cmap(data:&[u8],format:u16,glyphs:u16)->Result<()>{
 if format==4{range(data,0,16)?;let twice=u16at(data,6)?as usize;if twice==0||twice%2!=0||twice/2>8192{return Err(err("invalid-cmap-segment-count",6));}let n=twice/2;range(data,0,16+n*8)?;if u16at(data,14+n*2)?!=0{return Err(err("invalid-cmap-reserved-word",14+n*2));}let mut previous=None;
  for i in 0..n{let end=u16at(data,14+i*2)?;let start=u16at(data,16+n*2+i*2)?;if start>end||previous.map(|p|start<=p).unwrap_or(false){return Err(err("unordered-cmap-segments",i));}previous=Some(end);let at=16+n*6+i*2;let offset=u16at(data,at)?as usize;if offset!=0{if offset%2!=0{return Err(err("unaligned-cmap-glyph-offset",at));}let first=at+offset;if first<16+n*8{return Err(err("cmap-glyph-offset-outside-array",at));}range(data,first,(end-start)as usize*2+2)?;}}
  if u16at(data,14+(n-1)*2)?!=65535||u16at(data,16+n*2+(n-1)*2)?!=65535{return Err(err("cmap-terminal-segment-missing",0));}
 }else{range(data,0,16)?;if u16at(data,2)?!=0{return Err(err("invalid-cmap-reserved-word",2));}let n=u32at(data,12)?as usize;if n>65536{return Err(err("cmap-group-bound",12));}range(data,16,n*12)?;let mut previous=None;
  for i in 0..n{let at=16+i*12;let start=u32at(data,at)?;let end=u32at(data,at+4)?;let gid=u32at(data,at+8)?;if start>end||end>0x10ffff||previous.map(|p|start<=p).unwrap_or(false){return Err(err("unordered-cmap-groups",at));}previous=Some(end);if gid.checked_add(end-start).map(|g|g>=u32::from(glyphs)).unwrap_or(true){return Err(err("cmap-group-glyph-out-of-range",at));}}
 }Ok(())
}
