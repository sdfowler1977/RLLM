//! Bounded foreground PCM segmentation; recognition remains a configured capability.
use super::*;
use std::collections::VecDeque;
use std::sync::{Mutex,OnceLock};
use std::time::{Duration,Instant};
const FRAME:usize=640; // 20 ms, mono signed 16-bit little endian at 16 kHz.
struct Segmenter{pending:Vec<u8>,preroll:VecDeque<u8>,audio:Vec<u8>,hot:usize,quiet:usize,noise:u64,threshold:u64,onset:usize,silence:usize,limit:usize}
impl Segmenter{
 fn new(settings:&Json)->Result<Self>{let silence=number(settings,"silence_ms");let threshold=number(settings,"threshold");let seconds=number(settings,"max_seconds");if !(300..=3000).contains(&silence)||!(100..=5000).contains(&threshold)||!(5..=30).contains(&seconds){return Err("Voice settings require a 300–3000 ms pause, sensitivity level100–5000 and5–30 seconds per utterance".into());}Ok(Self{pending:vec![],preroll:VecDeque::new(),audio:vec![],hot:0,quiet:0,noise:0,threshold:(threshold*threshold)as u64,onset:12,silence:(silence+19)/20,limit:seconds*32000})}
 fn push(&mut self,bytes:&[u8])->Result<Option<Vec<u8>>>{
  if bytes.is_empty()||bytes.len()>FRAME||bytes.len()%2!=0{return Err("Microphone input requires1–320 complete16-bit samples".into());}self.pending.extend_from_slice(bytes);if self.pending.len()<FRAME{return Ok(None);}let frame:Vec<u8>=self.pending.drain(..FRAME).collect();let mut sum=0i64;let mut squares=0u64;for sample in frame.chunks_exact(2){let n=i16::from_le_bytes(sample.try_into().unwrap())as i64;sum+=n;squares+=(n*n)as u64;}let mean=sum/320;let energy=(squares/320).saturating_sub((mean*mean)as u64);let speech=energy>self.threshold.max(self.noise.saturating_mul(4));
  if self.audio.is_empty(){self.preroll.extend(&frame);while self.preroll.len()>(15+self.onset)*FRAME{self.preroll.pop_front();}if speech{self.hot+=1;}else{self.hot=0;self.noise=(self.noise*31+energy.min(self.threshold))/32;}if self.hot>=self.onset{self.audio.extend(self.preroll.drain(..));self.quiet=0;}}
  else{self.audio.extend(&frame);if speech{self.quiet=0;}else{self.quiet+=1;}}
  if !self.audio.is_empty()&&(self.quiet>=self.silence||self.audio.len()>=self.limit){let pcm=std::mem::take(&mut self.audio);return Ok(Some(wave(&pcm)?));}Ok(None)
 }
}
fn wave(pcm:&[u8])->Result<Vec<u8>>{if pcm.is_empty()||pcm.len()>1024*1024||pcm.len()%2!=0{return Err("Voice utterance exceeds its PCM bound".into());}let mut bytes=Vec::with_capacity(44+pcm.len());bytes.extend(b"RIFF");bytes.extend(&(pcm.len()as u32+36).to_le_bytes());bytes.extend(b"WAVEfmt ");bytes.extend(&16u32.to_le_bytes());bytes.extend(&1u16.to_le_bytes());bytes.extend(&1u16.to_le_bytes());bytes.extend(&16000u32.to_le_bytes());bytes.extend(&32000u32.to_le_bytes());bytes.extend(&2u16.to_le_bytes());bytes.extend(&16u16.to_le_bytes());bytes.extend(b"data");bytes.extend(&(pcm.len()as u32).to_le_bytes());bytes.extend(pcm);Ok(bytes)}
struct Capture{session:String,playback:String,onset:Option<String>,notified:bool,created:Instant,engine:Segmenter,ready:Option<(String,Vec<u8>)>,speech:Option<super::voice::duplex_speech::Session>}
static CAPTURES:OnceLock<Mutex<BTreeMap<String,Capture>>>=OnceLock::new();
static STOPPED:OnceLock<Mutex<Vec<(String,Instant)>>>=OnceLock::new();
fn captures()->&'static Mutex<BTreeMap<String,Capture>>{CAPTURES.get_or_init(||Mutex::new(BTreeMap::new()))}
fn stopped()->&'static Mutex<Vec<(String,Instant)>>{STOPPED.get_or_init(||Mutex::new(Vec::new()))}
pub(super) fn validate(settings:&Json)->Result<()>{Segmenter::new(settings).map(|_|())}
fn open_mode(session:&str,settings:&Json,playback:&str)->Result<String>{open_with(session,settings,playback,None)}
fn open_with(session:&str,settings:&Json,playback:&str,speech:Option<super::voice::duplex_speech::Session>)->Result<String>{let engine=Segmenter::new(settings)?;let mut held=captures().lock().map_err(|_|"Voice capture registry unavailable")?;held.retain(|_,c|c.created.elapsed()<Duration::from_secs(600));if held.len()>=4{return Err("Voice capture slots are occupied".into());}let ticket=format!("voice-audio:{}",now());held.insert(ticket.clone(),Capture{session:session.into(),playback:playback.into(),onset:None,notified:false,created:Instant::now(),engine,ready:None,speech});Ok(ticket)}
pub(super) fn open_speech(session:&str,settings:&Json,speech:super::voice::duplex_speech::Session)->Result<String>{open_with(session,settings,"",Some(speech))}
pub(super) fn is_speech(ticket:&str)->bool{captures().lock().ok().is_some_and(|held|held.get(ticket).is_some_and(|c|c.speech.is_some()))}
pub(super) fn speech_caption(session:&str)->String{captures().lock().ok().and_then(|held|held.values().find(|c|c.session==session&&c.speech.is_some()).and_then(|c|c.speech.as_ref().map(|speech|speech_preview(speech)))).unwrap_or_default()}
fn speech_preview(speech:&super::voice::duplex_speech::Session)->String{speech.caption()}
pub(super) fn open(session:&str,settings:&Json)->Result<String>{open_mode(session,settings,"")}
pub(super) fn watch(session:&str,settings:&Json,playback:&str)->Result<String>{if playback.is_empty(){return Err("Spoken interruption requires current playback ownership".into());}open_mode(session,settings,playback)}
pub(super) fn owns(ticket:&str,session:&str)->bool{captures().lock().ok().is_some_and(|held|held.get(ticket).is_some_and(|c|c.session==session))}
pub(super) fn receipt_owned(ticket:&str,session:&str,receipt:&str)->bool{captures().lock().ok().is_some_and(|held|held.get(ticket).is_some_and(|c|c.session==session&&c.playback.is_empty()&&c.ready.as_ref().is_some_and(|(r,_)|r==receipt)))}
pub(super) fn watch_owned(ticket:&str,session:&str,playback:&str,onset:Option<&str>)->bool{captures().lock().ok().is_some_and(|held|held.get(ticket).is_some_and(|c|c.session==session&&!playback.is_empty()&&c.playback==playback&&c.created.elapsed()<Duration::from_secs(600)&&onset.map(|r|c.onset.as_deref()==Some(r)).unwrap_or(true)))}
pub(super) fn promote(ticket:&str,session:&str,playback:&str,onset:&str)->Result<()>{let mut held=captures().lock().map_err(|_|"Voice capture registry unavailable")?;let c=held.get_mut(ticket).filter(|c|c.session==session&&!playback.is_empty()&&c.playback==playback&&c.onset.as_deref()==Some(onset)&&c.created.elapsed()<Duration::from_secs(600)).ok_or("Spoken interruption ownership changed")?;c.playback.clear();Ok(())}
fn note_stopped(ticket:&str){if let Ok(mut held)=stopped().lock(){held.retain(|(_,at)|at.elapsed()<Duration::from_secs(30));held.push((ticket.to_string(),Instant::now()));}}
pub(super) fn cancel_capture(ticket:&str,session:&str){if let Ok(mut held)=captures().lock(){if held.get(ticket).is_some_and(|c|c.session==session){if held.get(ticket).is_some_and(|c|c.speech.is_some()){note_stopped(ticket);}held.remove(ticket);}}}
pub(super) fn cancel_session(session:&str){if let Ok(mut held)=captures().lock(){let gone:Vec<String>=held.iter().filter(|(_,c)|c.session==session&&c.speech.is_some()).map(|(ticket,_)|ticket.clone()).collect();for ticket in gone{note_stopped(&ticket);}held.retain(|_,c|c.session!=session);}}
pub(super) fn take_speech_text(session:&str)->String{let Ok(mut held)=captures().lock()else{return String::new();};let mut out=String::new();for capture in held.values_mut(){if capture.session==session{if let Some(speech)=capture.speech.as_mut(){out.push_str(&speech.take_text());}}}out}
pub(super) fn take(ticket:&str,session:&str,receipt:&str)->Result<Vec<u8>>{let mut held=captures().lock().map_err(|_|"Voice capture registry unavailable")?;let capture=held.get(ticket).filter(|c|c.session==session&&c.playback.is_empty()&&c.created.elapsed()<Duration::from_secs(600)).ok_or("Voice capture expired, changed or still belongs to playback")?;if !capture.ready.as_ref().is_some_and(|(r,_)|r==receipt){return Err("Voice utterance receipt differs".into());}Ok(held.remove(ticket).unwrap().ready.unwrap().1)}
pub(super) fn operation(v:&Json)->Result<Json>{let ticket=field(v,"voice_audio_ticket")?;let session=field(v,"voice_session")?;let mut held=captures().lock().map_err(|_|"Voice capture registry unavailable")?;
 if field(v,"op")?=="voice_audio_cancel"{if held.get(ticket).is_some_and(|c|c.session==session){if held.get(ticket).is_some_and(|c|c.speech.is_some()){note_stopped(ticket);}held.remove(ticket);}return Ok(Null);}
 if held.get(ticket).is_none()&&stopped().lock().ok().is_some_and(|gone|gone.iter().any(|(id,at)|id==ticket&&at.elapsed()<Duration::from_secs(30))){return Ok(obj(vec![("ready",Bool(false)),("speech",Bool(true)),("stopped",Bool(true))]));}
 if !voice::token_live(session){return Err("Voice microphone consent expired or was stopped".into());}let capture=held.get_mut(ticket).filter(|c|c.session==session&&c.created.elapsed()<Duration::from_secs(600)).ok_or("Microphone ticket differs or expired")?;if capture.ready.is_some(){return Err("Voice utterance is already complete".into());}if field(v,"op")?=="voice_audio_check"{return Ok(Bool(true));}let encoded=field(v,"data")?;if encoded.len()>856{return Err("Microphone frame exceeds20 milliseconds".into());}let bytes=network::unbase64(encoded)?;
 if capture.speech.is_some(){return speech_feed(capture,&bytes);}
fn speech_feed(capture:&mut Capture,bytes:&[u8])->Result<Json>{
 if bytes.len()!=640||bytes.len()%2!=0{return Err("Full-duplex speech expects 20 ms of 16 kHz microphone audio".into());}
 let input:Vec<i16>=bytes.chunks_exact(2).map(|sample|i16::from_le_bytes([sample[0],sample[1]])).collect();
 let wide=super::voice::opus::upsample_16_to_24(&input)?;
 let speech=capture.speech.as_mut().ok_or("Speech session is not open")?;
 let heard=speech.push(&wide)?;
 let narrow=super::voice::opus::downsample_24_to_16(&heard.pcm).unwrap_or_default();
 let playback=le_bytes(&narrow);let wide_bytes=le_bytes(&heard.pcm);
 Ok(obj(vec![("ready",Bool(false)),("speech",Bool(true)),("text",s(&heard.text)),("playback",s(&network::b64(&playback))),("playback_rate",Num("16000".into())),("playback24",s(&network::b64(&wide_bytes))),("speech_rate",Num("24000".into())),("dropped_samples",Num(heard.dropped.to_string()))]))
}
fn le_bytes(samples:&[i16])->Vec<u8>{let mut out=Vec::with_capacity(samples.len()*2);for sample in samples{out.extend(sample.to_le_bytes());}out}
 let wav=capture.engine.push(&bytes)?;
 if !capture.playback.is_empty()&&capture.onset.is_none()&&capture.engine.hot>=capture.engine.onset{capture.onset=Some(format!("voice-onset:{}",now()));}
 let mut out=obj(vec![("ready",Bool(wav.is_some()))]);if let Some(wav)=wav{let receipt=format!("voice-utterance:{}",now());capture.ready=Some((receipt.clone(),wav));put(&mut out,"receipt",s(&receipt));}
 if !capture.notified{if let Some(onset)=&capture.onset{put(&mut out,"onset_receipt",s(onset));capture.notified=true;}}Ok(out)
}

