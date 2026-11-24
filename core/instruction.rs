//! Encoding-derived instruction IR and candidate blocks; never a machine runner.
use super::control_flow::Branch;
use std::{collections::{BTreeMap,BTreeSet},fmt::Write as _};
#[path="instruction_structure.rs"] mod reconstruction;
const OUTPUT:usize=16384;
const ROWS:usize=96;
#[derive(Clone,Copy)]pub(super) enum Instruction{
 Unknown,Nop,Branch,Trap{kind:u8,immediate:u16},
 Wide{wide:bool,kind:u8,d:u8,immediate:u16,shift:u8},
 Address{page:bool,d:u8,offset:i64},
 Immediate{wide:bool,subtract:bool,flags:bool,d:u8,n:u8,value:u64},
 Arithmetic{wide:bool,subtract:bool,flags:bool,d:u8,n:u8,m:u8,shift:u8,amount:u8},
 Carry{wide:bool,subtract:bool,flags:bool,d:u8,n:u8,m:u8},
 Logical{wide:bool,kind:u8,invert:bool,d:u8,n:u8,m:u8,shift:u8,amount:u8},
 LogicalImmediate{wide:bool,kind:u8,d:u8,n:u8,value:u64},
 Bitfield{wide:bool,kind:u8,d:u8,n:u8,rotate:u8,last:u8},
 Select{wide:bool,kind:u8,d:u8,n:u8,m:u8,condition:u8},
 Product{wide:bool,subtract:bool,d:u8,n:u8,m:u8,a:u8},
 Binary{wide:bool,kind:u8,d:u8,n:u8,m:u8},
 Extract{wide:bool,d:u8,n:u8,m:u8,amount:u8},
 Memory{load:bool,size:u8,t:u8,n:u8,offset:i64,mode:u8,unscaled:bool},
 Pair{load:bool,wide:bool,t:u8,t2:u8,n:u8,offset:i64,mode:u8},
 Literal{wide:bool,t:u8,offset:i64},
}
pub(super) type Word=(u64,u64,u32,Option<Branch>,Instruction);
pub(super) struct Statement{pub offset:u64,pub address:u64,pub word:u32,pub block:u64,pub mnemonic:&'static str,pub text:String,pub path_facts:String}
pub(super) struct Analysis{pub text:String,pub statements:Vec<Statement>}
fn mnemonic(inst:Instruction,branch:Option<Branch>)->&'static str{use Instruction::*;match inst{
 Unknown=>"unclassified",Nop=>"NOP",Branch=>match branch.unwrap(){super::control_flow::Branch::Direct{link,..}=>if link{"BL"}else{"B"},super::control_flow::Branch::Conditional{consistent,..}=>if consistent{"BC.cond"}else{"B.cond"},super::control_flow::Branch::Compare{nonzero,..}=>if nonzero{"CBNZ"}else{"CBZ"},super::control_flow::Branch::Test{nonzero,..}=>if nonzero{"TBNZ"}else{"TBZ"},super::control_flow::Branch::Authenticated{kind,register:_,modifier,key_b}=>super::control_flow::authentication_name(kind,key_b,modifier),super::control_flow::Branch::Register{kind,..}=>["BR","BLR","RET"][kind as usize]},Trap{kind,..}=>["SVC","HVC","SMC","BRK"][kind as usize],
 Wide{kind,..}=>if kind==0{"MOVN"}else if kind==2{"MOVZ"}else{"MOVK"},Address{page,..}=>if page{"ADRP"}else{"ADR"},Immediate{subtract,flags,..}|Arithmetic{subtract,flags,..}=>match(subtract,flags){(false,false)=>"ADD",(false,true)=>"ADDS",(true,false)=>"SUB",_=>"SUBS"},
 Carry{subtract,flags,..}=>match(subtract,flags){(false,false)=>"ADC",(false,true)=>"ADCS",(true,false)=>"SBC",_=>"SBCS"},
 Logical{kind,invert,..}=>match(kind,invert){(0,false)=>"AND",(0,true)=>"BIC",(1,false)=>"ORR",(1,true)=>"ORN",(2,false)=>"EOR",(2,true)=>"EON",(3,false)=>"ANDS",_=>"BICS"},LogicalImmediate{kind,..}=>["AND","ORR","EOR","ANDS"][kind as usize],Bitfield{kind,..}=>["SBFM","BFM","UBFM"][kind as usize],Select{kind,..}=>["CSEL","CSINC","CSINV","CSNEG"][kind as usize],Product{subtract,..}=>if subtract{"MSUB"}else{"MADD"},Binary{kind,..}=>["UDIV","SDIV","LSLV","LSRV","ASRV","RORV"][kind as usize],Extract{..}=>"EXTR",
 Memory{load,size,unscaled,..}=>match(load,unscaled,size){(true,false,0)=>"LDRB",(true,false,1)=>"LDRH",(true,false,_)=>"LDR",(false,false,0)=>"STRB",(false,false,1)=>"STRH",(false,false,_)=>"STR",(true,true,0)=>"LDURB",(true,true,1)=>"LDURH",(true,true,_)=>"LDUR",(false,true,0)=>"STURB",(false,true,1)=>"STURH",_=>"STUR"},Pair{load,..}=>if load{"LDP"}else{"STP"},Literal{..}=>"LDR",
}}
fn signed(n:u32,bits:u32)->i64{((n as i64)<<(64-bits))>>(64-bits)}
fn logical_mask(wide:bool,n:u32,r:u8,s:u8)->Option<u64>{
 if !wide&&n!=0{return None;}let selector=(n<<6)|((!s as u32)&63);if selector==0{return None;}let size=1u32<<(31-selector.leading_zeros());let width=if wide{64}else{32};if size<2||size>width{return None;}let ones=(s as u32&(size-1))+1;if ones==size{return None;}let rotation=r as u32&(size-1);let mut bits=0;
 for i in 0..width{if(i+rotation)%size<ones{bits|=1u64<<i;}}Some(bits)
}
pub(super) fn decode(w:u32,branch:Option<Branch>)->Instruction{
 use Instruction::*;
 if branch.is_some(){return Branch;}
 if w==0xd503201f{return Nop;}
 let trap=w&0xffe0001f;
 if matches!(trap,0xd4000001|0xd4000002|0xd4000003|0xd4200000){return Trap{kind:match trap{0xd4000001=>0,0xd4000002=>1,0xd4000003=>2,_=>3},immediate:((w>>5)&0xffff)as u16};}
 let wide=w>>31!=0;let d=(w&31)as u8;let n=((w>>5)&31)as u8;let m=((w>>16)&31)as u8;
 if w&0x1f000000==0x10000000{return Address{page:wide,d,offset:signed(((w>>5)&0x7ffff)*4+((w>>29)&3),21)*if wide{4096}else{1}};}
 if w&0x1f800000==0x12800000{let kind=((w>>29)&3)as u8;let hw=((w>>21)&3)as u8;if kind==1||(!wide&&hw>=2){return Unknown;}return Wide{wide,kind,d,immediate:((w>>5)&0xffff)as u16,shift:hw*16};}
 if w&0x1f800000==0x12000000{let Some(value)=logical_mask(wide,(w>>22)&1,((w>>16)&63)as u8,((w>>10)&63)as u8)else{return Unknown;};return LogicalImmediate{wide,kind:((w>>29)&3)as u8,d,n,value};}
 if w&0x1f800000==0x13000000{let kind=((w>>29)&3)as u8;let rotate=((w>>16)&63)as u8;let last=((w>>10)&63)as u8;if kind==3||((w>>22)&1)!=u32::from(wide)||(!wide&&(rotate>=32||last>=32)){return Unknown;}return Bitfield{wide,kind,d,n,rotate,last};}
 if w&0x7fa00000==0x13800000{let amount=((w>>10)&63)as u8;if ((w>>22)&1)!=u32::from(wide)||(!wide&&amount>=32){return Unknown;}return Extract{wide,d,n,m,amount};}
 if w&0x3fe00800==0x1a800000{return Select{wide,kind:(((w>>29)&2)|((w>>10)&1))as u8,d,n,m,condition:((w>>12)&15)as u8};}
 if w&0x7fe00000==0x1b000000{return Product{wide,subtract:w&(1<<15)!=0,d,n,m,a:((w>>10)&31)as u8};}
 let binary=w&0x7fe0fc00;let kind=match binary{0x1ac00800=>Some(0),0x1ac00c00=>Some(1),0x1ac02000=>Some(2),0x1ac02400=>Some(3),0x1ac02800=>Some(4),0x1ac02c00=>Some(5),_=>None};if let Some(kind)=kind{return Binary{wide,kind,d,n,m};}
 let subtract=w&(1<<30)!=0;let flags=w&(1<<29)!=0;
 if w&0x1fe0fc00==0x1a000000{return Carry{wide,subtract,flags,d,n,m};}
 if w&0x1f800000==0x11000000{return Immediate{wide,subtract,flags,d,n,value:(((w>>10)&0xfff)as u64)<<if w&(1<<22)!=0{12}else{0}};}
 let shift=((w>>22)&3)as u8;let amount=((w>>10)&63)as u8;
 if w&0x1f200000==0x0b000000{if shift==3||(!wide&&amount>=32){return Unknown;}return Arithmetic{wide,subtract,flags,d,n,m,shift,amount};}
 if w&0x1f000000==0x0a000000{if !wide&&amount>=32{return Unknown;}return Logical{wide,kind:((w>>29)&3)as u8,invert:w&(1<<21)!=0,d,n,m,shift,amount};}
 if w&0x3f000000==0x18000000{let kind=w>>30;if kind>1{return Unknown;}return Literal{wide:kind==1,t:d,offset:signed((w>>5)&0x7ffff,19)*4};}
 if w&0x3f000000==0x39000000{let opc=(w>>22)&3;if opc>1{return Unknown;}let size=(w>>30)as u8;return Memory{load:opc==1,size,t:d,n,offset:(((w>>10)&0xfff)as i64)<<size,mode:0,unscaled:false};}
 if w&0x3f200000==0x38000000{let opc=(w>>22)&3;let mode=((w>>10)&3)as u8;if opc>1||mode==2||(mode!=0&&n==d&&n!=31){return Unknown;}return Memory{load:opc==1,size:(w>>30)as u8,t:d,n,offset:signed((w>>12)&0x1ff,9),mode,unscaled:mode==0};}
 if w&0x3e000000==0x28000000{let opc=w>>30;let load=w&(1<<22)!=0;let mode=((w>>23)&3)as u8;let t2=((w>>10)&31)as u8;if !matches!(opc,0|2)||mode==0||(load&&d==t2)||(mode!=2&&n!=31&&(n==d||n==t2)){return Unknown;}return Pair{load,wide:opc==2,t:d,t2,n,offset:signed((w>>15)&127,7)*if opc==2{8}else{4},mode};}
 Unknown
}
#[derive(Clone,Copy)]struct Value{bits:u64,known:u64,relative:bool}
const UNKNOWN:Value=Value{bits:0,known:0,relative:false};
impl Value{
 fn integer(bits:u64)->Self{Self{bits,known:u64::MAX,relative:false}}
 fn coordinate(bits:u64)->Self{Self{bits,known:u64::MAX,relative:true}}
 fn join(self,other:Self)->Self{
  if self.relative||other.relative{return if self.relative==other.relative&&self.known==u64::MAX&&other.known==u64::MAX&&self.bits==other.bits{self}else{UNKNOWN};}
  let known=self.known&other.known&!(self.bits^other.bits);Self{bits:self.bits&known,known,relative:false}
 }
 fn full(self)->Option<Self>{(self.known==u64::MAX).then_some(self)}
 fn width(self,wide:bool)->Self{if wide{self}else if self.relative{Self{bits:0,known:!0xffff_ffff,relative:false}}else{Self{bits:self.bits&0xffff_ffff,known:self.known|!0xffff_ffff,relative:false}}}
 fn describe(self,label:&str)->String{if let Some(v)=self.full(){if v.relative{format!("encoded source coordinate {label}0x{:x}",v.bits)}else{format!("encoded integer 0x{:x}",v.bits)}}else if self.known!=0{format!("partial integer bits0x{:x}, known mask0x{:x}; remaining bits unknown",self.bits&self.known,self.known)}else{"unknown".into()}}
}
#[derive(Clone,Copy,PartialEq,Eq)]struct Flags{n:bool,z:bool,c:bool,v:bool,known:u8}
impl Flags{
 fn bits(self)->u8{(u8::from(self.n)<<3)|(u8::from(self.z)<<2)|(u8::from(self.c)<<1)|u8::from(self.v)}
 fn from_bits(bits:u8,known:u8)->Option<Self>{let known=known&15;if known==0{return None;}let bits=bits&known;Some(Self{n:bits&8!=0,z:bits&4!=0,c:bits&2!=0,v:bits&1!=0,known})}
 fn get(self,bit:u8)->Option<bool>{(self.known&bit!=0).then_some(self.bits()&bit!=0)}
 fn join(self,other:Self)->Option<Self>{Self::from_bits(self.bits(),self.known&other.known&!(self.bits()^other.bits()))}
}
#[derive(Clone)]struct State{registers:[Value;32],flags:Option<Flags>}
impl State{
 fn new()->Self{Self{registers:[UNKNOWN;32],flags:None}}
 fn join(&mut self,other:&Self){for(i,value)in self.registers.iter_mut().enumerate(){*value=value.join(other.registers[i]);}self.flags=match(self.flags,other.flags){(Some(a),Some(b))=>a.join(b),_=>None};}
 fn read(&self,n:u8,wide:bool,sp:bool)->Value{if n==31&&!sp{Value::integer(0)}else{self.registers[n as usize]}.width(wide)}
 fn write(&mut self,n:u8,wide:bool,sp:bool,v:Value){if n!=31||sp{self.registers[n as usize]=v.width(wide);}}
}
fn reg(n:u8,wide:bool,sp:bool)->String{if n==31{match(wide,sp){(true,true)=>"SP",(false,true)=>"WSP",(true,false)=>"XZR",_=>"WZR"}.into()}else{format!("{}{n}",if wide{"X"}else{"W"})}}
fn mask(wide:bool)->u64{if wide{u64::MAX}else{0xffff_ffff}}
fn shifted(v:Value,wide:bool,kind:u8,amount:u8)->Value{
 let width=if wide{64}else{32};if amount==0{return v.width(wide);}if amount>=width{return UNKNOWN.width(wide);}
 let word=mask(wide);let v=if v.relative{UNKNOWN}else{v};let x=v.bits&word;let known=v.known&word;let inserted=(word<<(width-amount))&word;
 let(bits,known)=match kind{
  0=>(x<<amount,(known<<amount)|((1u64<<amount)-1)),
  1=>(x>>amount,(known>>amount)|inserted),
  2=>{let sign=1u64<<(width-1);let sign_known=known&sign!=0;((x>>amount)|if sign_known&&x&sign!=0{inserted}else{0},(known>>amount)|if sign_known{inserted}else{0})},
  _=>if wide{(x.rotate_right(amount as u32),known.rotate_right(amount as u32))}else{((x as u32).rotate_right(amount as u32)as u64,(known as u32).rotate_right(amount as u32)as u64)},
 };
 let known=known&word;Value{bits:bits&known,known,relative:false}.width(wide)
}
fn logical(a:Value,b:Value,wide:bool,kind:u8)->Value{
 let word=mask(wide);let constant=|v:Value|v.full().filter(|v|!v.relative).map(|v|v.bits&word);
 if (matches!(kind,0|3)&&constant(b)==Some(word))||(matches!(kind,1|2)&&constant(b)==Some(0)){return a.width(wide);}
 if (matches!(kind,0|3)&&constant(a)==Some(word))||(matches!(kind,1|2)&&constant(a)==Some(0)){return b.width(wide);}
 let a=if a.relative{UNKNOWN}else{a};let b=if b.relative{UNKNOWN}else{b};let a1=a.bits&a.known&word;let b1=b.bits&b.known&word;let a0=!a.bits&a.known&word;let b0=!b.bits&b.known&word;
 let(bits,known)=match kind{0|3=>{let ones=a1&b1;(ones,ones|a0|b0)},1=>{let ones=a1|b1;(ones,ones|(a0&b0))},_=>{let known=a.known&b.known&word;((a.bits^b.bits)&known,known)}};
 Value{bits,known,relative:false}.width(wide)
}
fn logical_flags(v:Value,wide:bool)->Option<Flags>{
 let v=if v.relative{UNKNOWN}else{v};let word=mask(wide);let sign=1u64<<if wide{63}else{31};let mut known=3;let mut bits=0;
 if v.known&sign!=0{known|=8;if v.bits&sign!=0{bits|=8;}}
 if v.bits&v.known&word!=0{known|=4;}else if v.known&word==word{known|=4;bits|=4;}
 Flags::from_bits(bits,known)
}
fn bitfield(source:Value,old:Value,wide:bool,kind:u8,rotate:u8,last:u8)->Value{
 let width=if wide{64}else{32};let word=mask(wide);
 if rotate==0&&u32::from(last)+1==width{return source.width(wide);}
 let source=if source.relative{UNKNOWN}else{source};let old=if old.relative{UNKNOWN}else{old};
 let ones=u64::MAX>>(63-last);let wmask=shifted(Value::integer(ones),wide,3,rotate).bits&word;
 let count=(u32::from(last)+width-u32::from(rotate))%width+1;let tmask=u64::MAX>>(64-count);let selected=wmask&tmask;
 let rotated=shifted(source,wide,3,rotate);let mut bits=rotated.bits&selected;let mut known=rotated.known&selected;
 if kind==1{bits|=old.bits&!selected&word;known|=old.known&!selected&word;}
 else if kind==0{known|=tmask&!wmask;let sign=1u64<<last;if source.known&sign!=0{let top=!tmask&word;known|=top;if source.bits&sign!=0{bits|=top;}}}
 else{known|=!selected&word;}
 Value{bits:bits&known,known,relative:false}.width(wide)
}
fn extract(a:Value,b:Value,wide:bool,amount:u8)->Value{
 if amount==0{return b.width(wide);}let width=if wide{64}else{32};let word=mask(wide);
 let a=if a.relative{UNKNOWN}else{a};let b=if b.relative{UNKNOWN}else{b};
 let known=((a.known&word)<<(width-amount))|((b.known&word)>>amount);let bits=((a.bits&word)<<(width-amount))|((b.bits&word)>>amount);
 Value{bits:bits&known&word,known:known&word,relative:false}.width(wide)
}
fn product(a:Value,b:Value,wide:bool)->Value{
 let word=mask(wide);let constant=|v:Value|v.full().filter(|v|!v.relative).map(|v|v.bits&word);let ca=constant(a);let cb=constant(b);
 if ca==Some(0)||cb==Some(0){return Value::integer(0);}if ca==Some(1){return b.width(wide);}if cb==Some(1){return a.width(wide);}let a=if a.relative{UNKNOWN}else{a};let b=if b.relative{UNKNOWN}else{b};
 if let Some(c)=ca.filter(|v|v.is_power_of_two()){return shifted(b,wide,0,c.trailing_zeros()as u8);}if let Some(c)=cb.filter(|v|v.is_power_of_two()){return shifted(a,wide,0,c.trailing_zeros()as u8);}
 let width=if wide{64}else{32};let prefix=(a.known&word).trailing_ones().min((b.known&word).trailing_ones());let zeros=(a.known&!a.bits&word).trailing_ones().saturating_add((b.known&!b.bits&word).trailing_ones());let count=prefix.max(zeros).min(width);if count==0{return UNKNOWN.width(wide);}
 let known=u64::MAX>>(64-count);Value{bits:(a.bits&a.known&word).wrapping_mul(b.bits&b.known&word)&known,known,relative:false}.width(wide)
}
fn shift_name(n:u8)->&'static str{match n{0=>"LSL",1=>"LSR",2=>"ASR",_=>"ROR"}}
fn coordinate_delta(pc:u64,delta:i64)->Option<Value>{if delta<0{pc.checked_sub(delta.unsigned_abs())}else{pc.checked_add(delta as u64)}.map(Value::coordinate)}
/// Address of a PC-relative encoding when coordinate arithmetic succeeds.
/// The cross-reference pass accepts a direct target only when this agrees with `displaced`.
pub(super) fn coordinate_target(pc:u64,delta:i64)->Option<u64>{coordinate_delta(pc,delta).and_then(|value|value.full().filter(|value|value.relative).map(|value|value.bits))}
fn arithmetic_bits(a:Value,b:Value,wide:bool,subtract:bool)->(Value,Option<Flags>){arithmetic_carry_bits(a,b,wide,subtract,if subtract{2}else{1})}
fn arithmetic_carry_bits(a:Value,b:Value,wide:bool,subtract:bool,mut carry:u8)->(Value,Option<Flags>){
 let width=if wide{64}else{32};let(mut bits,mut known)=(0u64,0u64);let mut overflow=0u8;
 for i in 0..width{
  let bit=1u64<<i;let aa=if a.known&bit!=0{1u8<<((a.bits>>i)&1)}else{3};let bb=if b.known&bit!=0{1u8<<(((b.bits>>i)&1)^subtract as u64)}else{3};
  let(mut sum,mut next)=(0u8,0u8);
  for x in 0..2u8{for y in 0..2u8{for c in 0..2u8{if aa&(1<<x)!=0&&bb&(1<<y)!=0&&carry&(1<<c)!=0{
   let total=x+y+c;sum|=1<<(total&1);next|=1<<(total>>1);if i+1==width{overflow|=1<<(c^(total>>1));}
  }}}}
  if sum==1||sum==2{known|=bit;if sum==2{bits|=bit;}}carry=next;
 }
 let value=Value{bits,known,relative:false}.width(wide);
 let base=logical_flags(value,wide).unwrap();let mut known=base.known&12;let mut bits=base.bits()&12;if matches!(carry,1|2){known|=2;if carry==2{bits|=2;}}if matches!(overflow,1|2){known|=1;if overflow==2{bits|=1;}}let flags=Flags::from_bits(bits,known);
 (value,flags)
}
fn arithmetic(a:Value,b:Value,wide:bool,subtract:bool)->(Value,Option<Flags>){
 let(Some(a),Some(b))=(a.full(),b.full())else{return if a.relative||b.relative{(UNKNOWN.width(wide),None)}else{arithmetic_bits(a,b,wide,subtract)}};
 if a.relative||b.relative{
  if !wide{return(UNKNOWN.width(wide),None);}
  let value=match(a.relative,b.relative,subtract){(true,false,false)=>a.bits.checked_add(b.bits).map(Value::coordinate),(true,false,true)=>a.bits.checked_sub(b.bits).map(Value::coordinate),(false,true,false)=>a.bits.checked_add(b.bits).map(Value::coordinate),(true,true,true)=>Some(Value::integer(a.bits.wrapping_sub(b.bits))),_=>None};return(value.unwrap_or(UNKNOWN),None);
 }
 let a=a.bits&mask(wide);let b=b.bits&mask(wide);let x=if subtract{a.wrapping_sub(b)}else{a.wrapping_add(b)}&mask(wide);let sign=if wide{1u64<<63}else{1u64<<31};
 let flags=Flags{n:x&sign!=0,z:x==0,c:if subtract{a>=b}else{a as u128+b as u128>mask(wide)as u128},v:if subtract{(a^b)&(a^x)&sign!=0}else{(!(a^b))&(a^x)&sign!=0},known:15};
 (Value::integer(x),Some(flags))
}
fn carry_arithmetic(a:Value,b:Value,wide:bool,subtract:bool,carry:Option<bool>)->(Value,Option<Flags>){
 if a.relative||b.relative{
  let candidate=|c:bool|{let value=arithmetic(a,b,wide,subtract).0;if c!=subtract{arithmetic(value,Value::integer(1),wide,subtract).0}else{value}};
  return(carry.map(candidate).unwrap_or_else(||candidate(false).join(candidate(true))).width(wide),None);
 }
 arithmetic_carry_bits(a,b,wide,subtract,carry.map(|c|1u8<<u8::from(c)).unwrap_or(3))
}
fn condition(flags:Option<Flags>,code:u8)->Option<bool>{
 if code>=14{return Some(true);}let f=flags?;
 let both=|a:Option<bool>,b:Option<bool>|match(a,b){(Some(false),_)|(_,Some(false))=>Some(false),(Some(true),Some(true))=>Some(true),_=>None};
 let equal=|a:Option<bool>,b:Option<bool>|Some(a?==b?);
 let value=match code>>1{0=>f.get(4),1=>f.get(2),2=>f.get(8),3=>f.get(1),4=>both(f.get(2),f.get(4).map(|v|!v)),5=>equal(f.get(8),f.get(1)),_=>both(equal(f.get(8),f.get(1)),f.get(4).map(|v|!v))};value.map(|v|if code&1!=0{!v}else{v})
}
fn memory_address(state:&State,n:u8,offset:i64)->Value{let base=state.read(n,true,true);if offset<0{arithmetic(base,Value::integer(offset.unsigned_abs()),true,true).0}else{arithmetic(base,Value::integer(offset as u64),true,false).0}}
fn address_text(n:u8,offset:i64,mode:u8)->String{match mode{1=>format!("[{}], #{offset}",reg(n,true,true)),3=>format!("[{}, #{offset}]!",reg(n,true,true)),_=>format!("[{}, #{offset}]",reg(n,true,true))}}
fn destination(state:&State,d:u8,wide:bool,sp:bool,label:&str)->String{if d==31&&!sp{"result discarded in zero register".into()}else{format!("{} <- {}",reg(d,wide,sp),state.read(d,wide,sp).describe(label))}}
fn statement(inst:Instruction,branch:Option<Branch>,pc:u64,state:&mut State,label:&str)->String{
 use Instruction::*;
 match inst{
  Unknown=>"unclassified/reserved/unsupported word; candidate block and register/flag knowledge end here".into(),
  Nop=>"NOP; no register/flag change".into(),
  Trap{kind,immediate}=>format!("{} #{immediate}; external exception/debug transition, continuation unresolved",["SVC","HVC","SMC","BRK"][kind as usize]),
  Wide{wide,kind,d,immediate,shift}=>{
   let bits=(immediate as u64)<<shift;let value=if kind==3{let old=state.read(d,wide,false);if old.relative{UNKNOWN.width(wide)}else{let field=0xffffu64<<shift;Value{bits:(old.bits&!field)|bits,known:old.known|field,relative:false}}}else{Value::integer(if kind==0{!bits}else{bits})};
   state.write(d,wide,false,value);format!("{} {}, #{immediate}, LSL #{shift}; {}",if kind==0{"MOVN"}else if kind==2{"MOVZ"}else{"MOVK"},reg(d,wide,false),destination(state,d,wide,false,label))
  },
  Address{page,d,offset}=>{let value=coordinate_delta(if page{pc&!4095}else{pc},offset).unwrap_or(UNKNOWN);state.write(d,true,false,value);format!("{} {}, displacement {offset}; {}",if page{"ADRP"}else{"ADR"},reg(d,true,false),destination(state,d,true,false,label))},
  Immediate{wide,subtract,flags,d,n,value}=>{let(v,f)=arithmetic(state.read(n,wide,true),Value::integer(value),wide,subtract);state.write(d,wide,!flags,v);if flags{state.flags=f;}format!("{}{} {}, {}, #{value}; {}{}",if subtract{"SUB"}else{"ADD"},if flags{"S"}else{""},reg(d,wide,!flags),reg(n,wide,true),destination(state,d,wide,!flags,label),if flags{"; NZCV updated from available integer operands"}else{""})},
  Arithmetic{wide,subtract,flags,d,n,m,shift,amount}=>{let(v,f)=if subtract&&n==m&&amount==0{arithmetic(Value::integer(0),Value::integer(0),wide,true)}else{arithmetic(state.read(n,wide,false),shifted(state.read(m,wide,false),wide,shift,amount),wide,subtract)};state.write(d,wide,false,v);if flags{state.flags=f;}format!("{}{} {}, {}, {}, {} #{amount}; {}",if subtract{"SUB"}else{"ADD"},if flags{"S"}else{""},reg(d,wide,false),reg(n,wide,false),reg(m,wide,false),shift_name(shift),destination(state,d,wide,false,label))},
  Carry{wide,subtract,flags,d,n,m}=>{
   let carry=state.flags.and_then(|f|f.get(2));let(value,result_flags)=if subtract&&n==m{carry_arithmetic(Value::integer(0),Value::integer(0),wide,true,carry)}else{carry_arithmetic(state.read(n,wide,false),state.read(m,wide,false),wide,subtract,carry)};state.write(d,wide,false,value);if flags{state.flags=result_flags;}
   format!("{}{} {}, {}, {}; incoming carry {}; {}{}",if subtract{"SBC"}else{"ADC"},if flags{"S"}else{""},reg(d,wide,false),reg(n,wide,false),reg(m,wide,false),carry.map(|c|if c{"1"}else{"0"}).unwrap_or("unknown"),destination(state,d,wide,false,label),if flags{"; NZCV updated from available integer operands and carry"}else{""})
  },
  Logical{wide,kind,invert,d,n,m,shift,amount}=>{
   let a=state.read(n,wide,false);let mut b=shifted(state.read(m,wide,false),wide,shift,amount);if invert{if !b.relative{b.bits=!b.bits&mask(wide);}else{b=UNKNOWN.width(wide);}}
   let v=if n==m&&amount==0{match(kind,invert){(0|1|3,false)=>a,(2,false)|(0|3,true)=>Value::integer(0),_=>Value::integer(mask(wide))}}else if kind==1&&n==31&&!invert{b}else{logical(a,b,wide,kind)};
   state.write(d,wide,false,v);if kind==3{state.flags=logical_flags(v,wide);}
   let name=match(kind,invert){(0,false)=>"AND",(0,true)=>"BIC",(1,false)=>"ORR",(1,true)=>"ORN",(2,false)=>"EOR",(2,true)=>"EON",(3,false)=>"ANDS",_=>"BICS"};format!("{name} {}, {}, {}, {} #{amount}; {}",reg(d,wide,false),reg(n,wide,false),reg(m,wide,false),shift_name(shift),destination(state,d,wide,false,label))
  },
  LogicalImmediate{wide,kind,d,n,value}=>{let v=logical(state.read(n,wide,false),Value::integer(value),wide,kind);state.write(d,wide,kind!=3,v);if kind==3{state.flags=logical_flags(v,wide);}format!("{} {}, {}, #0x{value:x}; {}",["AND","ORR","EOR","ANDS"][kind as usize],reg(d,wide,kind!=3),reg(n,wide,false),destination(state,d,wide,kind!=3,label))},
  Bitfield{wide,kind,d,n,rotate,last}=>{
   let source=state.read(n,wide,false);let v=if kind==1&&d==n&&rotate==0{source}else{bitfield(source,state.read(d,wide,false),wide,kind,rotate,last)};state.write(d,wide,false,v);format!("{} {}, {}, #{rotate}, #{last}; {}",["SBFM","BFM","UBFM"][kind as usize],reg(d,wide,false),reg(n,wide,false),destination(state,d,wide,false,label))
  },
  Select{wide,kind,d,n,m,condition:code}=>{let a=state.read(n,wide,false);let mut b=state.read(m,wide,false);if kind!=0{b=if b.relative{UNKNOWN.width(wide)}else{match kind{1=>arithmetic(b,Value::integer(1),wide,false).0,2=>Value{bits:!b.bits&b.known&mask(wide),known:b.known,relative:false}.width(wide),_=>arithmetic(Value::integer(0),b,wide,true).0}};}
   let value=match condition(state.flags,code){Some(true)=>a,Some(false)=>b,None=>a.join(b).width(wide)};state.write(d,wide,false,value);format!("{} {}, {}, {}, condition0x{code:x}; {}",["CSEL","CSINC","CSINV","CSNEG"][kind as usize],reg(d,wide,false),reg(n,wide,false),reg(m,wide,false),destination(state,d,wide,false,label))
  },
  Product{wide,subtract,d,n,m,a}=>{let x=state.read(n,wide,false);let y=state.read(m,wide,false);let acc=state.read(a,wide,false);let p=product(x,y,wide);let one=|v:Value|v.known==u64::MAX&&!v.relative&&v.bits==1;let zero=|v:Value|v.known==u64::MAX&&!v.relative&&v.bits==0;let value=if subtract&&((a==n&&one(y))||(a==m&&one(x))){Value::integer(0)}else if zero(p){acc}else if !subtract&&zero(acc){p}else{arithmetic(acc,p,wide,subtract).0};state.write(d,wide,false,value);format!("{} {}, {}, {}, {}; {}",if subtract{"MSUB"}else{"MADD"},reg(d,wide,false),reg(n,wide,false),reg(m,wide,false),reg(a,wide,false),destination(state,d,wide,false,label))},
  Binary{wide,kind,d,n,m}=>{let a=state.read(n,wide,false);let b=state.read(m,wide,false).full().filter(|v|!v.relative);let value=if kind>=2{b.map(|b|shifted(a,wide,kind-2,(b.bits&if wide{63}else{31})as u8)).unwrap_or(UNKNOWN.width(wide))}else{a.full().filter(|a|!a.relative).zip(b).map(|(a,b)|Value::integer(if b.bits==0{0}else if kind==0{a.bits/b.bits}else if wide{(a.bits as i64).wrapping_div(b.bits as i64)as u64}else{((a.bits as u32)as i32).wrapping_div((b.bits as u32)as i32)as u32 as u64})).unwrap_or(UNKNOWN.width(wide))};state.write(d,wide,false,value);format!("{} {}, {}, {}; {}",["UDIV","SDIV","LSLV","LSRV","ASRV","RORV"][kind as usize],reg(d,wide,false),reg(n,wide,false),reg(m,wide,false),destination(state,d,wide,false,label))},
  Extract{wide,d,n,m,amount}=>{let value=extract(state.read(n,wide,false),state.read(m,wide,false),wide,amount);state.write(d,wide,false,value);format!("EXTR {}, {}, {}, #{amount}; {}",reg(d,wide,false),reg(n,wide,false),reg(m,wide,false),destination(state,d,wide,false,label))},
  Memory{load,size,t,n,offset,mode,unscaled}=>{let addr=if mode==1{state.read(n,true,true)}else{memory_address(state,n,offset)};let update=memory_address(state,n,offset);let wide=size==3;if load{state.write(t,wide,false,UNKNOWN);}if mode==1||mode==3{state.write(n,true,true,update);}
   format!("{}{} {}, {}; {}-byte {} at {}; {}{}",match(load,unscaled){(true,true)=>"LDUR",(false,true)=>"STUR",(true,false)=>"LDR",_=>"STR"},match size{0=>"B",1=>"H",_=>""},reg(t,wide,false),address_text(n,offset,mode),1usize<<size,if load{"load"}else{"store"},addr.describe(label),if load{"memory value unknown; destination overwritten"}else{"store described; memory not accessed"},if mode==1||mode==3{"; base writeback applied symbolically"}else{""})
  },
  Pair{load,wide,t,t2,n,offset,mode}=>{let addr=if mode==1{state.read(n,true,true)}else{memory_address(state,n,offset)};let update=memory_address(state,n,offset);if load{state.write(t,wide,false,UNKNOWN);state.write(t2,wide,false,UNKNOWN);}if mode!=2{state.write(n,true,true,update);}format!("{} {}, {}, {}; pair of {}-byte {} at {}; memory values unknown{}",if load{"LDP"}else{"STP"},reg(t,wide,false),reg(t2,wide,false),address_text(n,offset,mode),if wide{8}else{4},if load{"loads"}else{"stores"},addr.describe(label),if mode!=2{"; base writeback applied symbolically"}else{""})},
  Literal{wide,t,offset}=>{state.write(t,wide,false,UNKNOWN);format!("LDR {}, PC-relative literal displacement {offset}; {}-byte load at {}; memory value unknown",reg(t,wide,false),if wide{8}else{4},coordinate_delta(pc,offset).unwrap_or(UNKNOWN).describe(label))},
  Branch=>match branch.unwrap(){
   super::control_flow::Branch::Direct{link,offset}=>format!("{} displacement {offset}; {}",if link{"BL"}else{"B"},coordinate_delta(pc,offset).unwrap_or(UNKNOWN).describe(label)),
   super::control_flow::Branch::Conditional{condition:code,offset,consistent}=>format!("{} condition0x{code:x}, displacement {offset}; locally determined condition {:?}",if consistent{"BC.cond (FEAT_HBC)"}else{"B.cond"},condition(state.flags,code)),
   super::control_flow::Branch::Compare{nonzero,wide,register,offset}=>format!("{} {}, displacement {offset}; operand {}",if nonzero{"CBNZ"}else{"CBZ"},reg(register,wide,false),state.read(register,wide,false).describe(label)),
   super::control_flow::Branch::Test{nonzero,bit,register,offset}=>format!("{} {}, bit{bit}, displacement {offset}; operand {}",if nonzero{"TBNZ"}else{"TBZ"},reg(register,bit>=32,false),state.read(register,bit>=32,false).describe(label)),
   super::control_flow::Branch::Authenticated{kind,register,modifier,key_b}=>format!("{} X{register}, {}; instruction key {}; FEAT_PAuth; target requires runtime authentication context",super::control_flow::authentication_name(kind,key_b,modifier),super::control_flow::authentication_operand(modifier),if key_b{"B"}else{"A"}),
   super::control_flow::Branch::Register{kind,register}=>format!("{} {}; target {}",["BR","BLR","RET"][kind as usize],reg(register,true,false),state.read(register,true,false).describe(label)),
  }
 }
}
fn terminal(inst:Instruction)->bool{matches!(inst,Instruction::Branch|Instruction::Unknown|Instruction::Trap{..})}
fn direct(branch:Branch)->Option<i64>{match branch{Branch::Direct{offset,..}|Branch::Conditional{offset,..}|Branch::Compare{offset,..}|Branch::Test{offset,..}=>Some(offset),_=>None}}
fn test_branch(branch:Branch,state:&State)->Option<bool>{match branch{
 Branch::Conditional{condition:code,..}=>condition(state.flags,code),
 Branch::Compare{nonzero,wide,register,..}=>state.read(register,wide,false).full().filter(|v|!v.relative).map(|v|(v.bits!=0)==nonzero),
 Branch::Test{nonzero,bit,register,..}=>{let v=state.read(register,bit>=32,false);(v.known&(1u64<<bit)!=0&&!v.relative).then_some((v.bits&(1u64<<bit)!=0)==nonzero)},_=>None}}
