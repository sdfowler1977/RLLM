//! Original RGBA surface, unhinted quadratic outline coverage and PNG encoder.
use super::font_outline::Point;
type Result<T>=std::result::Result<T,String>;
const PIXELS:usize=4*1024*1024;
const EDGES:usize=32768;
const WORK:usize=32*1024*1024;
#[derive(Clone,Debug)]
pub(super) struct Surface {pub width:u32,pub height:u32,pub pixels:Vec<u8>,work_remaining:usize}
impl Surface {
 pub fn new(width:u32,height:u32,color:[u8;4])->Result<Self>{let count=(width as usize).checked_mul(height as usize).ok_or("Image dimensions overflow")?;if width==0||height==0||width>4096||height>4096||count>PIXELS{return Err("Image exceeds 4096-per-side / 4 Mi-pixel bound".into());}let mut pixels=Vec::with_capacity(count*4);for _ in 0..count{pixels.extend_from_slice(&color);}Ok(Self{width,height,pixels,work_remaining:128*1024*1024})}
 fn charge(&mut self,work:usize)->Result<()>{charge(&mut self.work_remaining,work)}
 fn blend(&mut self,x:usize,y:usize,color:[u8;4],coverage:u32){if coverage==0||color[3]==0{return;}let i=(y*self.width as usize+x)*4;let sa=(color[3] as u32*coverage+8)/16;let da=self.pixels[i+3] as u32;let a=sa*255+da*(255-sa);if a==0{return;}for c in 0..3{let numerator=color[c] as u32*sa*255+self.pixels[i+c] as u32*da*(255-sa);self.pixels[i+c]=((numerator+a/2)/a).min(255) as u8;}self.pixels[i+3]=((a+127)/255) as u8;}
 pub fn rectangle(&mut self,x:f64,y:f64,width:f64,height:f64,color:[u8;4])->Result<()> {
  if [x,y,width,height,x+width,y+height].iter().any(|n|!n.is_finite()||n.abs()>1_000_000.)||width<0.||height<0.{return Err("Invalid rectangle coordinates".into());}
  let left=x.floor().max(0.).min(self.width as f64) as usize;let right=(x+width).ceil().max(0.).min(self.width as f64) as usize;let top=y.floor().max(0.).min(self.height as f64) as usize;let bottom=(y+height).ceil().max(0.).min(self.height as f64) as usize;
  self.charge((bottom-top).saturating_mul(right-left).saturating_mul(17))?;
  for py in top..bottom{for px in left..right{let mut coverage=0;for sy in 0..4{for sx in 0..4{let xx=px as f64+(sx as f64+0.5)/4.;let yy=py as f64+(sy as f64+0.5)/4.;if xx>=x&&xx<x+width&&yy>=y&&yy<y+height{coverage+=1;}}}self.blend(px,py,color,coverage);}}Ok(())
 }
 /// Explicit nearest-neighbour image sampling with straight-alpha source-over.
 /// Pixel centres select source samples; fractional edges use the same 4x4
 /// coverage grid as rectangles. No color-profile conversion is implied.
 pub fn image_rgba8(&mut self,source:&[u8],sw:u32,sh:u32,x:f64,y:f64,width:f64,height:f64)->Result<()> {
  let count=(sw as usize).checked_mul(sh as usize).ok_or("Source image dimensions overflow")?;
  if sw==0||sh==0||count>PIXELS||count.checked_mul(4)!=Some(source.len()){return Err("Invalid source RGBA image dimensions or byte count".into());}
  if [x,y,width,height,x+width,y+height].iter().any(|n|!n.is_finite()||n.abs()>1_000_000.)||width<0.||height<0.{return Err("Invalid image destination geometry".into());}
  if width==0.||height==0.{return Ok(());}
  let left=x.floor().max(0.).min(self.width as f64) as usize;let right=(x+width).ceil().max(0.).min(self.width as f64) as usize;
  let top=y.floor().max(0.).min(self.height as f64) as usize;let bottom=(y+height).ceil().max(0.).min(self.height as f64) as usize;
  self.charge((bottom-top).saturating_mul(right-left).saturating_mul(22))?;
  for py in top..bottom{for px in left..right{
   let mut coverage=0;for sy in 0..4{for sx in 0..4{let xx=px as f64+(sx as f64+0.5)/4.;let yy=py as f64+(sy as f64+0.5)/4.;if xx>=x&&xx<x+width&&yy>=y&&yy<y+height{coverage+=1;}}}
   let ix=(((px as f64+0.5-x)/width*sw as f64).floor().max(0.) as usize).min(sw as usize-1);
   let iy=(((py as f64+0.5-y)/height*sh as f64).floor().max(0.) as usize).min(sh as usize-1);
   let at=(iy*sw as usize+ix)*4;self.blend(px,py,source[at..at+4].try_into().unwrap(),coverage);
  }}Ok(())
 }
 /// Baseline transform: font +Y is up, surface +Y is down. Nonzero winding,
 /// 4x4 coverage samples; no TrueType instructions or platform hinting.
 pub fn outline(&mut self,contours:&[Vec<Point>],x:f64,baseline:f64,scale:f64,color:[u8;4])->Result<()> {
  if !x.is_finite()||!baseline.is_finite()||!scale.is_finite()||scale<0.||scale>1024.||x.abs()>1_000_000.||baseline.abs()>1_000_000.{return Err("Invalid outline transform".into());}if scale==0.{return Ok(());}
  if contours.len()>4096{return Err("Outline contour bound exceeded".into());}
  let input_points=contours.iter().map(Vec::len).try_fold(0usize,|a,b|a.checked_add(b)).ok_or("Outline point count overflow")?;if input_points>65536{return Err("Outline point bound exceeded".into());}self.charge(input_points.saturating_mul(2).saturating_add(contours.len()))?;
  let mut edges=Vec::new();let mut points=0usize;
  for contour in contours{points=points.checked_add(contour.len()).ok_or("Outline point overflow")?;if points>65536{return Err("Outline point bound exceeded".into());}if contour.is_empty(){continue;}
   let mut v=Vec::with_capacity(contour.len());for p in contour{let p=Vertex{x:x+p.x*scale,y:baseline-p.y*scale,on:p.on_curve};if !p.x.is_finite()||!p.y.is_finite()||p.x.abs()>1_000_000.||p.y.abs()>1_000_000.{return Err("Outline coordinate bound exceeded".into());}v.push(p);}
   let first=v[0];let last=*v.last().unwrap();let start=if first.on{first}else if last.on{last}else{mid(first,last)};
   let mut current=start;let mut at=if first.on{1}else{0};let end=if !first.on&&last.on{v.len()-1}else{v.len()};
   while at<end{let p=v[at];if p.on{line(&mut edges,current,p,&mut self.work_remaining)?;current=p;at+=1;}else{let next=if at+1<end{v[at+1]}else{start};let target=if next.on{next}else{mid(p,next)};curve(&mut edges,current,p,target,0,&mut self.work_remaining)?;current=target;at+=if next.on&&at+1<end{2}else{1};}}
   line(&mut edges,current,start,&mut self.work_remaining)?;
  }
  if edges.is_empty(){return Ok(());}let miny=edges.iter().map(|e|e.0.y.min(e.1.y)).fold(f64::INFINITY,f64::min);let maxy=edges.iter().map(|e|e.0.y.max(e.1.y)).fold(f64::NEG_INFINITY,f64::max);
  let minx=edges.iter().map(|e|e.0.x.min(e.1.x)).fold(f64::INFINITY,f64::min);let maxx=edges.iter().map(|e|e.0.x.max(e.1.x)).fold(f64::NEG_INFINITY,f64::max);let left=minx.floor().max(0.).min(self.width as f64) as usize;let right=maxx.ceil().max(0.).min(self.width as f64) as usize;
  let top=miny.floor().max(0.).min(self.height as f64) as usize;let bottom=maxy.ceil().max(0.).min(self.height as f64) as usize;
  if (bottom-top).checked_mul(4).and_then(|n|n.checked_mul(edges.len())).map(|n|n>WORK).unwrap_or(true){return Err("Outline scan-conversion work bound exceeded".into());}
  let scan=(bottom-top).saturating_mul(4).saturating_mul(edges.len()).saturating_mul(2+edges.len().ilog2() as usize);let samples=(bottom-top).saturating_mul(right-left).saturating_mul(17);self.charge(scan.saturating_add(samples))?;
  // All fallible checks precede pixel changes for this glyph.
  let mut coverage=vec![0u32;right-left];let mut intersections=Vec::with_capacity(edges.len());
  for py in top..bottom{coverage.fill(0);for sy in 0..4{let y=py as f64+(sy as f64+0.5)/4.;intersections.clear();for &(a,b)in &edges{if (a.y<=y&&y<b.y)||(b.y<=y&&y<a.y){let crossing=a.x+(y-a.y)*(b.x-a.x)/(b.y-a.y);intersections.push((crossing,if b.y>a.y{1i32}else{-1}));}}intersections.sort_by(|a,b|a.0.total_cmp(&b.0));let mut index=0;let mut winding=0i32;for px in left..right{for sx in 0..4{let x=px as f64+(sx as f64+0.5)/4.;while index<intersections.len()&&intersections[index].0<=x{winding+=intersections[index].1;index+=1;}if winding!=0{coverage[px-left]+=1;}}}}
   for (px,&n)in coverage.iter().enumerate(){self.blend(px+left,py,color,n);}
  }Ok(())
 }
 pub fn png(&self)->Result<Vec<u8>>{
  let count=(self.width as usize).checked_mul(self.height as usize).ok_or("Invalid surface")?;if count>PIXELS||self.width>4096||self.height>4096||self.pixels.len()!=count*4||self.width==0||self.height==0{return Err("Invalid surface byte count".into());}
  let stride=self.width as usize*4;let mut raw=Vec::with_capacity(self.pixels.len()+self.height as usize);for row in self.pixels.chunks_exact(stride){raw.push(0);raw.extend_from_slice(row);}
  let mut z=vec![0x78,0x01];let chunks=raw.chunks(65535);let total=chunks.len();for(i,chunk)in chunks.enumerate(){z.push(if i+1==total{1}else{0});let len=chunk.len() as u16;z.extend_from_slice(&len.to_le_bytes());z.extend_from_slice(&(!len).to_le_bytes());z.extend_from_slice(chunk);}z.extend_from_slice(&adler(&raw).to_be_bytes());
  let mut out=b"\x89PNG\r\n\x1a\n".to_vec();let mut header=Vec::new();header.extend_from_slice(&self.width.to_be_bytes());header.extend_from_slice(&self.height.to_be_bytes());header.extend_from_slice(&[8,6,0,0,0]);chunk(&mut out,b"IHDR",&header);chunk(&mut out,b"IDAT",&z);chunk(&mut out,b"IEND",&[]);Ok(out)
 }
}
#[derive(Clone,Copy)]struct Vertex{x:f64,y:f64,on:bool}
fn mid(a:Vertex,b:Vertex)->Vertex{Vertex{x:(a.x+b.x)/2.,y:(a.y+b.y)/2.,on:true}}
fn charge(remaining:&mut usize,work:usize)->Result<()>{if work>*remaining{return Err("Capture aggregate painting work bound exceeded".into());}*remaining-=work;Ok(())}
fn line(edges:&mut Vec<(Vertex,Vertex)>,a:Vertex,b:Vertex,work:&mut usize)->Result<()>{charge(work,1)?;if a.x==b.x&&a.y==b.y{return Ok(());}if edges.len()>=EDGES{return Err("Outline segment bound exceeded".into());}edges.push((a,b));Ok(())}
fn curve(edges:&mut Vec<(Vertex,Vertex)>,a:Vertex,b:Vertex,c:Vertex,depth:usize,work:&mut usize)->Result<()>{charge(work,1)?;let dx=a.x-2.*b.x+c.x;let dy=a.y-2.*b.y+c.y;if dx*dx+dy*dy<=0.0625{return line(edges,a,c,work);}if depth>=20{return Err("Outline curve subdivision bound exceeded".into());}let ab=mid(a,b);let bc=mid(b,c);let center=mid(ab,bc);curve(edges,a,ab,center,depth+1,work)?;curve(edges,center,bc,c,depth+1,work)}
fn crc(bytes:&[u8])->u32{let mut c=!0u32;for &b in bytes{c^=b as u32;for _ in 0..8{c=if c&1!=0{(c>>1)^0xedb88320}else{c>>1};}}!c}
fn adler(bytes:&[u8])->u32{let(mut a,mut b)=(1u32,0u32);for &x in bytes{a=(a+x as u32)%65521;b=(b+a)%65521;}(b<<16)|a}
fn chunk(out:&mut Vec<u8>,kind:&[u8;4],data:&[u8]){out.extend_from_slice(&(data.len() as u32).to_be_bytes());let at=out.len();out.extend_from_slice(kind);out.extend_from_slice(data);let checksum=crc(&out[at..]);out.extend_from_slice(&checksum.to_be_bytes());}
#[cfg(test)]mod checks{use super::*;
fn point(x:f64,y:f64)->Point{Point{x,y,on_curve:true}}
#[test]fn coverage_clipping_and_source_over(){let mut s=Surface::new(4,4,[0;4]).unwrap();s.rectangle(-1.,-1.,3.,3.,[255,0,0,255]).unwrap();assert_eq!(&s.pixels[..4],&[255,0,0,255]);assert_eq!(&s.pixels[12..16],&[0;4]);s.rectangle(0.,0.,1.,1.,[0,0,255,128]).unwrap();assert_eq!(&s.pixels[..4],&[127,0,128,255]);let old=s.pixels.clone();assert!(s.rectangle(f64::NAN,0.,1.,1.,[0;4]).is_err());assert_eq!(old,s.pixels);}
#[test]fn nonzero_hole_and_quadratic_coverage(){let mut s=Surface::new(8,8,[255;4]).unwrap();let outer=vec![point(0.,0.),point(8.,0.),point(8.,8.),point(0.,8.)];let hole=vec![point(2.,2.),point(2.,6.),point(6.,6.),point(6.,2.)];s.outline(&[outer,hole],0.,8.,1.,[0,0,0,255]).unwrap();assert_eq!(&s.pixels[..4],&[0,0,0,255]);assert_eq!(&s.pixels[(3*8+3)*4..(3*8+3)*4+4],&[255;4]);let curve=vec![point(0.,0.),Point{x:4.,y:8.,on_curve:false},point(8.,0.)];s.outline(&[curve],0.,8.,1.,[255,0,0,128]).unwrap();assert!(s.pixels.chunks(4).any(|p|p[0]>0&&p[1]==0));}
#[test]fn aggregate_work_refuses_without_partial_glyph_pixels(){let mut s=Surface::new(8,8,[0;4]).unwrap();s.work_remaining=4;let before=s.pixels.clone();assert!(s.rectangle(0.,0.,8.,8.,[255;4]).unwrap_err().contains("aggregate"));assert_eq!(s.pixels,before);s.work_remaining=1;assert!(s.outline(&[vec![point(0.,0.),point(8.,0.),point(0.,8.)]],0.,8.,1.,[255;4]).unwrap_err().contains("aggregate"));assert_eq!(s.pixels,before);}
#[test]fn png_signature_chunks_and_checksums(){let s=Surface::new(2,1,[1,2,3,4]).unwrap();let p=s.png().unwrap();assert_eq!(&p[..8],b"\x89PNG\r\n\x1a\n");let mut at=8;let mut kinds=vec![];while at<p.len(){let n=u32::from_be_bytes(p[at..at+4].try_into().unwrap())as usize;let kind=&p[at+4..at+8];kinds.push(kind.to_vec());assert_eq!(crc(&p[at+4..at+8+n]),u32::from_be_bytes(p[at+8+n..at+12+n].try_into().unwrap()));at+=12+n;}assert_eq!(kinds,vec![b"IHDR".to_vec(),b"IDAT".to_vec(),b"IEND".to_vec()]);assert_eq!(crc(b"123456789"),0xcbf43926);assert_eq!(adler(b"Wikipedia"),0x11e60398);}
}
