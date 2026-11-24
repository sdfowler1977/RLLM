//! Embedded base-color images; sampling never performs network or file lookup.
use super::*;

pub(super) struct Texture{width:u32,height:u32,pixels:Vec<[f32;4]>,s:[usize;2],mag:usize,min:usize}
impl Texture{
 fn pixel(&self,x:i64,y:i64)->Color{
  fn wrapped(x:i64,n:i64,mode:usize)->usize{match mode{33071=>x.clamp(0,n-1)as usize,33648=>{let x=x.rem_euclid(n*2);if x<n{x as usize}else{(n*2-1-x)as usize}},_=>x.rem_euclid(n)as usize}}
  let x=wrapped(x,self.width as i64,self.s[0]);let y=wrapped(y,self.height as i64,self.s[1]);
  self.pixels[y*self.width as usize+x].map(|v|v as f64)
 }
 pub(super) fn sample(&self,uv:[f64;2],minified:bool)->Color{
  // Reduce large finite coordinates before integer conversion, preserving each
  // sampler's repeat phase and the half-texel neighbors used by linear filtering.
  let uv=std::array::from_fn::<_,2,_>(|k|match self.s[k]{33071=>uv[k].clamp(0.,1.),33648=>uv[k].rem_euclid(2.),_=>uv[k].rem_euclid(1.)});
  let x=uv[0]*self.width as f64;let y=uv[1]*self.height as f64;
  let filter=if minified{self.min}else{self.mag};
  if filter==9728{return self.pixel(x.floor()as i64,y.floor()as i64);}
  let x=x-0.5;let y=y-0.5;let a=x.floor();let b=y.floor();let tx=x-a;let ty=y-b;
  let p=[self.pixel(a as i64,b as i64),self.pixel(a as i64+1,b as i64),self.pixel(a as i64,b as i64+1),self.pixel(a as i64+1,b as i64+1)];
  std::array::from_fn(|k|(p[0][k]*(1.-tx)+p[1][k]*tx)*(1.-ty)+(p[2][k]*(1.-tx)+p[3][k]*tx)*ty)
 }
 pub(super) fn minified(&self,uv:[[f64;2];3],points:[[f64;3];3],area:f64)->bool{
  // Orthographic viewport: screen derivatives are constant over a triangle.
  let mut rho=0f64;
  for k in 0..2{let a=uv[1][k]-uv[0][k];let b=uv[2][k]-uv[0][k];let d=if k==0{self.width}else{self.height}as f64;
   let dx=(a*(points[2][1]-points[0][1])-b*(points[1][1]-points[0][1]))/area*d;
   let dy=(b*(points[1][0]-points[0][0])-a*(points[2][0]-points[0][0]))/area*d;
   rho=rho.max(dx.abs()).max(dy.abs());
  }rho>1.
 }
}

pub(super) fn load(reader:&mut Reader<'_>,primitive:&Json,count:usize)->Result<Option<(Arc<Texture>,Values)>>{
 if get(primitive,"material")==&Null{return Ok(None);}
 let m=entry(reader.j,"materials",index(get(primitive,"material"))?)?;let info=get(get(m,"pbrMetallicRoughness"),"baseColorTexture");if info==&Null{return Ok(None);}
 let id=index(get(info,"index"))?;let texture=entry(reader.j,"textures",id)?;let image=entry(reader.j,"images",index(get(texture,"source"))?)?;
 // Retain the previous geometry-only behavior for unowned image locations and
 // codecs. No external URI is read, and the original artifact stays exportable.
 if get(image,"uri")!=&Null||text(image,"mimeType")!="image/png"{return Ok(None);}
 let key=format!("TEXCOORD_{}",optional(info,"texCoord",0)?);let uv=reader.accessor(index(get(get(primitive,"attributes"),&key))?,reader.vertices)?;
 if uv.dim!=2||uv.rows.len()!=count||!matches!(uv.component,5121|5123|5126)||(uv.component==5126&&uv.normalized)||(uv.component!=5126&&!uv.normalized){return Err("Scene texture coordinates differ from position count or format".into());}
 if let Some(texture)=reader.textures.get(&id){return Ok(Some((texture.clone(),uv)));}
 let sampler=if get(texture,"sampler")==&Null{&Null}else{entry(reader.j,"samplers",index(get(texture,"sampler"))?)?};
 let wraps=[optional(sampler,"wrapS",10497)?,optional(sampler,"wrapT",10497)?];if wraps.iter().any(|w|!matches!(w,33071|33648|10497)){return Err("Scene texture wrapping mode is invalid".into());}
 let mag=optional(sampler,"magFilter",9729)?;if !matches!(mag,9728|9729){return Err("Scene texture magnification filter is invalid".into());}
 // The glTF fallback for unavailable mip levels selects the base nearest/linear
 // filter, so no synthetic mip chain or claimed trilinear output is introduced.
 let min=match optional(sampler,"minFilter",9729)?{9728|9984|9986=>9728,9729|9985|9987=>9729,_=>return Err("Scene texture minification filter is invalid".into())};
 let view=entry(reader.j,"bufferViews",index(get(image,"bufferView"))?)?;
 if optional(view,"buffer",0)?!=0||get(view,"byteStride")!=&Null{return Err("Scene image requires a packed embedded buffer view".into());}
 let start=optional(view,"byteOffset",0)?;let length=index(get(view,"byteLength"))?;
 if length==0||length>reader.texture_room||start.checked_add(length).is_none_or(|n|n>reader.bytes){return Err("Scene image exceeds its embedded buffer or memory budget".into());}
 let mut raw=vec![0;length];reader.file.seek(SeekFrom::Start(reader.bin+start as u64)).and_then(|_|reader.file.read_exact(&mut raw)).map_err(|e|e.to_string())?;
 // PNG working buffers plus retained samples share the admitted texture room.
 let limits=image_png::Limits{input_bytes:reader.texture_room,pixels:reader.texture_room/32,decoded_bytes:reader.texture_room/2,chunk_count:4096,work:reader.texture_work};
 let image=image_png::decode_png_with_budget(&raw,&limits,&mut reader.texture_work).map_err(|e|format!("Scene texture: {e}"))?;
 let retained=image.rgba.len().checked_mul(4).ok_or("Scene texture size overflow")?;
 if retained>reader.texture_room{return Err("Scene texture exceeds retained memory".into());}reader.texture_room-=retained;
 let pixels=image.rgba.chunks_exact(4).map(|p|std::array::from_fn(|c|{let v=p[c]as f64/65535.;(if c==3{v}else if v<=0.04045{v/12.92}else{((v+0.055)/1.055).powf(2.4)})as f32})).collect();
 let texture=Arc::new(Texture{width:image.width,height:image.height,pixels,s:wraps,mag,min});reader.textures.insert(id,texture.clone());Ok(Some((texture,uv)))
}