#[cfg(test)]mod checks{
 use super::*;
 fn engine()->Segmenter{Segmenter::new(&obj(vec![("silence_ms",Num("800".into())),("threshold",Num("500".into())),("max_seconds",Num("5".into()))])).unwrap()}
 fn tone()->Vec<u8>{(0..320).flat_map(|n|(if n%2==0{3000i16}else{-3000i16}).to_le_bytes()).collect()}
 #[test]fn silence_and_short_noise_never_emit_audio(){let mut e=engine();for _ in 0..1000{assert!(e.push(&[0;FRAME]).unwrap().is_none());}for _ in 0..5{assert!(e.push(&tone()).unwrap().is_none());}for _ in 0..100{assert!(e.push(&[0;FRAME]).unwrap().is_none());}assert!(e.audio.is_empty());assert!(e.preroll.len()<18000);}
 #[test]fn utterance_preserves_original_pcm_and_valid_wave_length(){let mut e=engine();for _ in 0..20{e.push(&[0;FRAME]).unwrap();}for _ in 0..20{assert!(e.push(&tone()).unwrap().is_none());}let mut ready=None;for _ in 0..40{if let Some(w)=e.push(&[0;FRAME]).unwrap(){ready=Some(w);}}let w=ready.unwrap();assert_eq!(&w[..4],b"RIFF");assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap())as usize+8,w.len());assert!(w.windows(FRAME).any(|p|p==tone()));}
 #[test]fn continuous_audio_and_bad_frames_are_bounded(){let mut e=engine();assert!(e.push(&[0]).is_err());assert!(e.push(&[0;FRAME+2]).is_err());let mut ready=None;for _ in 0..300{if let Some(w)=e.push(&tone()).unwrap(){ready=Some(w);break;}}assert!(ready.unwrap().len()<=5*32000+FRAME+44);}
}