struct Text{body:String,rows:usize,omitted:usize,limit:usize}
impl Text{fn line(&mut self,line:String){if self.rows<ROWS&&self.body.len()+line.len()+1<self.limit{self.body.push_str(&line);self.body.push('\n');self.rows+=1;}else{self.omitted+=1;}}}
fn edge(out:&mut Text,kind:&str,target:Option<Value>,words:&[Word],label:&str){
 let detail=match target{Some(v)=>{let location=if v.relative{if label=="RVA "&&v.bits>u32::MAX as u64{"outside32-bit image RVA width"}else if words.binary_search_by_key(&v.bits,|w|w.1).is_ok(){"captured candidate word; execution unobserved"}else{"outside analyzed word set; no edge invented to unavailable code"}}else{"absolute integer; not reinterpreted as an ELF VA/PE RVA"};format!("{} ({location})",v.describe(label))},None=>"unresolved; register, mapping or arithmetic unavailable".into()};out.line(format!("  Edge {kind}: {detail}."));
}
fn exits(word:&Word,state:&State,words:&[Word],label:&str,out:&mut Text){
 let next=||word.1.checked_add(4).map(Value::coordinate);
 match word.3{
  Some(Branch::Direct{link,offset})=>{edge(out,if link{"call candidate"}else{"jump candidate"},coordinate_delta(word.1,offset),words,label);if link{edge(out,"possible return continuation (callee may not return; no state propagated)",next(),words,label);}},
  Some(b @ (Branch::Conditional{..}|Branch::Compare{..}|Branch::Test{..}))=>{let known=test_branch(b,state);if known!=Some(false){edge(out,"taken alternative",coordinate_delta(word.1,direct(b).unwrap()),words,label);}if known!=Some(true){edge(out,"not-taken alternative",next(),words,label);}},
  Some(Branch::Authenticated{kind,..})=>{edge(out,["authenticated indirect jump candidate","authenticated indirect call candidate","authenticated return candidate"][kind as usize],None,words,label);if kind==1{edge(out,"encoded link continuation after authenticated call",next(),words,label);}},
  Some(Branch::Register{kind,register})=>{edge(out,["indirect jump candidate","indirect call candidate","return candidate"][kind as usize],state.read(register,true,false).full(),words,label);if kind==1{edge(out,"possible return continuation (callee may not return; no state propagated)",next(),words,label);}},
  None=>if !terminal(word.4){edge(out,"sequential encoding continuation",next(),words,label);}else{out.line("  Edge unresolved: unknown instruction or exception transition ends this candidate block.".into());},
 }
}
/// Conditional, one-edge interpretations stay separate from the unknown-entry
/// interpretation. A captured predecessor is not proof of a unique entry, and
/// a call/exception/unknown transition cannot transfer machine state.
fn conditional_paths(words:&[Word],ends:&[(usize,u64,State)],statements:&mut [Statement],label:&str,out:&mut Text,cancelled:&dyn Fn()->bool)->Result<(),String>{
 let(mut edges,mut steps,mut bytes,mut retained,mut omitted)=(0usize,0usize,0usize,0usize,0usize);
 let mut incoming=BTreeMap::<usize,(State,Vec<usize>)>::new();
 for(source,block,state)in ends{
  if cancelled(){return Err("Inspection cancelled during conditional path analysis".into());}
  let word=&words[*source];let mut targets=Vec::with_capacity(2);
  let mut add=|address:Option<Value>,kind:&'static str|{if let Some(v)=address{if v.relative&&(label!="RVA "||v.bits<=u32::MAX as u64){if let Ok(index)=words.binary_search_by_key(&v.bits,|w|w.1){if statements[index].block==v.bits{targets.push((index,kind));}}}}};
  match word.3{
   Some(Branch::Direct{link:false,offset})=>add(coordinate_delta(word.1,offset),"jump"),
   Some(b @ (Branch::Conditional{..}|Branch::Compare{..}|Branch::Test{..}))=>{let known=test_branch(b,state);if known!=Some(false){add(coordinate_delta(word.1,direct(b).unwrap()),"taken alternative");}if known!=Some(true){add(word.1.checked_add(4).map(Value::coordinate),"not-taken alternative");}},
   Some(Branch::Register{kind:0,register})=>add(state.read(register,true,false).full(),"indirect jump"),
   None if !terminal(word.4)=>add(word.1.checked_add(4).map(Value::coordinate),"sequential continuation"),
   _=>{},
  }
  for(target,kind)in targets{
   if edges>=64||steps>=1024{omitted+=1;continue;}edges+=1;let target_block=words[target].1;
   let entry=incoming.entry(target).or_insert_with(||(state.clone(),Vec::new()));
   if !entry.1.contains(source){if !entry.1.is_empty(){entry.0.join(state);}entry.1.push(*source);}
   let mut path=state.clone();
   for index in target..words.len().min(target+32){
    if statements[index].block!=target_block{break;}if steps>=1024{omitted+=1;break;}steps+=1;
    if steps%16==0&&cancelled(){return Err("Inspection cancelled during conditional path analysis".into());}
    let w=&words[index];let interpreted=statement(w.4,w.3,w.1,&mut path,label);
    // Compare only with the original local statement, never an earlier path.
    if interpreted==statements[index].text{continue;}
    let fact=format!("\nConditional path from block {label}0x{block:x}, edge at {label}0x{:x} ({kind}) into block {label}0x{target_block:x}: if those encodings are followed in order, {interpreted}. Other incoming entries remain unknown; this is not a joined or runtime fact.",word.1);
    if statements[index].text.len()+statements[index].path_facts.len()+fact.len()>1600||bytes+fact.len()>24*1024{omitted+=1;continue;}
    bytes+=fact.len();retained+=1;statements[index].path_facts.push_str(&fact);out.line(format!("  At {label}0x{:x}:{}",w.1,fact));
   }
  }
 }
 let(mut joined,mut joined_steps)=(0usize,0usize);
 for(&target,(state,sources))in &incoming{
  if cancelled(){return Err("Inspection cancelled during captured-entry joins".into());}if sources.len()<2{continue;}
  let mut facts=Vec::new();for(index,value)in state.registers.iter().enumerate(){if value.known!=0{facts.push(format!("{} <- {}",reg(index as u8,true,true),value.describe(label)));}}
  if let Some(f)=state.flags{let value=|bit|f.get(bit).map(|v|u8::from(v).to_string()).unwrap_or("unknown".into());facts.push(format!("NZCV N={} Z={} C={} V={}",value(8),value(4),value(2),value(1)));}if facts.is_empty(){continue;}
  let origins=sources.iter().take(8).map(|&index|format!("{label}0x{:x}",words[index].1)).collect::<Vec<_>>().join(", ");
  let fact=format!("\nCaptured-entry join for block {label}0x{:x} across {} distinct predecessor edges (showing {}: {origins}). Under one of these captured predecessor paths, common facts: {} (retained {} of {} facts). External entries keep the original unknown-entry interpretation.",words[target].1,sources.len(),sources.len().min(8),facts.iter().take(4).cloned().collect::<Vec<_>>().join("; "),facts.len().min(4),facts.len());
  if statements[target].text.len()+statements[target].path_facts.len()+fact.len()>1600||bytes+fact.len()>24*1024{omitted+=1;}else{
   bytes+=fact.len();retained+=1;joined+=1;statements[target].path_facts.push_str(&fact);out.line(format!("  At {label}0x{:x}:{}",words[target].1,fact));
  }

 }
 // Retain every existing entry annotation before spending remaining room on
 // derived successor annotations. The passes share the same bounded storage.
 for(&target,(state,sources))in &incoming{
  if cancelled(){return Err("Inspection cancelled during captured-entry interpretation".into());}
  if sources.len()<2||(state.flags.is_none()&&state.registers.iter().all(|v|v.known==0)){continue;}
  let mut common=state.clone();let target_block=words[target].1;
  for index in target..words.len().min(target+32){
   if statements[index].block!=target_block{break;}if steps>=1024{omitted+=1;break;}steps+=1;joined_steps+=1;
   if steps%16==0&&cancelled(){return Err("Inspection cancelled during captured-entry interpretation".into());}
   let w=&words[index];let interpreted=statement(w.4,w.3,w.1,&mut common,label);if interpreted==statements[index].text{continue;}
   let fact=format!("\nCaptured-entry interpretation in block {label}0x{target_block:x}, under its {} captured incoming edges: {interpreted}.",sources.len());
   if statements[index].text.len()+statements[index].path_facts.len()+fact.len()>1600||bytes+fact.len()>24*1024{omitted+=1;continue;}
   bytes+=fact.len();retained+=1;statements[index].path_facts.push_str(&fact);out.line(format!("  At {label}0x{:x}:{}",w.1,fact));
  }
 }
 let _=writeln!(out.body,"Captured-entry joins: {joined} blocks retain common register/flag facts under distinct captured incoming edges. Original block entries retain unknown state. {joined_steps} same-block words interpreted from common captured-entry facts.\n");
 let _=writeln!(out.body,"Conditional predecessor paths: {edges} captured edges examined, {steps} successor words interpreted, {retained} distinct local interpretations retained, {omitted} bounded items omitted; maximum64 edges/1024 steps/32 words per edge/24KiB retained annotations. Only one predecessor edge is transferred; calls, exceptions, unavailable targets and unknown instructions do not propagate state. Common captured-entry facts feed same-block successor interpretations within these shared bounds; alternate external entries retain unknown state.");
 Ok(())
}
/// Starts are all unknown: no calling convention, loader state, or incoming edge
/// can inject fabricated register values. Constants are local encoding facts.
/// This is the implementation of the captured instruction analysis: the report
/// text and the retained statements are produced here.
pub(super) fn analyze(words:&[Word],label:&str,cancelled:&dyn Fn()->bool)->Result<Analysis,String>{
 if words.len()>512{return Err("Instruction analysis exceeds512 captured words".into());}
 let mut starts=BTreeSet::new();
 for(i,w)in words.iter().enumerate(){if i==0||words[i-1].1.checked_add(4)!=Some(w.1)||terminal(words[i-1].4){starts.insert(w.1);}if let Some(delta)=w.3.and_then(direct){if let Some(v)=coordinate_delta(w.1,delta){if words.binary_search_by_key(&v.bits,|w|w.1).is_ok(){starts.insert(v.bits);}}}}
 // Split possible indirect destinations once, then recompute from unknown block
 // entries. Extra conservative splits cannot create register knowledge.
 let mut state=State::new();for(i,w)in words.iter().enumerate(){if i%16==0&&cancelled(){return Err("Inspection cancelled during instruction analysis".into());}if starts.contains(&w.1){state=State::new();}if let Some(Branch::Register{register,..})=w.3{if let Some(v)=state.read(register,true,false).full(){if v.relative&&words.binary_search_by_key(&v.bits,|w|w.1).is_ok(){starts.insert(v.bits);}}}let _=statement(w.4,w.3,w.1,&mut state,label);}
 let reconstruction=reconstruction::reconstruct(words,&starts,label,cancelled)?;
 let limit=OUTPUT.saturating_sub(1800+reconstruction.summary.len()+reconstruction.available_bytes.min(7200));
 let mut out=Text{body:String::from("AArch64 instruction IR, candidate blocks and local register data flow\nScope: captured aligned encoding candidates with source offsets, declared coordinates and local register facts. Each candidate block starts with unknown registers/SP/NZCV; separate captured-entry interpretations retain their incoming-edge conditions. Source coordinates retain their declared address namespace. Graph and local facts share at most512 captured words; the report retains16KiB and96 rows.\n"),rows:0,omitted:0,limit};
 let mut blocks=0;let mut recognized=0;let mut state=State::new();let mut statements=Vec::with_capacity(words.len());let mut block=0;let mut ends=Vec::new();
 for(i,w)in words.iter().enumerate(){
  if i%16==0&&cancelled(){return Err("Inspection cancelled during instruction analysis".into());}
  if starts.contains(&w.1){state=State::new();blocks+=1;block=w.1;out.line(format!("Candidate block {label}0x{:x}: unknown entry registers, stack and flags.",w.1));}
  if !matches!(w.4,Instruction::Unknown){recognized+=1;}
  let line=statement(w.4,w.3,w.1,&mut state,label);out.line(format!("  IR file offset{}, {label}0x{:x}, word0x{:08x}: {line}.",w.0,w.1,w.2));statements.push(Statement{offset:w.0,address:w.1,word:w.2,block,mnemonic:mnemonic(w.4,w.3),text:line,path_facts:String::new()});
  if i+1==words.len()||starts.contains(&words[i+1].1){exits(w,&state,words,label,&mut out);ends.push((i,block,state.clone()));}
 }
 conditional_paths(words,&ends,&mut statements,label,&mut out,cancelled)?;
 let _=writeln!(out.body,"Computed {blocks} candidate blocks over {} captured words; recognized {recognized} supported instruction encodings, {} unclassified; reported {} IR/graph rows, omitted {} rows. Omitted rows are not absent behavior.",words.len(),words.len()-recognized,out.rows,out.omitted);
 out.body.push_str("Supported IR: existing branches, NOP, move-wide immediates, ADR/ADRP, immediate/shifted-register ADD/SUB and register ADC/ADCS/SBC/SBCS with NZCV, immediate/shifted-register logical operations, signed/unsigned bitfield moves, register extraction, conditional selects, integer multiply-add/subtract, signed/unsigned division, variable shifts/rotates, unsigned/unscaled/pre/post-indexed integer loads/stores, integer register pairs, literal integer loads and selected exception encodings. Source-coordinate arithmetic uses checked address bounds; integer arithmetic follows encoded widths.\n");
 reconstruction.write(&mut out.body,OUTPUT);
 Ok(Analysis{text:out.body,statements})
}
/// Observe the captured encoding candidates and return the instruction IR with
/// its retained statements. The observation is the captured instruction
/// analysis itself; this named entry keeps the binary observers readable.
pub(super) fn observe(words:&[Word],label:&str,cancelled:&dyn Fn()->bool)->Result<Analysis,String>{
 analyze(words,label,cancelled)
}
