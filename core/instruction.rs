//! Encoding-derived instruction IR and candidate blocks; never a machine runner.
use super::control_flow::Branch;
use std::{collections::{BTreeMap,BTreeSet},fmt::Write as _};
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
struct Text{body:String,rows:usize,omitted:usize}
impl Text{fn line(&mut self,line:String){if self.rows<ROWS&&self.body.len()+line.len()+1<OUTPUT-1800{self.body.push_str(&line);self.body.push('\n');self.rows+=1;}else{self.omitted+=1;}}}
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
pub(super) fn analyze(words:&[Word],label:&str,cancelled:&dyn Fn()->bool)->Result<String,String>{observe(words,label,cancelled).map(|v|v.text)}
pub(super) fn observe(words:&[Word],label:&str,cancelled:&dyn Fn()->bool)->Result<Analysis,String>{
 if words.len()>512{return Err("Instruction analysis exceeds512 captured words".into());}
 let mut starts=BTreeSet::new();
 for(i,w)in words.iter().enumerate(){if i==0||words[i-1].1.checked_add(4)!=Some(w.1)||terminal(words[i-1].4){starts.insert(w.1);}if let Some(delta)=w.3.and_then(direct){if let Some(v)=coordinate_delta(w.1,delta){if words.binary_search_by_key(&v.bits,|w|w.1).is_ok(){starts.insert(v.bits);}}}}
 // Split possible indirect destinations once, then recompute from unknown block
 // entries. Extra conservative splits cannot create register knowledge.
 let mut state=State::new();for(i,w)in words.iter().enumerate(){if i%16==0&&cancelled(){return Err("Inspection cancelled during instruction analysis".into());}if starts.contains(&w.1){state=State::new();}if let Some(Branch::Register{register,..})=w.3{if let Some(v)=state.read(register,true,false).full(){if v.relative&&words.binary_search_by_key(&v.bits,|w|w.1).is_ok(){starts.insert(v.bits);}}}let _=statement(w.4,w.3,w.1,&mut state,label);}
 let mut out=Text{body:String::from("AArch64 instruction IR, candidate blocks and local register data flow\nScope: captured aligned encoding candidates with source offsets, declared coordinates and local register facts. Each candidate block starts with unknown registers/SP/NZCV; separate captured-entry interpretations retain their incoming-edge conditions. Source coordinates retain their declared address namespace. Graph and local facts share at most512 captured words; the report retains16KiB and96 rows.\n"),rows:0,omitted:0};
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
 Ok(Analysis{text:out.body,statements})
}
#[cfg(test)]mod tests{
 use super::*;
 fn words(values:&[u32])->Vec<Word>{values.iter().enumerate().map(|(i,&word)|{let branch=super::super::control_flow::decode(word);((i*4)as u64,(i*4)as u64,word,branch,decode(word,branch))}).collect()}
 fn report(values:&[u32])->Analysis{observe(&words(values),"declared VA ",&||false).unwrap()}
 #[test]fn predecessor_alternatives_preserve_unknown_entry(){
  let a=report(&[0xd28000e0,0x14000002,0xd2800c60,0x91000401,0xd65f03c0]);let line=format!("{}{}",a.statements[3].text,a.statements[3].path_facts);
  assert!(line.starts_with("ADD X1, X0, #1; X1 <- unknown"));assert!(line.contains("encoded integer 0x8")&&line.contains("encoded integer 0x64"));assert!(line.contains("edge at declared VA 0x4 (jump)")&&line.contains("edge at declared VA 0x8 (sequential continuation)"));assert!(line.contains("Other incoming entries remain unknown"));
 }
 #[test]fn call_unknown_and_exception_never_transfer_state(){
  for transition in [0x94000002,0xffffffff,0xd4000001,0xd63f0040]{let a=report(&[0xd28000e0,transition,0x91000401,0xd65f03c0]);assert!(!a.statements[2].path_facts.contains("Conditional path"),"{transition:x}");assert!(!a.statements[2].text.contains("integer 0x8"));}
 }
 #[test]fn bitfield_known_bits_cover_each_encoded_mask(){
  fn concrete(source:u64,old:u64,width:u32,kind:u8,rotate:u32,last:u32)->u64{let end=(last+width-rotate)%width;let mut out=0;for i in 0..width{let input=(i+rotate)%width;let selected=i<=end&&input<=last;let bit=if selected{source>>input}else if kind==1{old>>i}else if kind==0&&i>end{source>>last}else{0};out|=(bit&1)<<i;}out}
  for wide in [false,true]{let width=if wide{64}else{32};let word=mask(wide);for rotate in 0..width{for last in 0..width{for kind in 0..3{
   let a=Value{bits:0x8123_4567_89ab_cdec&word,known:!3,relative:false}.width(wide);let b=Value{bits:0xfedc_ba98_7654_3210&!5&word,known:!5,relative:false}.width(wide);let result=bitfield(a,b,wide,kind,rotate as u8,last as u8);
   for aa in 0..4{for bb in [0,1,4,5]{let exact=concrete(a.bits|aa,b.bits|bb,width,kind,rotate,last);assert_eq!((exact^result.bits)&result.known&word,0,"{wide} {kind} {rotate} {last}");}}
   let exact=bitfield(Value::integer(a.bits),Value::integer(b.bits),wide,kind,rotate as u8,last as u8);assert_eq!(exact.known,u64::MAX);assert_eq!(exact.bits,concrete(a.bits,b.bits,width,kind,rotate,last));
  }}}}
  let v=bitfield(UNKNOWN,UNKNOWN,true,2,8,15);assert_eq!(v.known,0xffff_ffff_ffff_ff00);assert_eq!(logical(v,Value::integer(0xff00),true,0).bits,0);assert_eq!(logical(v,Value::integer(0xff00),true,0).known,u64::MAX);
 }
 #[test]fn extraction_retains_each_selected_known_bit(){
  for wide in [false,true]{let width=if wide{64}else{32};let word=mask(wide);for amount in 0..width{
   let a=Value{bits:0x8123_4567_89ab_cdec&word,known:!3,relative:false}.width(wide);let b=Value{bits:0xfedc_ba98_7654_3210&!5&word,known:!5,relative:false}.width(wide);let result=extract(a,b,wide,amount as u8);
   for aa in 0..4{for bb in [0,1,4,5]{let expected=((((a.bits|aa)as u128)<<width)|((b.bits|bb)as u128))>>amount;assert_eq!((expected as u64^result.bits)&result.known&word,0);}}
   let exact=extract(Value::integer(a.bits),Value::integer(b.bits),wide,amount as u8);let expected=((((a.bits as u128)<<width)|(b.bits as u128))>>amount)as u64&word;assert_eq!(exact.known,u64::MAX);assert_eq!(exact.bits,expected);
  }}
  let v=extract(UNKNOWN,Value::integer(0),true,8);assert_eq!(v.known,0x00ff_ffff_ffff_ffff);assert_eq!(v.bits,0);
 }
 #[test]fn field_transfers_preserve_identity_types_width_and_flags(){
  let mut state=State::new();state.registers[1]=Value::coordinate(0x1000);state.flags=Some(Flags{n:false,z:true,c:true,v:false,known:15});let flags=state.flags;
  for kind in 0..3{statement(Instruction::Bitfield{wide:true,kind,d:2,n:1,rotate:0,last:63},None,0,&mut state,"VA ");assert!(state.read(2,true,false).relative);assert_eq!(state.read(2,true,false).bits,0x1000);assert!(state.flags==flags);}
  statement(Instruction::Bitfield{wide:true,kind:1,d:1,n:1,rotate:0,last:7},None,0,&mut state,"VA ");assert!(state.read(1,true,false).relative);
  let v=bitfield(Value::coordinate(0x1000),UNKNOWN,true,2,0,7);assert!(!v.relative);assert_eq!(v.known,0xffff_ffff_ffff_ff00);
  assert!(extract(UNKNOWN,Value::coordinate(0x1000),true,0).relative);let v=extract(Value::coordinate(0x1000),Value::integer(0),true,1);assert!(!v.relative);assert_eq!(v.known,0x7fff_ffff_ffff_ffff);
  for v in [bitfield(UNKNOWN,UNKNOWN,false,2,4,7),extract(UNKNOWN,UNKNOWN,false,4)]{assert_eq!(v.known>>32,0xffff_ffff);assert_eq!(v.bits>>32,0);}
  statement(Instruction::Bitfield{wide:true,kind:2,d:31,n:1,rotate:0,last:7},None,0,&mut state,"VA ");assert_eq!(state.read(31,true,false).bits,0);assert!(state.flags==flags);
 }
 #[test]fn partial_flag_conditions_match_every_concrete_completion(){
  let truth:[u16;16]=[0xf0f0,0x0f0f,0xcccc,0x3333,0xff00,0x00ff,0xaaaa,0x5555,0x0c0c,0xf3f3,0xaa55,0x55aa,0x0a05,0xf5fa,0xffff,0xffff];
  for known in 0..16{for bits in 0..16{let facts=Flags::from_bits(bits,known);for code in 0..16{let mut outcomes=0;for concrete in 0..16{if concrete&known==bits&known{outcomes|=1u8<<u8::from(truth[code]&(1<<concrete)!=0);}}let expected=match outcomes{1=>Some(false),2=>Some(true),_=>None};assert_eq!(condition(facts,code as u8),expected,"{known} {bits} {code}");}}}
 }
 #[test]fn flag_joins_keep_only_independently_agreeing_facts(){
  for known_a in 0..16{for bits_a in 0..16{for known_b in 0..16{for bits_b in 0..16{
   let mut a=State::new();a.flags=Flags::from_bits(bits_a,known_a);let mut b=State::new();b.flags=Flags::from_bits(bits_b,known_b);a.join(&b);let expected=known_a&known_b&!(bits_a^bits_b)&15;assert_eq!(a.flags.map(|f|f.known).unwrap_or(0),expected);if let Some(f)=a.flags{assert_eq!(f.bits()&expected,bits_a&expected);}
  }}}}
  let f=logical_flags(UNKNOWN,true).unwrap();assert_eq!((f.known,f.bits()),(3,0));let f=logical_flags(Value{bits:1,known:1,relative:false},true).unwrap();assert_eq!((f.known,f.bits()),(7,0));
  let(_,f)=arithmetic(Value{bits:0,known:15,relative:false},Value::integer(1),true,false);let f=f.unwrap();assert_eq!(f.get(4),Some(false));
 }
 #[test]fn partial_flags_drive_transfers_without_guessing_unknown_conditions(){
  let mut state=State::new();state.registers[4]=Value::integer(42);state.registers[5]=Value::integer(7);
  statement(Instruction::LogicalImmediate{wide:true,kind:3,d:31,n:0,value:255},None,0,&mut state,"VA ");let flags=state.flags.unwrap();assert_eq!((flags.known,flags.bits()),(11,0));
  statement(Instruction::Carry{wide:true,subtract:false,flags:false,d:1,n:31,m:31},None,4,&mut state,"VA ");assert_eq!(state.read(1,true,false).full().unwrap().bits,0);
  statement(Instruction::Carry{wide:true,subtract:true,flags:false,d:2,n:31,m:31},None,8,&mut state,"VA ");assert_eq!(state.read(2,true,false).full().unwrap().bits,u64::MAX);
  for code in [3,5,7,9]{statement(Instruction::Select{wide:true,kind:0,d:3,n:4,m:5,condition:code},None,12,&mut state,"VA ");assert_eq!(state.read(3,true,false).full().unwrap().bits,42);}
  statement(Instruction::Select{wide:true,kind:0,d:3,n:4,m:5,condition:0},None,16,&mut state,"VA ");assert!(state.read(3,true,false).full().is_none());assert!(state.flags==Some(flags));
 }
 #[test]fn product_bits_cover_concrete_wrapping_operands(){
  fn variants(mask:u64)->Vec<u64>{let mut out=vec![0];for i in 0..64{if mask&(1u64<<i)!=0{let prior=out.clone();out.extend(prior.into_iter().map(|v|v|(1u64<<i)));}}out}
  for wide in [false,true]{let word=mask(wide);for base_a in [0,1,0x1234,word,word-15,1u64<<if wide{63}else{31}]{for base_b in [0,1,2,3,0xabcd,word]{for unknown_a in [0,1,3,5,1u64<<if wide{63}else{31}]{for unknown_b in [0,1,3,5]{
   let a=Value{bits:base_a&!unknown_a&word,known:!unknown_a,relative:false}.width(wide);let b=Value{bits:base_b&!unknown_b&word,known:!unknown_b,relative:false}.width(wide);let value=product(a,b,wide);
   for aa in variants(unknown_a&word){for bb in variants(unknown_b&word){let exact=(a.bits|aa).wrapping_mul(b.bits|bb)&word;assert_eq!((exact^value.bits)&value.known&word,0);}}
   if unknown_a==0&&unknown_b==0{assert_eq!(value.known,u64::MAX);assert_eq!(value.bits,a.bits.wrapping_mul(b.bits)&word);}
  }}}}}
  let a=Value{bits:0x1234,known:0xffff,relative:false};let value=product(a,Value::integer(2),true);assert_eq!((value.bits,value.known),(0x2468,0x1ffff));let value=product(a,Value{bits:0xabcd,known:0xffff,relative:false},true);assert_eq!((value.bits,value.known),(0x1234u64.wrapping_mul(0xabcd)&0xffff,0xffff));
  let multiple=Value{bits:0,known:0xffff_ffff,relative:false};assert_eq!(product(multiple,multiple,true).full().unwrap().bits,0);
 }
 #[test]fn product_identities_preserve_types_registers_and_flags(){
  let mut state=State::new();state.flags=logical_flags(UNKNOWN,true);let flags=state.flags;state.registers[2]=Value::integer(0);state.registers[3]=Value::integer(42);state.registers[31]=Value::integer(0xdeadbeef);
  for subtract in [false,true]{statement(Instruction::Product{wide:true,subtract,d:4,n:0,m:2,a:3},None,0,&mut state,"VA ");assert_eq!(state.read(4,true,false).full().unwrap().bits,42);assert!(state.flags==flags);}
  state.registers[2]=Value::integer(1);statement(Instruction::Product{wide:true,subtract:true,d:4,n:0,m:2,a:0},None,0,&mut state,"VA ");assert_eq!(state.read(4,true,false).full().unwrap().bits,0);
  state.registers[0]=Value::coordinate(0x1000);statement(Instruction::Product{wide:true,subtract:false,d:4,n:0,m:2,a:31},None,0,&mut state,"VA ");assert!(state.read(4,true,false).relative);assert_eq!(state.read(4,true,false).bits,0x1000);
  assert_eq!(product(Value::coordinate(0x1000),Value::integer(0),true).full().unwrap().bits,0);let shifted=product(Value::coordinate(0x1000),Value::integer(2),true);assert!(!shifted.relative);assert_eq!((shifted.bits,shifted.known),(0,1));
  statement(Instruction::Product{wide:true,subtract:false,d:31,n:0,m:2,a:31},None,0,&mut state,"VA ");assert_eq!(state.registers[31].bits,0xdeadbeef);assert!(state.flags==flags);
  let narrowed=product(UNKNOWN,Value::integer(2),false);assert_eq!(narrowed.known>>32,0xffff_ffff);assert_eq!(narrowed.bits>>32,0);
 }
 #[test]fn product_accumulation_retains_partial_addend_facts(){
  let mut state=State::new();state.registers[0]=Value::integer(2);state.registers[1]=Value::integer(3);state.registers[2]=Value{bits:0x1000,known:0xffff,relative:false};
  for(subtract,expected)in[(false,0x1006),(true,0xffa)]{statement(Instruction::Product{wide:true,subtract,d:3,n:0,m:1,a:2},None,0,&mut state,"VA ");let value=state.read(3,true,false);assert_eq!(value.bits&0xffff,expected);assert_eq!(value.known&0xffff,0xffff);}
  state.registers[0]=UNKNOWN;state.registers[1]=Value::integer(0);state.registers[2]=Value{bits:1<<63,known:(1<<63)|1,relative:false};statement(Instruction::Product{wide:true,subtract:false,d:3,n:0,m:1,a:2},None,0,&mut state,"VA ");assert_eq!(state.read(3,true,false).known,(1<<63)|1);assert_eq!(state.read(3,true,false).bits,1<<63);
 }
 #[test]fn arithmetic_known_bits_cover_concrete_operands_and_flags(){
  for wide in [false,true]{let word=mask(wide);let sign=1u64<<if wide{63}else{31};
   for subtract in [false,true]{for(base_a,base_b)in [(0,0),(0,word),(sign-8,4),(sign,word-3),(word-7,1)]{for unknown_a in [1u64,3,5]{for unknown_b in [1u64,3,5]{
    let a=Value{bits:base_a&!unknown_a,known:!unknown_a,relative:false}.width(wide);let b=Value{bits:base_b&!unknown_b,known:!unknown_b,relative:false}.width(wide);let(result,flags)=arithmetic(a,b,wide,subtract);
    for aa in 0..8{if aa&!unknown_a!=0{continue;}for bb in 0..8{if bb&!unknown_b!=0{continue;}let(exact,exact_flags)=arithmetic(Value::integer(a.bits|aa),Value::integer(b.bits|bb),wide,subtract);assert_eq!(exact.bits&result.known,result.bits&result.known);if let Some(f)=flags{let g=exact_flags.unwrap();assert_eq!((f.bits()^g.bits())&f.known,0);}}}
   }}}}
  }
  let(value,flags)=arithmetic(Value{bits:0x10,known:!1,relative:false},Value::integer(1),true,false);assert!(value.full().is_none());let f=flags.unwrap();assert!(!f.n&&!f.z&&!f.c&&!f.v);
  assert_eq!(arithmetic(Value::coordinate(0x1000),Value{bits:0,known:!1,relative:false},true,false).0.known,0);
 }
 #[test]fn carry_results_and_flags_match_integer_oracle(){
  for wide in [false,true]{let word=mask(wide);let width=if wide{64}else{32};let sign=1u64<<(width-1);let signed=|x:u64|if x&sign!=0{x as i128-(1i128<<width)}else{x as i128};
   for subtract in [false,true]{for a in [0,1,word,word-1,sign,sign-1]{for b in [0,1,word,word-1,sign,sign-1]{for carry in [Some(false),Some(true),None]{
    let(value,flags)=carry_arithmetic(Value::integer(a),Value::integer(b),wide,subtract,carry);
    for c in [false,true]{if carry.is_some_and(|wanted|wanted!=c){continue;}let addend=if subtract{(!b)&word}else{b};let total=a as u128+addend as u128+u128::from(c);let bits=total as u64&word;let signed_total=if subtract{signed(a)-signed(b)-i128::from(!c)}else{signed(a)+signed(b)+i128::from(c)};let expected=Flags{n:bits&sign!=0,z:bits==0,c:total>word as u128,v:signed_total< -(1i128<<(width-1))||signed_total>=(1i128<<(width-1)),known:15};assert_eq!((bits^value.bits)&value.known,0);if carry.is_some(){assert_eq!(value.known,u64::MAX);assert!(flags==Some(expected));}else if let Some(actual)=flags{assert_eq!((actual.bits()^expected.bits())&actual.known,0);}
    }
   }}}}
  }
 }
 #[test]fn partial_carry_results_cover_concrete_operands(){
  for wide in [false,true]{let word=mask(wide);let sign=1u64<<if wide{63}else{31};for subtract in [false,true]{for carry in [Some(false),Some(true),None]{for(base_a,base_b)in [(0,0),(sign-4,1),(word-3,word-7)]{for unknown_a in [1u64,3,5]{for unknown_b in [1u64,3,5]{
   let a=Value{bits:base_a&!unknown_a,known:!unknown_a,relative:false}.width(wide);let b=Value{bits:base_b&!unknown_b,known:!unknown_b,relative:false}.width(wide);let(value,flags)=carry_arithmetic(a,b,wide,subtract,carry);
   for aa in 0..8{if aa&!unknown_a!=0{continue;}for bb in 0..8{if bb&!unknown_b!=0{continue;}for c in [false,true]{if carry.is_some_and(|wanted|wanted!=c){continue;}let(exact,exact_flags)=carry_arithmetic(Value::integer(a.bits|aa),Value::integer(b.bits|bb),wide,subtract,Some(c));assert_eq!((exact.bits^value.bits)&value.known,0);if let Some(actual)=flags{let exact=exact_flags.unwrap();assert_eq!((actual.bits()^exact.bits())&actual.known,0);}}}}
  }}}}}}
 }
 #[test]fn carry_registers_flags_and_coordinate_types_are_preserved(){
  let mut state=State::new();state.registers[2]=Value::integer(41);state.registers[31]=Value::integer(0xdeadbeef);state.flags=Some(Flags{n:false,z:true,c:true,v:false,known:15});let saved=state.flags;
  let inst=decode(0x9a1f0043,None);assert_eq!(mnemonic(inst,None),"ADC");statement(inst,None,0,&mut state,"RVA ");assert_eq!(state.read(3,true,false).full().unwrap().bits,42);assert!(state.flags==saved);assert_eq!(state.registers[31].bits,0xdeadbeef);
  statement(decode(0xba1f03ff,None),None,4,&mut state,"RVA ");assert!(state.flags==Some(Flags{n:false,z:false,c:false,v:false,known:15}));assert_eq!(state.registers[31].bits,0xdeadbeef);
  for subtract in [false,true]{for c in [false,true]{let(value,flags)=carry_arithmetic(Value::coordinate(0x1000),Value::integer(4),true,subtract,Some(c));assert!(value.relative&&flags.is_none());assert_eq!(value.bits,if subtract{0x1000-4-u64::from(!c)}else{0x1000+4+u64::from(c)});}}
  assert_eq!(carry_arithmetic(Value::coordinate(0x1000),Value::integer(0),true,false,None).0.known,0);assert_eq!(carry_arithmetic(Value::integer(u64::MAX),Value::integer(0),false,false,Some(true)).0.bits,0);
 }
 #[test]fn identical_register_operands_retain_algebraic_identities(){
  for wide in [false,true]{for initial in [UNKNOWN,Value::coordinate(0x1000),Value::integer(0x1234)]{
   let mut state=State::new();state.registers[1]=initial;
   statement(Instruction::Arithmetic{wide,subtract:true,flags:true,d:2,n:1,m:1,shift:1,amount:0},None,0,&mut state,"RVA ");assert_eq!(state.read(2,wide,false).full().unwrap().bits,0);assert!(state.flags==Some(Flags{n:false,z:true,c:true,v:false,known:15}));
   for c in [false,true]{state.flags=Some(Flags{n:false,z:false,c,v:false,known:15});statement(Instruction::Carry{wide,subtract:true,flags:true,d:3,n:1,m:1},None,0,&mut state,"RVA ");assert_eq!(state.read(3,wide,false).full().unwrap().bits,if c{0}else{mask(wide)});assert!(state.flags==Some(Flags{n:!c,z:c,c,v:false,known:15}));}
   for kind in 0..4{for invert in [false,true]{state.registers[1]=initial;statement(Instruction::Logical{wide,kind,invert,d:4,n:1,m:1,shift:3,amount:0},None,0,&mut state,"RVA ");let value=state.read(4,wide,false);match(kind,invert){(0|1|3,false)=>{let expected=initial.width(wide);assert_eq!((value.bits,value.known,value.relative),(expected.bits,expected.known,expected.relative));},(2,false)|(0|3,true)=>{assert_eq!(value.full().unwrap().bits,0);if kind==3{assert!(state.flags==Some(Flags{n:false,z:true,c:false,v:false,known:15}));}},_=>assert_eq!(value.full().unwrap().bits,mask(wide)),}}}
  }}
  let mut state=State::new();statement(Instruction::Arithmetic{wide:true,subtract:true,flags:true,d:2,n:0,m:1,shift:0,amount:0},None,0,&mut state,"RVA ");assert_eq!(state.read(2,true,false).known,0);assert!(state.flags.is_none());
 }
 #[test]fn conditional_selection_retains_only_common_value_bits(){
  for wide in [false,true]{for kind in 0..4{
   let mut state=State::new();state.registers[1]=Value::integer(41);state.registers[2]=Value::integer(43);
   let _=statement(Instruction::Select{wide,kind,d:3,n:1,m:2,condition:0},None,0,&mut state,"RVA ");let value=state.read(3,wide,false);let other=match kind{0=>43,1=>44,2=>!43u64,_=>43u64.wrapping_neg()}&mask(wide);assert_eq!(value.bits&value.known,41&value.known);assert_eq!(value.bits&value.known,other&value.known);
   if kind==0{let _=statement(Instruction::LogicalImmediate{wide,kind:0,d:4,n:3,value:mask(wide)&!2},None,4,&mut state,"RVA ");assert_eq!(state.read(4,wide,false).full().unwrap().bits,41);}
   state.flags=Some(Flags{n:false,z:true,c:false,v:false,known:15});let _=statement(Instruction::Select{wide,kind,d:3,n:1,m:2,condition:0},None,0,&mut state,"RVA ");assert_eq!(state.read(3,wide,false).full().unwrap().bits,41);
  }}
  let mut state=State::new();state.registers[1]=Value::coordinate(0x1000);state.registers[2]=Value::coordinate(0x1000);let _=statement(Instruction::Select{wide:true,kind:0,d:3,n:1,m:2,condition:0},None,0,&mut state,"RVA ");assert!(state.read(3,true,false).full().unwrap().relative);state.registers[2]=Value::integer(0x1000);let _=statement(Instruction::Select{wide:true,kind:0,d:3,n:1,m:2,condition:0},None,0,&mut state,"RVA ");assert_eq!(state.read(3,true,false).known,0);
 }
 #[test]fn logical_known_bits_cover_concrete_operands(){
  for ak in 0u64..16{for av in 0u64..16{if av&!ak!=0{continue;}for bk in 0u64..16{for bv in 0u64..16{if bv&!bk!=0{continue;}for kind in 0..3{let a=Value{bits:av,known:ak|!15,relative:false};let b=Value{bits:bv,known:bk|!15,relative:false};let result=logical(a,b,true,kind);for x in 0u64..16{if x&ak!=av{continue;}for y in 0u64..16{if y&bk!=bv{continue;}let actual=match kind{0=>x&y,1=>x|y,_=>x^y};assert_eq!((actual^result.bits)&result.known,0);}}}}}}}
 }
 #[test]fn shifted_known_bits_cover_concrete_operands(){
  for wide in [false,true]{let word=mask(wide);for negative in [false,true]{let prefix=if negative{word&!15}else{0};let abstract_value=Value{bits:prefix,known:!15,relative:false};for amount in [0,1,7,15,31]{for kind in 0..4{let result=shifted(abstract_value,wide,kind,amount);for low in 0..16{let x=prefix|low;let actual=match kind{0=>x.wrapping_shl(amount as u32)&word,1=>x>>amount,2=>if wide{((x as i64)>>amount)as u64}else{((x as u32 as i32)>>amount)as u32 as u64},_=>if wide{x.rotate_right(amount as u32)}else{(x as u32).rotate_right(amount as u32)as u64}};assert_eq!((actual^result.bits)&result.known,0);}}}}}
  assert_eq!(shifted(UNKNOWN,true,2,1).known,0);assert_eq!(shifted(UNKNOWN,false,2,1).known,!0xffff_ffff);
 }
 #[test]fn logical_coordinates_and_partial_flags_preserve_meaning(){
  let coordinate=Value::coordinate(0x1000);assert!(logical(coordinate,Value::integer(0),true,2).relative);assert_eq!(logical(coordinate,Value::integer(0),true,0).full().unwrap().bits,0);assert_eq!(logical(coordinate,Value::integer(1),true,2).known,0);
  let partial=Value{bits:1,known:(1<<63)|1,relative:false};let flags=logical_flags(partial,true).unwrap();assert!(!flags.n&&!flags.z&&!flags.c&&!flags.v);let coordinate_flags=logical_flags(coordinate,true).unwrap();assert_eq!(coordinate_flags.known,3);assert_eq!(coordinate_flags.bits(),0);
 }
 #[test]fn captured_entry_join_preserves_bits_coordinates_and_unknowns(){
  let v=Value::integer(0x29).join(Value::integer(0x63));assert_eq!(v.bits,0x21);assert_eq!(v.known,0xffffffffffffffb5);assert!(v.full().is_none());
  assert_eq!(Value::coordinate(0x1000).join(Value::integer(0x1000)).known,0);assert!(Value::coordinate(0x1000).join(Value::coordinate(0x1000)).relative);assert_eq!(Value::coordinate(0x1000).join(Value::coordinate(0x1004)).known,0);assert_eq!(UNKNOWN.join(Value::integer(0)).known,0);
  let mut left=State::new();left.registers[4]=Value::integer(0x29);left.flags=Some(Flags{n:false,z:true,c:true,v:false,known:15});let mut right=left.clone();right.registers[4]=Value::integer(0x63);left.join(&right);assert_eq!(left.registers[4].known,0xffffffffffffffb5);assert!(left.flags.is_some());left.join(&State::new());assert_eq!(left.registers[4].known,0);assert!(left.flags.is_none());
 }
 #[test]fn captured_entry_common_facts_feed_successor_instructions(){
  let a=report(&[0xd2800524,0x14000002,0xd2800c64,0x92400086,0x910004c7,0xd65f03c0]);
  assert!(!a.statements[3].text.contains("X6 <- encoded integer 0x1"));assert!(a.statements[4].text.contains("X7 <- partial integer bits0x0, known mask0xfffffffffffffffc"));
  assert!(a.statements[3].path_facts.contains("Captured-entry interpretation")&&a.statements[3].path_facts.contains("X6 <- encoded integer 0x1"));
  assert!(a.statements[4].path_facts.contains("Captured-entry interpretation")&&a.statements[4].path_facts.contains("X7 <- encoded integer 0x2"));
  assert!(a.statements[3].path_facts.contains("Conditional path")&&a.statements[3].path_facts.contains("Captured-entry join"));
  assert!(a.text.contains("same-block words interpreted from common captured-entry facts"));
 }
 #[test]fn captured_entry_annotations_keep_priority_under_shared_budget(){
  let mut w=words(&[0xd2800524,0x14000002,0xd2800c64,0x92400086,0x910004c7,0xd65f03c0].repeat(16));for word in &mut w{word.1+=0x20411c;}
  let a=observe(&w,"declared VA ",&||false).unwrap();
  assert!(a.statements.iter().filter(|s|s.path_facts.contains("Captured-entry join for block")).count()>=10);
  assert!(a.statements.iter().map(|s|s.path_facts.len()).sum::<usize>()<=24*1024);assert!(a.statements.iter().all(|s|s.text.len()+s.path_facts.len()<=1600));
 }
 #[test]fn authenticated_targets_require_runtime_context(){
  let words:Vec<Word>=[0x10000043,0xd71f0864,0x91000465,0xd65f03c0].iter().enumerate().map(|(i,&word)|{let b=super::super::control_flow::decode_version(word,3);((i*4)as u64,(i*4)as u64,word,b,decode(word,b))}).collect();
  let result=observe(&words,"declared VA ",&||false).unwrap();assert!(result.text.contains("authenticated indirect jump candidate: unresolved"));assert!(result.statements[2].text.contains("X5 <- unknown"));assert!(!result.statements[2].path_facts.contains("Conditional path"));
 }
 #[test]fn locally_known_condition_selects_only_its_successor(){
  let a=report(&[0xd2800000,0xb4000040,0x91000401,0x91000802,0xd65f03c0]);assert!(!a.statements[2].path_facts.contains("edge at declared VA 0x4"));assert!(a.statements[3].path_facts.contains("edge at declared VA 0x4 (taken alternative)")&&a.statements[3].path_facts.contains("encoded integer 0x2"));
 }
 #[test]fn source_coordinates_remain_distinct_from_integer_values(){
  let a=observe(&words(&[0x10000040,0x14000002,0xd503201f,0x91000401,0xd65f03c0]),"RVA ",&||false).unwrap();let line=format!("{}{}",a.statements[3].text,a.statements[3].path_facts);assert!(line.contains("encoded source coordinate RVA 0x9"));assert!(!line.contains("encoded integer 0x9"));
 }
 #[test]fn no_iterative_loop_or_unavailable_target_is_invented(){
  let a=report(&[0xd28000e0,0x17ffffff]);assert!(a.text.contains("1 captured edges examined"));let a=report(&[0xd28000e0,0x14000400]);assert!(a.text.contains("0 captured edges examined"));
 }
 #[test]fn path_work_and_annotation_storage_are_bounded(){
  let mut data=vec![0x14000001;512];data[0]=0xd28000e0;let a=report(&data);assert!(a.text.contains("64 captured edges examined"));assert!(a.text.len()<OUTPUT);assert!(a.statements.iter().all(|s|s.text.len()+s.path_facts.len()<=1600));assert!(a.statements.iter().map(|s|s.path_facts.len()).sum::<usize>()<=24*1024);assert!(observe(&words(&vec![0xd503201f;513]),"RVA ",&||false).is_err());
 }
 #[test]fn conditional_path_cancellation_is_cooperative(){
  let w=words(&[0xd28000e0,0x14000001,0x91000401]);let mut a=report(&[0xd28000e0,0x14000001,0x91000401]);let mut state=State::new();let _=statement(w[0].4,w[0].3,0,&mut state,"RVA ");let mut out=Text{body:String::new(),rows:0,omitted:0};assert!(conditional_paths(&w,&[(1,0,state)],&mut a.statements,"RVA ",&mut out,&||true).unwrap_err().contains("cancelled"));
 }
}
