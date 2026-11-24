//! Original bounded RFC 1950/1951 decoder for PNG compressed image bytes.
//! No archive module, filesystem, dictionary, external codec or dependency.
use std::fmt;
const MAX_INPUT:usize=64*1024*1024;
const MAX_OUTPUT:usize=128*1024*1024;
const MAX_PARTS:usize=4096;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub(super) enum ErrorKind{Input,OutputLimit,Resource}
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct InflateError {pub code:&'static str,pub offset:usize,pub kind:ErrorKind}
impl fmt::Display for InflateError{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{write!(f,"{} at compressed byte {}",self.code,self.offset)}}
impl std::error::Error for InflateError{}
type Result<T>=std::result::Result<T,InflateError>;
fn err(code:&'static str,offset:usize)->InflateError{InflateError{code,offset,kind:ErrorKind::Input}}
fn output_err(offset:usize)->InflateError{InflateError{code:"inflate-output-exceeds-expected",offset,kind:ErrorKind::OutputLimit}}
fn resource_err(code:&'static str,offset:usize)->InflateError{InflateError{code,offset,kind:ErrorKind::Resource}}
struct Input<'a,'b>{parts:&'a [&'a [u8]],part:usize,within:usize,offset:usize,total:usize,byte:u8,bits:u8,work:&'b mut usize}
impl Input<'_,'_>{
    fn charge(&mut self,units:usize)->Result<()>{if *self.work<units{*self.work=0;return Err(resource_err("inflate-work-limit",self.offset));}*self.work-=units;Ok(())}
    fn byte(&mut self)->Result<u8>{
        self.charge(1)?;
        while self.part<self.parts.len()&&self.within==self.parts[self.part].len(){self.part+=1;self.within=0;}
        let value=*self.parts.get(self.part).and_then(|p|p.get(self.within)).ok_or(err("zlib-input-truncated",self.offset))?;
        self.within+=1;self.offset+=1;Ok(value)
    }
    fn bits(&mut self,count:u8)->Result<u32>{
        self.charge(count as usize)?;let mut value=0;
        for shift in 0..count{if self.bits==0{self.byte=self.byte()?;self.bits=8;}value|=u32::from(self.byte&1)<<shift;self.byte>>=1;self.bits-=1;}Ok(value)
    }
    fn align(&mut self){self.bits=0;self.byte=0;}
    fn little16(&mut self)->Result<u16>{let a=self.byte()?;let b=self.byte()?;Ok(u16::from_le_bytes([a,b]))}
    fn big32(&mut self)->Result<u32>{Ok(u32::from_be_bytes([self.byte()?,self.byte()?,self.byte()?,self.byte()?]))}
}
#[derive(Clone,Copy)]enum TreeKind{Codes,Literals,Distances}
struct Tree{counts:[u16;16],first:[u16;16],positions:[u16;16],symbols:Vec<u16>,max:u8}
impl Tree{
    fn build(lengths:&[u8],kind:TreeKind,input:&mut Input<'_,'_>)->Result<Self>{
        input.charge(lengths.len().saturating_mul(2)+48)?;
        if lengths.len()>288{return Err(err("huffman-alphabet-bound",input.offset));}
        let mut counts=[0u16;16];let mut max=0;
        for &length in lengths{if length>15{return Err(err("huffman-code-length-invalid",input.offset));}if length!=0{counts[length as usize]+=1;max=max.max(length);}}
        let used:usize=counts.iter().map(|n|*n as usize).sum();
        if used==0&&!matches!(kind,TreeKind::Distances){return Err(err("huffman-empty-tree",input.offset));}
        let mut left=1i32;
        for n in 1..=15{left=left*2-i32::from(counts[n]);if left<0{return Err(err("huffman-oversubscribed-tree",input.offset));}}
        if used!=0&&left!=0&&(!matches!(kind,TreeKind::Literals|TreeKind::Distances)||used!=1||max!=1){return Err(err("huffman-incomplete-tree",input.offset));}
        if matches!(kind,TreeKind::Literals)&&lengths.get(256).copied().unwrap_or(0)==0{return Err(err("huffman-end-marker-missing",input.offset));}
        let mut first=[0u16;16];let mut positions=[0u16;16];let mut code=0u32;let mut position=0u16;
        for length in 1..=15{code=(code+u32::from(counts[length-1]))<<1;first[length]=code as u16;positions[length]=position;position+=counts[length];}
        let mut symbols=vec![0;used];let mut next=positions;
        for (symbol,&length) in lengths.iter().enumerate(){if length!=0{let at=next[length as usize]as usize;symbols[at]=symbol as u16;next[length as usize]+=1;}}
        Ok(Self{counts,first,positions,symbols,max})
    }
    fn decode(&self,input:&mut Input<'_,'_>)->Result<u16>{
        if self.max==0{return Err(err("distance-tree-empty-but-used",input.offset));}
        let mut code=0u32;
        for length in 1..=self.max as usize{code=(code<<1)|input.bits(1)?;let first=u32::from(self.first[length]);if code>=first{let delta=code-first;if delta<u32::from(self.counts[length]){return Ok(self.symbols[self.positions[length]as usize+delta as usize]);}}}
        Err(err("huffman-invalid-code",input.offset))
    }
}
fn fixed(input:&mut Input<'_,'_>)->Result<(Tree,Tree)>{
    let mut lengths=[0;288];lengths[..144].fill(8);lengths[144..256].fill(9);lengths[256..280].fill(7);lengths[280..].fill(8);
    Ok((Tree::build(&lengths,TreeKind::Literals,input)?,Tree::build(&[5;32],TreeKind::Distances,input)?))
}
fn dynamic(input:&mut Input<'_,'_>)->Result<(Tree,Tree)>{
    let literals=input.bits(5)? as usize+257;let distances=input.bits(5)? as usize+1;let codes=input.bits(4)? as usize+4;
    if literals>286{return Err(err("dynamic-literal-count-invalid",input.offset));}
    let order=[16,17,18,0,8,7,9,6,10,5,11,4,12,3,13,2,14,1,15];
    let mut code_lengths=[0u8;19];for &index in &order[..codes]{code_lengths[index]=input.bits(3)? as u8;}
    let alphabet=Tree::build(&code_lengths,TreeKind::Codes,input)?;
    let wanted=literals+distances;let mut lengths=Vec::with_capacity(wanted);
    while lengths.len()<wanted{
        let symbol=alphabet.decode(input)?;
        let (length,count)=match symbol{
            0..=15=>(symbol as u8,1),
            16=>{let prior=*lengths.last().ok_or(err("repeat-code-without-prior-length",input.offset))?;(prior,input.bits(2)? as usize+3)},
            17=>(0,input.bits(3)? as usize+3),18=>(0,input.bits(7)? as usize+11),_=>return Err(err("code-length-symbol-invalid",input.offset)),
        };
        if count>wanted-lengths.len(){return Err(err("code-length-repeat-overflow",input.offset));}
        input.charge(count)?;lengths.extend(std::iter::repeat_n(length,count));
    }
    Ok((Tree::build(&lengths[..literals],TreeKind::Literals,input)?,Tree::build(&lengths[literals..],TreeKind::Distances,input)?))
}
struct Output{bytes:Vec<u8>,expected:usize,growing:bool,s1:u32,s2:u32}
impl Output{
    fn emit(&mut self,byte:u8,input:&mut Input<'_,'_>)->Result<()>{
        if self.bytes.len()==self.expected{return Err(output_err(input.offset));}
        input.charge(1)?;
        if self.growing&&self.bytes.len()==self.bytes.capacity(){let wanted=self.bytes.capacity().saturating_mul(2).max(64).min(self.expected);input.charge(self.bytes.len().saturating_add(wanted))?;self.bytes.try_reserve_exact(wanted-self.bytes.len()).map_err(|_|resource_err("inflate-output-allocation-failed",input.offset))?;}
        self.bytes.push(byte);self.s1+=u32::from(byte);if self.s1>=65521{self.s1-=65521;}self.s2+=self.s1;if self.s2>=65521{self.s2-=65521;}Ok(())
    }
    fn copy(&mut self,distance:usize,count:usize,window:usize,input:&mut Input<'_,'_>)->Result<()>{
        if distance==0||distance>self.bytes.len()||distance>window{return Err(err("deflate-distance-outside-history-window",input.offset));}
        if count>self.expected-self.bytes.len(){return Err(output_err(input.offset));}
        for _ in 0..count{let byte=self.bytes[self.bytes.len()-distance];self.emit(byte,input)?;}Ok(())
    }
}
fn compressed(input:&mut Input<'_,'_>,out:&mut Output,window:usize,lit:&Tree,dist:&Tree)->Result<()>{
    const LENGTH_BASE:[usize;29]=[3,4,5,6,7,8,9,10,11,13,15,17,19,23,27,31,35,43,51,59,67,83,99,115,131,163,195,227,258];
    const LENGTH_EXTRA:[u8;29]=[0,0,0,0,0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3,4,4,4,4,5,5,5,5,0];
    const DIST_BASE:[usize;30]=[1,2,3,4,5,7,9,13,17,25,33,49,65,97,129,193,257,385,513,769,1025,1537,2049,3073,4097,6145,8193,12289,16385,24577];
    const DIST_EXTRA:[u8;30]=[0,0,0,0,1,1,2,2,3,3,4,4,5,5,6,6,7,7,8,8,9,9,10,10,11,11,12,12,13,13];
    loop{let symbol=lit.decode(input)?;match symbol{
        0..=255=>out.emit(symbol as u8,input)?,256=>return Ok(()),257..=285=>{
            let n=(symbol-257)as usize;let length=LENGTH_BASE[n]+input.bits(LENGTH_EXTRA[n])? as usize;
            let d=dist.decode(input)? as usize;if d>=30{return Err(err("reserved-distance-symbol",input.offset));}
            let distance=DIST_BASE[d]+input.bits(DIST_EXTRA[d])? as usize;out.copy(distance,length,window,input)?;
        },_=>return Err(err("reserved-length-symbol",input.offset)),}}
}
fn blocks(input:&mut Input<'_ ,'_>,out:&mut Output,window:usize)->Result<()>{
    loop{input.charge(1)?;let final_block=input.bits(1)?!=0;let kind=input.bits(2)?;
        match kind{
            0=>{input.align();let length=input.little16()?;let inverse=input.little16()?;if length!=!inverse{return Err(err("stored-block-length-check-failed",input.offset));}
                if usize::from(length)>out.expected-out.bytes.len(){return Err(output_err(input.offset));}
                for _ in 0..length{let b=input.byte()?;out.emit(b,input)?;}
            },
            1=>{let(lit,dist)=fixed(input)?;compressed(input,out,window,&lit,&dist)?;},
            2=>{let(lit,dist)=dynamic(input)?;compressed(input,out,window,&lit,&dist)?;},
            _=>return Err(err("reserved-deflate-block-type",input.offset)),
        }
        if final_block{break;}
    }
    Ok(())
}

/// `parts` are consecutive pieces of one zlib stream, e.g. PNG IDAT payloads.
/// No concatenated streams, preset dictionary or trailing bytes are accepted.
pub(super) fn decode_zlib_exact(parts:&[&[u8]],expected_len:usize,work_limit:usize)->Result<Vec<u8>>{
    let mut remaining=work_limit;decode_zlib_exact_with_budget(parts,expected_len,&mut remaining)
}
/// Debits a shared caller-owned work budget even when decoding fails. Fixed
/// bounds precede allocation; work accounts input reads, bit walks, tree builds,
/// length expansion and every output byte/checksum update.
pub(super) fn decode_zlib_exact_with_budget(parts:&[&[u8]],expected_len:usize,remaining_work:&mut usize)->Result<Vec<u8>>{
    if parts.len()>MAX_PARTS{return Err(err("zlib-part-count-bound",0));}
    if expected_len>MAX_OUTPUT{return Err(err("zlib-output-memory-bound",0));}
    if *remaining_work<parts.len(){*remaining_work=0;return Err(resource_err("inflate-work-limit",0));}*remaining_work-=parts.len();
    let total=parts.iter().try_fold(0usize,|sum,p|sum.checked_add(p.len())).ok_or(err("zlib-input-size-overflow",0))?;
    if total>MAX_INPUT{return Err(err("zlib-input-memory-bound",0));}
    let mut input=Input{parts,part:0,within:0,offset:0,total,byte:0,bits:0,work:remaining_work};
    let cmf=input.byte()?;let flg=input.byte()?;
    if cmf&15!=8||cmf>>4>7{return Err(err("unsupported-zlib-method-or-window",0));}
    if (u16::from(cmf)*256+u16::from(flg))%31!=0{return Err(err("zlib-header-check-failed",0));}
    if flg&32!=0{return Err(err("zlib-preset-dictionary-not-supported",1));}
    if *input.work<expected_len{return Err(resource_err("inflate-work-insufficient-for-expected-output",input.offset));}
    let window=1usize<<((cmf>>4)as usize+8);
    let mut bytes=Vec::new();bytes.try_reserve_exact(expected_len).map_err(|_|resource_err("zlib-output-allocation-failed",input.offset))?;
    let mut out=Output{bytes,expected:expected_len,growing:false,s1:1,s2:0};
    blocks(&mut input,&mut out,window)?;
    // The final DEFLATE byte may contain unused padding bits. RFC framing
    // discards these bits, but never any additional whole bytes or stream.
    input.align();let checksum=input.big32()?;
    if checksum!=((out.s2<<16)|out.s1){return Err(err("zlib-adler32-check-failed",input.offset-4));}
    if input.offset!=input.total{return Err(err("zlib-trailing-data",input.offset));}
    if out.bytes.len()!=expected_len{return Err(err("inflate-output-shorter-than-expected",input.offset));}
    Ok(out.bytes)
}

/// One raw DEFLATE member, stopping exactly after its final padded byte.
/// Lazy bounded growth avoids reserving the caller's entire ceiling for empty members.
pub(super) fn decode_raw_bounded(bytes:&[u8],limit:usize,work:&mut usize)->Result<(Vec<u8>,usize)>{
 if bytes.len()>MAX_INPUT||limit>MAX_OUTPUT{return Err(err("raw-deflate-memory-bound",0));}
 let parts=[bytes];let mut input=Input{parts:&parts,part:0,within:0,offset:0,total:bytes.len(),byte:0,bits:0,work};
 let mut out=Output{bytes:Vec::new(),expected:limit,growing:true,s1:1,s2:0};
 blocks(&mut input,&mut out,32768)?;input.align();Ok((out.bytes,input.offset))
}
