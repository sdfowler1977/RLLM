//! Bounded, non-executing file and web evidence tools.
use super::*;
#[path="model_artifact.rs"] mod model_artifact;
#[path="dex.rs"] pub(crate) mod dex;
pub(super) const NATIVE_CAPABILITIES:&str="Workspace investigation uses the shared inspection, comparison and retained-evidence paths. inspect/compare content:complete_digest streams complete selected regular files in64KiB chunks and records exact length/SHA256. File/folder observations retain source identity, captured byte ranges, typed nodes and encoded relationships. observations pages saved records and nodes. reconstruct uses current source identity to create selected file, representation or structural outputs through review and readback.\nSupported observations include JSON and CSV/TSV structure, Unicode text, decoded PNG pixels/alpha, RIFF/WAVE, WebAssembly section envelopes, Rust declarations, JavaScript/Electron module graphs, package.json entries, ASAR listings, route and IPC string sites, and source-map references, DEX identifiers/classes/members, GGUF and safetensors metadata/tensor layouts, ELF headers/mappings/symbols/dependencies/relocations, PE headers/sections/imports/base-relocation declarations, HTML/CSS structure and cascade, static page text and rendered PNG capture.\nAArch64 observations retain encoded branches, candidate blocks, register/SP/NZCV facts, integer/address/memory expressions and separately labelled predecessor interpretations. Coordinates identify declared source VAs or PE RVAs. PE base-relocation rows include source entry locations, raw types, page/offset RVAs, paired HIGHADJ words, known field widths and captured target mappings. Coverage, captured prefixes and retained/omitted counts accompany each observation.\nPage capture uses the selected font, dimensions and resource settings. scripts:inline runs the supported inline-program/DOM operations transactionally through the existing reviewed page path. Use the operation reference and returned fields to choose a focused next step. Keep source observations, interpretations and implementation decisions clearly labelled.";
#[path="directory_observation.rs"] pub(super) mod directory_observation;
#[path="directory_comparison.rs"] mod directory_comparison;
pub(super) fn inspect_scoped(root:&Path,p:&Path,relative:&str,access:&operation::InspectionAccess)->Result<String>{
    if !p.is_dir(){return inspect_using(p,&||access.cancelled(),(root,relative,access));}
    let request=obj(vec![("tool",s("inspect")),("args",obj(vec![("path",s(relative))]))]);
    let report=directory_report(root,relative,access,&request)?;let display=report.describe(120000.min(access.budget.saturating_sub(256*1024)));
    Ok(if access.graph{observation_graph::envelope(display,observation_graph::snapshot(&report).map(|g|vec![g]),access.budget)}else{display})
}
pub(super) fn compare_scoped(root:&Path,left:&Path,right:&Path,args:&Json,access:&operation::InspectionAccess)->Result<String>{
    if !left.is_dir()&&!right.is_dir(){return comparison::compare_scoped(root,left,right,args,access);}
    if !left.is_dir()||!right.is_dir(){return Err("Select two files or two folders for comparison".into());}
    let request=obj(vec![("tool",s("compare")),("args",args.clone())]);
    let initial=harness_rules::digest(&harness_rules::worker_policy(&request)?);
    let mut half=access.clone();half.budget=access.budget/2;
    let a=directory_report(root,&text(args,"path"),&half,&request)?;
    let b=directory_report(root,&text(args,"other"),&half,&request)?;
    if harness_rules::digest(&harness_rules::worker_policy(&request)?)!=initial{return Err("Harness rules changed during folder comparison; request a fresh comparison".into());}
    let display=directory_comparison::describe(&a,&b,120000.min(access.budget/4),&||access.cancelled())?;
    Ok(if access.graph{observation_graph::envelope(display,observation_graph::snapshot(&a).and_then(|a|observation_graph::snapshot(&b).map(|b|vec![a,b])),access.budget)}else{display})
}
fn directory_report(root:&Path,relative:&str,access:&operation::InspectionAccess,request:&Json)->Result<directory_observation::Report>{
    use directory_observation::{Access,Decision,Limits};
    let policy_identity=harness_rules::digest(&harness_rules::worker_policy(&request)?);
    let available=access.budget.saturating_sub(256*1024);
    if available<1024{return Err("Directory observation exceeds the current response memory budget".into());}
    let mut limits=Limits::default();
    limits.report_bytes=limits.report_bytes.min(available);
    limits.total_sample_bytes=limits.total_sample_bytes.min(limits.report_bytes/2);
    let mut policy=|path:&Path,kind:Access|{
        let (tool,enabled)=match kind{Access::ListDirectory=>("list",access.list),Access::ReadHeader=>("read",access.read)};
        if !enabled{return Decision::Deny;}
        let Some(path)=path.to_str() else{return Decision::Deny;};
        let request=obj(vec![("tool",s(tool)),("args",obj(vec![("path",s(path))]))]);
        match harness_rules::worker_policy(&request){
            Ok(global)=>if harness_rules::worker_review_required(&global,&request){Decision::Ask}else{Decision::Allow},
            Err(_)=>Decision::Deny,
        }
    };
    // A reviewed dispatch binds the opened directory to the approved workspace identity.
    // Ticket-less engine calls have no identity to bind; they use the same limits and policy.
    let report=if let Some(identity)=access.root_identity{directory_observation::observe_bound(root,Path::new(relative),Some(identity),&limits,&mut policy,&||access.cancelled())?}else{directory_observation::observe(root,Path::new(relative),&limits,&mut policy,&||access.cancelled())?};
    if harness_rules::digest(&harness_rules::worker_policy(&request)?)!=policy_identity{return Err("Harness rules changed during directory observation; request a fresh inspection".into());}
    Ok(report)
}
/// Inspect one regular file through its reviewed workspace source. The source
/// is opened under the current policy guard before any byte is read.
fn inspect_using(p:&Path,cancelled:&impl Fn()->bool,scope:(&Path,&str,&operation::InspectionAccess))->Result<String>{
    let (root,relative,access)=scope;
    let source=inspect_projected(root,relative,access)?;
    inspect(p,&source,cancelled,scope)
}
/// Open the reviewed workspace source of the selected file.
fn inspect_projected<'a>(root:&Path,relative:&str,access:&'a operation::InspectionAccess)->Result<file_evidence::Source<'a>>{
    file_evidence::Source::open(root,relative,"inspect",&obj(vec![("path",s(relative))]),access)
}
fn map_file_status(root:&Path,module_relative:&str,target:&str,access:&operation::InspectionAccess)->super::super::js_evidence::MapFile{
    let Some(relative)=resolve_map_relative(module_relative,target) else{return super::super::js_evidence::MapFile::Unreadable;};
    if super::file_evidence::normalize(&relative).is_err(){return super::super::js_evidence::MapFile::Unreadable;}
    let args=obj(vec![("path",s(&relative))]);
    match super::file_evidence::Source::open(root,&relative,"inspect",&args,access){
        Ok(_)=>super::super::js_evidence::MapFile::Present,
        Err(error) if error.contains("os error 2")||error.contains("No such file")=>super::super::js_evidence::MapFile::Absent,
        Err(_)=>super::super::js_evidence::MapFile::Unreadable,
    }
}
fn resolve_map_relative(module_relative:&str,target:&str)->Option<String>{
    if target.is_empty()||target.starts_with('/')||target.contains('\\')||target.contains('\0')||target.contains(':'){return None;}
    let mut parts:Vec<&str>=module_relative.rsplit_once('/').map(|(dir,_)|dir).unwrap_or("").split('/').filter(|part|!part.is_empty()).collect();
    for part in target.split('/'){
        match part{
            ""|"."=>{},
            ".."=>{parts.pop()?;}
            other=>parts.push(other),
        }
    }
    if parts.is_empty(){None}else{Some(parts.join("/"))}
}
/// Inspect one reviewed regular file and return its bounded static observation.
/// The opened source is the reviewed workspace file: its policy guard and
/// component bindings are re-checked on every read step.
pub(super) fn inspect(p:&Path,source:&file_evidence::Source<'_>,cancelled:&impl Fn()->bool,scope:(&Path,&str,&operation::InspectionAccess))->Result<String>{
    let mut f=source.file()?;
    let meta=f.metadata().map_err(|e|e.to_string())?;
    if !meta.is_file(){return Err("Inspection requires a regular file".into());}
    let mut data=Vec::new();let mut block=[0u8;65536];
    while data.len()<4*1024*1024+1 {
        if cancelled(){return Err("Inspection cancelled".into());}source.check()?;
        let remaining=(4*1024*1024+1-data.len()).min(block.len());
        let n=f.read(&mut block[..remaining]).map_err(|e|e.to_string())?;
        if n==0{break;}data.extend_from_slice(&block[..n]);
    }
    source.check()?;let after=f.metadata().map_err(|e|e.to_string())?;
    if meta.len()!=after.len()||meta.modified().ok()!=after.modified().ok(){return Err("File changed while being inspected; request a fresh observation".into());}
    let complete=data.len()<=4*1024*1024;if complete&&data.len() as u64!=after.len(){return Err("File length changed while being inspected".into());}data.truncate(4*1024*1024);
    let mut structure=super::exploration::analyze(&data,complete);let mut binary=None;
    if p.extension().and_then(|v|v.to_str())==Some("rs") {
        structure.push('\n');structure.push_str(&super::rust_source_observation::analyze(&data,complete));
    }
    if let Some(kind)=super::super::js_evidence::node_kind(p) {
        if cancelled(){return Err("Inspection cancelled".into());}
        let (root,relative,access)=scope;
        let text=super::file_evidence::javascript_observation(&data,meta.len(),complete,kind,relative,&|target| Some(map_file_status(root,relative,target,access)));
        structure.push('\n');structure.push_str(&text);
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        structure.push('\n');
        if complete{match super::image_png::decode_png(&data,&super::image_png::Limits::default()){
            Ok(image)=>{let transparent=image.rgba.chunks_exact(4).filter(|p|p[3]==0).count();let partial=image.rgba.chunks_exact(4).filter(|p|p[3]>0&&p[3]<65535).count();structure.push_str(&format!("Native PNG decode: {}x{}, source sample depth {} bits; {} decoded straight-alpha RGBA16 samples. Fully transparent pixels: {transparent}; partially transparent pixels: {partial}. {}; {}. No image inference or visual meaning is asserted.",image.width,image.height,image.source_depth,image.rgba.len(),image.color_status,image.animation_status));},
            Err(error)=>structure.push_str(&format!("Native PNG decoding refused: {error}")),
        }}else{structure.push_str("PNG prefix only; complete decoding was not attempted.");}
    }
    if let Some(observation)=dex::analyze_with_cancel(&data,complete,after.len(),cancelled) {
        if cancelled(){return Err("Inspection cancelled".into());}
        structure.push('\n');structure.push_str(&observation);
    }
    if let Some(observation)=model_artifact::analyze(&data,complete,after.len(),p.extension().and_then(|v|v.to_str())==Some("safetensors")) {
        structure.push('\n');structure.push_str(&observation);
    }
    let graph=scope.2.graph;
    if data.starts_with(b"MZ") {
        if graph{let observation=super::pe_observation::observe_with_cancel(&data,complete,after.len(),cancelled)?;structure.push('\n');structure.push_str(&observation.text);binary=Some(observation);}
        else{let text=super::pe_observation::analyze(&data,complete,after.len(),cancelled)?;structure.push('\n');structure.push_str(&text);}
        if cancelled(){return Err("Inspection cancelled".into());}
        // Observed carries a parse, including a malformed-image refusal.
        // Bounded is a cap refusal. Err is cancellation. The sentences in the text are unchanged.
        match super::super::dotnet_evidence::cli_evidence(&data, cancelled) {
            Ok(super::super::dotnet_evidence::CliText::Observed(dotnet) | super::super::dotnet_evidence::CliText::Bounded(dotnet)) => {
                structure.push('\n');
                structure.push_str(&dotnet);
            }
            Ok(super::super::dotnet_evidence::CliText::Absent) => {}
            Err(()) => return Err("Inspection cancelled".into()),
        }
    }
    if data.starts_with(b"\x7fELF") {
        if graph{let observation=super::elf_observation::observe_with_cancel(&data,complete,after.len(),cancelled)?;structure.push('\n');structure.push_str(&observation.text);binary=Some(observation);}
        else{let text=super::elf_observation::analyze_with_cancel(&data,complete,after.len(),cancelled)?;structure.push('\n');structure.push_str(&text);}
    }
    if matches!(p.extension().and_then(|v|v.to_str()),Some("html"|"htm")) {
        structure.push('\n');let document=super::html_document::parse(&data,complete);structure.push_str(&document.summary());
        structure.push('\n');structure.push_str(&page_geometry(&document));
    }
    if p.extension().and_then(|v|v.to_str())==Some("css") {
        structure.push('\n');let tokens=super::css_syntax::tokenize(&data,complete);structure.push_str(&tokens.summary());structure.push('\n');structure.push_str(&super::css_stylesheet::parse(&tokens).summary());
    }
    if [b"RIFF".as_slice(),b"RIFX",b"RF64",b"\0asm"].iter().any(|magic|data.starts_with(magic)) {
        structure.push('\n');structure.push_str(&super::binary_observation::analyze(&data,complete,after.len()));
    }
    // Recorders write HAR and SAZ in either ASCII case. The same parser still decides rows or a named refusal.
    // Source, page, and stylesheet extensions stay exact, so notes.TXT keeps today's ordinary display.
    if p.extension().and_then(|v|v.to_str()).is_some_and(|v|v.eq_ignore_ascii_case("har")||v.eq_ignore_ascii_case("saz")){
        if cancelled(){return Err("Inspection cancelled".into());}
        let (root,relative,access)=scope;
        let capture=file_evidence::saved_file_capture(root,relative,access)?;
        structure.push('\n');
        structure.push_str(&super::super::har_evidence::file_section(&capture));
    }
    if data.starts_with(&[0xCF, 0xFA, 0xED, 0xFE]) || data.starts_with(&[0xFE, 0xED, 0xFA, 0xCF]) {
        structure.push('\n');
        structure.push_str(&super::super::mach_image::mach_image(&data));
        structure.push('\n');
        structure.push_str(&super::super::mach_libraries::mach_libraries(&data));
        if data.starts_with(&[0xCF, 0xFA, 0xED, 0xFE]) {
            structure.push('\n');
            structure.push_str(&super::super::mach_symbols::mach_symbols(&data));
            structure.push('\n');
            structure.push_str(&super::super::mach_text::mach_text(&data));
            structure.push('\n');
            structure.push_str(&super::super::arm_branch::arm_branch(&data));
        }
    }
    if data.len() >= 4 && u32::from_le_bytes([data[0], data[1], data[2], data[3]]) == 0x0008_0003 {
        structure.push('\n');
        structure.push_str(&super::super::manifest_pool::manifest_pool(&data));
    }
    if data.starts_with(b"TRC1") {
        structure.push('\n');
        structure.push_str(&super::super::trace_record::trace_record(&data));
    }
    if data.starts_with(b"DEV1") {
        structure.push('\n');
        structure.push_str(&super::super::device_record::device_record(&data));
    }
    if data.starts_with(b"KEY1") {
        structure.push('\n');
        structure.push_str(&super::super::record_plain::record_plain(&data));
    }
    if data.starts_with(b"EVB1\n") {
        structure.push('\n');
        structure.push_str(&super::super::evidence_bundle::evidence_bundle_bytes(&data));
    }
    if data.starts_with(b"TFL3") {
        structure.push('\n');
        structure.push_str(&super::super::model_graph::model_graph(&data));
    }
    if data.starts_with(b"TGR1") {
        structure.push('\n');
        structure.push_str(&super::super::tensor_edges::tensor_edges(&data));
    }
    if data.starts_with(b"dex\n") {
        structure.push('\n');
        structure.push_str(&super::super::dex_strings::dex_strings(&data));
        structure.push('\n');
        structure.push_str(&super::super::dex_use::dex_use(&data));
        structure.push('\n');
        structure.push_str(&super::super::dex_source::dex_source(&data));
    }
    if data.starts_with(b"PK\x03\x04") {
        structure.push('\n');
        structure.push_str(&super::super::zip_members::zip_members(&data));
        structure.push('\n');
        structure.push_str(&super::super::saved_archive::saved_archive(&data));
    }
    if data.starts_with(b"MZ") || data.starts_with(b"\x7fELF") || data.starts_with(b"dex\n") || data.starts_with(b"PK\x03\x04") {
        let near = &data[..data.len().min(1 << 20)];
        structure.push('\n');
        structure.push_str(&super::super::string_near_call::string_near_call(near));
        structure.push('\n');
        structure.push_str(&super::super::text_catalog::text_catalog(near, 6));
    }
    if data.starts_with(b"MZ") || (data.len() >= 20 && data.starts_with(b"\x7fELF") && data[5] == 1 && matches!(u16::from_le_bytes([data[18], data[19]]), 3 | 62)) {
        structure.push('\n');
        structure.push_str(&super::super::reach_edges::reach_edges(&data[..data.len().min(1 << 20)]));
    }
    if data.len() >= 20 && data.starts_with(b"\x7fELF") && data[5] == 1 && u16::from_le_bytes([data[18], data[19]]) == 183 {
        structure.push('\n');
        structure.push_str(&super::super::arm_branch::arm_branch(&data));
        structure.push('\n');
        structure.push_str(&super::super::arm_text::arm_text(&data));
    }
    if !data.starts_with(b"MZ") && data.windows(4).take(61).any(|w| w == b"BSJB") {
        structure.push('\n');
        structure.push_str(&super::super::assembly_tables::assembly_tables(&data));
    }
    let format=if data.starts_with(b"\x7fELF"){"ELF"}else if data.starts_with(b"MZ"){"DOS/PE candidate"}else if data.starts_with(b"dex\n"){"DEX"}else if data.starts_with(b"PK\x03\x04"){"ZIP"}else if data.starts_with(b"\0asm"){"WebAssembly"}else if data.starts_with(b"\x89PNG\r\n\x1a\n"){"PNG"}else if data.starts_with(b"%PDF-"){"PDF"}else if std::str::from_utf8(&data).is_ok(){"UTF-8 text"}else{"Unknown binary"};
    if format == "Unknown binary" {
        let window = &data[..data.len().min(4096)];
        let catalog = &data[..data.len().min(1 << 20)];
        structure.push('\n');
        structure.push_str(&super::super::lifted_block::lifted_block(window, false));
        structure.push('\n');
        structure.push_str(&super::super::call_sites::call_sites(window));
        structure.push('\n');
        structure.push_str(&super::super::block_trace::block_trace(&data[..data.len().min(256)], false));
        structure.push('\n');
        structure.push_str(&super::super::statement_source::statement_source(&data[..data.len().min(256)], false));
        structure.push('\n');
        structure.push_str(&super::super::reach_edges::reach_edges(window));
        structure.push('\n');
        structure.push_str(&super::super::text_catalog::text_catalog(catalog, 4));
        structure.push('\n');
        structure.push_str(&super::super::string_near_call::string_near_call(catalog));
        structure.push('\n');
        structure.push_str(&super::super::datagram_fields::datagram_fields(&data[..data.len().min(512)]));
        structure.push('\n');
        structure.push_str(&super::super::observer_index::observer_index("listing"));
        structure.push('\n');
        structure.push_str(&super::super::evidence_pack::evidence_listing(&[&data[..data.len().min(256)]]));
    }
    if data.starts_with(b"RIFF") {
        if let Some(fmt) = data.windows(4).position(|mark| mark == b"fmt ") {
            if fmt + 16 <= data.len() && u16::from_le_bytes([data[fmt + 8], data[fmt + 9]]) == 1 {
                let rate = u32::from_le_bytes([data[fmt + 12], data[fmt + 13], data[fmt + 14], data[fmt + 15]]);
                if let Some(tag) = data.windows(4).position(|mark| mark == b"data") {
                    let start = tag + 8;
                    if start <= data.len() {
                        let end = start.saturating_add(65536).min(data.len());
                        structure.push('\n');
                        structure.push_str(&super::super::tone_scan::tone_scan(&data[start..end], rate, 19000));
                        structure.push('\n');
                        structure.push_str(&super::super::spectrum_bins::spectrum_bins(&data[start..end], rate));
                    }
                }
            }
        }
    }
    if p.extension().and_then(|v| v.to_str()) == Some("js") {
        if let Ok(source) = std::str::from_utf8(&data) {
            structure.push('\n');
            structure.push_str(&super::super::script_text::script_text(source));
            if source.len() > 40 && !source.contains('\n') {
                structure.push('\n');
                structure.push_str(&super::super::script_layout::script_layout(source));
            }
        }
    }
    if let Ok(source) = std::str::from_utf8(&data) {
        if source.contains("\"entries\"") && source.contains("\"url\": ") {
            let rows = super::super::session_rows::session_rows(&data);
            if !rows.starts_with("A saved session") {
                structure.push('\n');
                structure.push_str(&rows);
                structure.push('\n');
                structure.push_str(&super::super::session_env::session_env(&data));
            }
        }
    }
    let mut counts=[0usize;256];for b in &data{counts[*b as usize]+=1;}
    let entropy=counts.iter().filter(|&&n|n>0).map(|&n|{let p=n as f64/data.len() as f64;-p*p.log2()}).sum::<f64>();
    let strings = super::super::ascii_runs::ascii_runs(&data);
    let elf=if format=="ELF"&&data.len()>=20{let machine=if data[5]==1{u16::from_le_bytes([data[18],data[19]])}else{u16::from_be_bytes([data[18],data[19]])};format!("\nELF class byte: {}; endian byte: {}; machine: {machine}",data[4],data[5])}else{String::new()};
    if cancelled(){return Err("Inspection cancelled before publication".into());}source.check()?;
    let mut display=format!("Observed static inspection\nSource: {}\nFile size: {} bytes\nSampled: {} bytes\nSignature: {format}{elf}\nSample Shannon entropy: {entropy:.4} bits/byte\nFirst bytes: {}\nASCII strings (up to 160; 256 bytes each):\n{}\n\n{structure}\nSignatures and symbolic instruction IR describe the captured bytes. File sizes and sampled byte counts identify the observation coverage.",p.display(),meta.len(),data.len(),data.iter().take(32).map(|b|format!("{b:02x}")).collect::<Vec<_>>().join(" "),strings);
    if data.starts_with(b"MZ") {
        display.push('\n');
        display.push_str(&super::super::call_sites::pe_code_sites(&data));
        if data.get(0x3c..0x40).and_then(|raw| raw.try_into().ok()).map(u32::from_le_bytes).map(|at| at as usize).is_some_and(|pe| {
            data.get(pe..pe.saturating_add(6)).is_some_and(|header| header.starts_with(b"PE\0\0") && u16::from_le_bytes([header[4], header[5]]) == 0xAA64)
        }) {
            display.push('\n');
            display.push_str(&super::super::arm_branch::arm_branch(&data));
        }
    }
    if let Some(text) = super::super::call_sites::elf_code_sites(&data) {
        display.push('\n');
        display.push_str(&text);
    }
    if let Some(text) = super::super::call_sites::macho_code_sites(&data) {
        display.push('\n');
        display.push_str(&text);
    }
    if let Some(binary)=binary{let (root,relative,access)=scope;if access.graph&&!binary.coordinate.is_empty(){let snapshot=binary_evidence::snapshot(root,relative,access,&after,&data,complete,&binary,&display);if cancelled(){return Err("Inspection cancelled before typed evidence publication".into());}return Ok(match snapshot{Ok((display,graph))=>observation_graph::envelope(display,Ok(vec![graph]),access.budget),Err(e)=>observation_graph::envelope(display,Err(e),access.budget)});}}
    Ok(display)
}
pub(super) fn validate_remote(v:&Json)->Result<bool>{
    let args=get(v,"args");match text(v,"tool").as_str(){
        "web_read"=>{page_program::enabled(args)?;let u=text(args,"url");if u.len()>2048{return Err("Web URL exceeds 2048 bytes".into());}network::url(&u,u.starts_with("http://"))?;let c=get(args,"capture");if c!=&Null{let Obj(fields)=c else{return Err("Page capture settings must be an object".into());};if c.string().len()>4096||fields.keys().any(|k|!["font","output","width","height","resources"].contains(&k.as_str())){return Err("Unsupported or oversized page capture settings".into());}capture_file::validate(c)?;}},
        "web_search"=>{let q=text(args,"query");if q.trim().is_empty()||q.len()>2048||q.chars().any(char::is_control){return Err("Enter a search query of at most 2048 bytes".into());}search::alternative(v)?;},
        "mcp"=>{if get(args,"discover")==&Bool(true){if args!=&obj(vec![("discover",Bool(true))]){return Err("Discovery accepts only discover:true".into());}return Ok(true);}let n=text(args,"name");if n.is_empty()||n.len()>256||!matches!(get(args,"arguments"),Obj(_))||args.string().len()>65536{return Err("MCP requires a tool name and bounded JSON object arguments".into());}},
        _=>return Ok(false)
    }Ok(true)
}
/// Ordinary non-JavaScript search. Custom services remain optional; no model
/// credential or dormant custom-search credential is sent to this default.
pub(super) const DEFAULT_SEARCH_URL:&str="https://api.mwmbl.org/api/v2/search/?q={query}";
pub(super) fn search_template(st:&Json)->String{let configured=text(get(st,"web"),"search_url");if configured.trim().is_empty(){DEFAULT_SEARCH_URL.into()}else{configured.trim().into()}}
pub(super) fn web_effect(st:&mut Json,pending:&Json)->Result<Json>{
    validate_remote(pending)?;
    if text(pending,"tool")=="web_search"{search::template(st,pending)?;}
    if text(pending,"tool")=="web_search"&&search::known(st){let mut chain=search::begin(st,pending)?;let effect=match search::effect(&chain){Ok(e)=>e,Err(reason)=>{chain=search::advance(&chain,&reason,false)?;search::effect(&chain)?}};let mut effect=effect;put(st,"web_source",s(&text(&effect,"host")));put(&mut effect,"search_chain",chain);return Ok(effect);}
    let args=get(pending,"args");let mut headers=vec![("Accept","text/html, application/json, text/plain".into())];
    let url=if text(pending,"tool")=="web_search"{let web=get(st,"web");let template=search_template(st);let custom=!text(web,"search_url").trim().is_empty()&&template!="https://html.duckduckgo.com/html/?q={query}"&&template!=DEFAULT_SEARCH_URL;if !template.contains("{query}"){return Err("Custom search URL must contain {query}".into());}let key=text(web,"key");if custom&&!key.is_empty(){let header=text(web,"header");let prefix=text(web,"prefix");headers.push((if header.is_empty(){"Authorization"}else{get(web,"header").text()?},if prefix.is_empty(){key}else{format!("{prefix} {key}")}));}template.replace("{query}",&network::encoded(&text(args,"query")))}else{text(args,"url")};
    let mut effect=if text(pending,"tool")=="web_read"{network::page_wire(&url,url.starts_with("http://"),headers)?}else{network::wire(&url,url.starts_with("http://"),"GET","",headers)?};put(&mut effect,"purpose",s("web_tool"));put(&mut effect,"limit",Num((2*1024*1024).to_string()));put(st,"web_source",s(&if text(pending,"tool")=="web_search"{endpoint_summary(&url)}else{url.clone()}));Ok(effect)
}
/// A search response can be a verification page even with HTTP200. Only
/// page-level title/form signals count; quoted words in result text do not.
pub(super) fn search_page(body:&str)->Result<String>{search_challenge(body)?;page(body)}
pub(super) fn search_challenge(body:&str)->Result<()>{
 if body.trim_start().starts_with('<'){
  let doc=super::html_document::parse(body.as_bytes(),true);
  let mut verification=false;
  for node in &doc.nodes{if let super::html_document::Kind::Element{name,attributes,..}=&node.kind{
   let mut parent=node.parent;let mut inert=false;let mut title_context=true;
   while let Some(i)=parent{let ancestor=&doc.nodes[i];if let super::html_document::Kind::Element{name,..}=&ancestor.kind{if ["template","svg","math","noscript"].contains(&name.as_str()){inert=true;}if !["html","head"].contains(&name.as_str()){title_context=false;}}parent=ancestor.parent;}
   if inert{continue;}
   if name=="title"&&title_context{let title=node.children.iter().filter_map(|i|doc.nodes.get(*i)).filter_map(|n|if let super::html_document::Kind::Text{data,..}=&n.kind{Some(data.as_str())}else{None}).collect::<String>();let title=title.trim().to_ascii_lowercase();if ["verify you are human","are you a human?","just a moment...","security verification"].contains(&title.as_str()){verification=true;}}
   if name=="form"{let attr=|key:&str|attributes.iter().find(|a|a.name==key).and_then(|a|a.value.as_deref()).unwrap_or("");if attr("id")=="challenge-form"&&attr("action").split('?').next().unwrap_or("").ends_with("/anomaly.js"){verification=true;}}
  }}
  if verification{return Err("Search source returned a verification page; no search results were accepted and no automatic retry was started".into());}
 }
 Ok(())
}
// Static HTML extraction. Script/style bodies are never returned as page text.
pub(super) fn page(body:&str)->Result<String>{
    if body.len()>2*1024*1024{return Err("Web response exceeds 2 MiB".into());}
    if !body.trim_start().starts_with('<'){return Ok(byte_limit(body,98304));}
    html_page(body)
}
fn html_space(b:u8)->bool{matches!(b,b' '|b'\t'|b'\n'|b'\r'|12)}
fn html_name(b:u8)->bool{b.is_ascii_alphanumeric()||matches!(b,b'-'|b':'|b'_')}
struct PageTag<'a>{name:&'a str,closing:bool,end:usize,href:Option<&'a str>,complete:bool}
// Read one tag without allocating its attributes. Quoting starts only after '=';
// quotation marks in unquoted values do not change the tokenizer's state.
fn page_tag(text:&str,start:usize)->Option<PageTag<'_>>{
 let b=text.as_bytes();let mut p=start+1;let closing=b.get(p)==Some(&b'/');if closing{p+=1;}
 if !b.get(p).is_some_and(u8::is_ascii_alphabetic){return None;}
 let n=p;while b.get(p).is_some_and(|v|html_name(*v)){p+=1;}let name=&text[n..p];let mut href=None;let mut seen_href=false;
 loop{
  while b.get(p).is_some_and(|v|html_space(*v)){p+=1;}
  match b.get(p){None=>return Some(PageTag{name,closing,end:b.len(),href:None,complete:false}),Some(b'>')=>return Some(PageTag{name,closing,end:p+1,href,complete:true}),Some(b'/')=>{p+=1;continue},_=>{}}
  let a=p;while b.get(p).is_some_and(|v|!html_space(*v)&&!matches!(v,b'='|b'>'|b'/')){p+=1;}
  if a==p{p+=1;continue;}
  let attr=&text[a..p];while b.get(p).is_some_and(|v|html_space(*v)){p+=1;}
  let is_href=attr.eq_ignore_ascii_case("href");let first_href=is_href&&!seen_href;if is_href{seen_href=true;}
  if b.get(p)!=Some(&b'='){continue;}p+=1;while b.get(p).is_some_and(|v|html_space(*v)){p+=1;}
  let quote=b.get(p).copied().filter(|v|matches!(v,b'\''|b'"'));if quote.is_some(){p+=1;}
  let v=p;while let Some(c)=b.get(p){if quote.map(|q|*c==q).unwrap_or_else(||html_space(*c)||*c==b'>'){break;}p+=1;}
  if quote.is_some()&&p==b.len(){return Some(PageTag{name,closing,end:b.len(),href:None,complete:false});}
  if first_href&&p-v<=2048{href=Some(&text[v..p]);}
  if quote.is_some(){p+=1;}
 }
}
fn page_end_raw(text:&str,from:usize,name:&str)->Option<usize>{
 let b=text.as_bytes();let mut p=from;
 while p<b.len(){if b[p]==b'<'&&b.get(p+1)==Some(&b'/')&&b.get(p+2..p+2+name.len()).is_some_and(|v|v.eq_ignore_ascii_case(name.as_bytes()))&&b.get(p+2+name.len()).is_some_and(|v|html_space(*v)||matches!(v,b'>'|b'/')){return Some(p);}p+=1;}None
}
fn page_entities(text:&str)->String{
 let mut out=String::with_capacity(text.len());let mut p=0;
 while p<text.len(){if text.as_bytes()[p]==b'&'{let end=text.as_bytes()[p+1..text.len().min(p+34)].iter().position(|b|*b==b';').map(|n|p+1+n);if let Some(end)=end{let value=&text[p+1..end];let decoded=match value{"amp"=>Some('&'),"lt"=>Some('<'),"gt"=>Some('>'),"quot"=>Some('"'),"apos"|"#39"=>Some('\''),"nbsp"=>Some(' '),v if v.starts_with("#x")||v.starts_with("#X")=>u32::from_str_radix(&v[2..],16).ok().and_then(char::from_u32),v if v.starts_with('#')=>v[1..].parse::<u32>().ok().and_then(char::from_u32),_=>None};if let Some(c)=decoded{out.push(if c=='\0'{'\u{fffd}'}else{c});p=end+1;continue;}}}let c=text[p..].chars().next().unwrap();out.push(c);p+=c.len_utf8();}out
}
fn page_append(output:&mut String,text:&str)->bool{let remaining=98304-output.len();output.push_str(&byte_limit(text,remaining));text.len()>remaining}
pub(super) fn html_page(body:&str)->Result<String>{html_page_with_room(body,usize::MAX)}
pub(super) fn html_page_with_room(body:&str,room:usize)->Result<String>{
    if body.len()>2*1024*1024{return Err("Web response exceeds 2 MiB".into());}
    let mut output=String::new();let mut p=0;let mut links=Vec::new();let mut hidden=Vec::<String>::new();let mut raw=None::<String>;let mut rcdata=None::<String>;let mut incomplete=false;let mut omitted_links=0usize;let mut text_abbreviated=false;
    while p<body.len()&&output.len()<98304{
        if let Some(name)=raw.take(){if let Some(end)=page_end_raw(body,p,&name){p=end;}else{incomplete=true;break;}}
        if let Some(name)=rcdata.take(){let end=page_end_raw(body,p,&name).unwrap_or(body.len());if hidden.is_empty(){text_abbreviated|=page_append(&mut output,&body[p..end]);}p=end;if p==body.len(){break;}}
        let rest=&body[p..];
        if rest.starts_with("<!--"){if let Some(end)=rest[4..].find("-->"){p+=4+end+3;}else{incomplete=true;break;}continue;}
        if rest.starts_with("<!")||rest.starts_with("<?"){if let Some(end)=rest.find('>'){p+=end+1;}else{incomplete=true;break;}continue;}
        if rest.starts_with('<'){
            if let Some(tag)=page_tag(body,p){
                let name=tag.name;let closing=tag.closing;let start=p;p=tag.end;
                if !tag.complete{incomplete=true;break;}
                if name.eq_ignore_ascii_case("script")||name.eq_ignore_ascii_case("style"){if !closing{raw=Some(name.to_ascii_lowercase());}continue;}
                if name.eq_ignore_ascii_case("template")||name.eq_ignore_ascii_case("noscript"){
                    if closing{if hidden.last().is_some_and(|v|v.eq_ignore_ascii_case(name)){hidden.pop();}}else{if hidden.len()==64{incomplete=true;break;}hidden.push(name.to_ascii_lowercase());}continue;
                }
                if name.eq_ignore_ascii_case("title")||name.eq_ignore_ascii_case("textarea")||name.eq_ignore_ascii_case("xmp")||name.eq_ignore_ascii_case("iframe")||name.eq_ignore_ascii_case("noembed")||name.eq_ignore_ascii_case("noframes"){if !closing{rcdata=Some(name.to_ascii_lowercase());}continue;}
                if name.eq_ignore_ascii_case("plaintext")&&!closing{if hidden.is_empty(){text_abbreviated|=page_append(&mut output,&body[p..]);}p=body.len();break;}
                if !hidden.is_empty(){continue;}
                if name.eq_ignore_ascii_case("a")&&!closing{if let Some(value)=tag.href{if links.len()<100{links.push(page_entities(value));}else{omitted_links+=1;}}}
                if ["p","div","br","li","h1","h2","h3","h4","h5","h6","tr","section","article","header","footer"].iter().any(|v|name.eq_ignore_ascii_case(v)){output.push('\n');}
                debug_assert!(p>start);continue;
            }
            // '<' followed by ordinary text is a literal data character.
            if hidden.is_empty(){output.push('<');}p+=1;
        }else{let end=body[p..].find('<').map(|n|p+n).unwrap_or(body.len());if hidden.is_empty(){text_abbreviated|=page_append(&mut output,&body[p..end]);}p=end;}
    }
    let output=page_entities(&output);let abbreviated=text_abbreviated||(p<body.len()&&!incomplete);
    // Visible text/link evidence has its own bounded buffers. Optional structure
    // and geometry must not consume more than the current remaining allowance.
    let structure_room=room.saturating_sub(body.len().saturating_mul(8).saturating_add(2*1024*1024));
    let document=super::html_document::parse_with_room(body.as_bytes(),true,structure_room);
    let summary=if document.allocation_refused(){format!("{}\nCurrent memory allowance omitted further structured-tree and geometry analysis; bounded visible text/link extraction remains available.",document.summary())}else{format!("{}\n{}",document.summary(),page_geometry(&document))};
    let notes=format!("{}{}{}",if incomplete{"\nStatic extraction retained preceding text; an unfinished markup/raw-content tail or inert nesting bound was omitted."}else{""},if abbreviated{"\nVisible text extraction abbreviated at 98304 UTF-8 bytes; later source was not extracted."}else{""},if omitted_links>0{"\nAdditional link targets omitted after 100 retained links."}else{""});
    let observed=format!("{}\n\nPage link targets (up to 100, each at most 2048 source bytes; may be relative; not fetched):\n{}\nThis extraction stage executes no scripts; a preceding selected inline-program stage, if any, is described separately.{notes}",output.trim(),links.join("\n"));
    let limit=120000usize.saturating_sub(summary.len()+2);
    let observed=if observed.len()>limit{let note="\nStatic extraction report abbreviated at 120000 UTF-8 bytes; later text/link records omitted.";format!("{}{}",byte_limit(&observed,limit.saturating_sub(note.len())),note)}else{observed};
    Ok(format!("{observed}\n\n{summary}"))
}

fn page_geometry(document:&super::html_document::Document)->String{
 let cascade=super::css_cascade::compute(document);let mut output=cascade.describe(document);
 if cascade.incomplete{return output;}
 match super::page_layout::layout(document,&cascade.styles,800.,super::page_layout::TextMetrics::default()){
  Ok(layout)=>{output.push('\n');output.push_str(&layout.summary());output.push_str("\nObservation viewport: 800 CSS px (not the device screen). Geometry is approximate and is not a screenshot.");
   for b in layout.boxes.iter().take(16){output.push_str(&format!("\nBox node {}: x={} y={} width={} height={}",b.node,b.bounds.x,b.bounds.y,b.bounds.width,b.bounds.height));}
   if layout.boxes.len()>16{output.push_str("\nFurther box records omitted from this observation.");}
   for command in layout.commands.iter().take(24){match command{
    super::page_layout::Draw::Image{node,bounds}=>output.push_str(&format!("\nImage node {node}: ({},{}) {}x{}",bounds.x,bounds.y,bounds.width,bounds.height)),
    super::page_layout::Draw::Rect{node,bounds,color}=>output.push_str(&format!("\nPaint rectangle node {node}: ({},{}) {}x{} rgba{:?}",bounds.x,bounds.y,bounds.width,bounds.height,color)),
    super::page_layout::Draw::Text{node,bounds,text,..}=>{let mut parent=Some(*node);let mut inert=false;while let Some(i)=parent{let item=&document.nodes[i];if matches!(&item.kind,super::html_document::Kind::Element{name,..}if ["script","style","template","noscript"].contains(&name.as_str())){inert=true;break;}parent=item.parent;}if !inert{output.push_str(&format!("\nText fragment node {node}: ({},{}) {}x{} {:?}",bounds.x,bounds.y,bounds.width,bounds.height,byte_limit(text,128)));}},
   }}
   if layout.commands.len()>24{output.push_str("\nFurther display commands omitted from this observation.");}
   for issue in layout.issues.iter().take(16){output.push_str(&format!("\nLayout observation node {}: {}",issue.node,issue.code));}
  },Err(error)=>{output.push_str("\nPage geometry unavailable for this input: ");output.push_str(&error);}
 }output
}

pub(super) fn byte_limit(text:&str,max:usize)->String{let mut n=text.len().min(max);while !text.is_char_boundary(n){n-=1;}text[..n].to_string()}
#[cfg(test)]mod checks{use super::*;
    #[test]fn static_page_excludes_active_content(){let p=page("<p>Hello &amp; welcome</p><script>steal()</script><style>private</style><a href='https://example.org/?a=>'>Link</a>").unwrap();assert!(p.contains("Hello & welcome"));assert!(!p.contains("steal"));assert!(!p.contains("private"));assert!(p.contains("https://example.org/?a=>"));assert!(page("<p attr='unterminated").unwrap().contains("unfinished markup"));assert!(page("<script>if(x<2){}</script><p>Visible</p>").unwrap().contains("Visible"));assert!(page(&"界".repeat(100000)).unwrap().len()<=98304);}
    #[test]fn static_page_skips_long_attributes_and_preserves_readable_tail(){
        let source=format!("<div data-props='{}'><p>Useful tail &amp; evidence</p><a href='https://example.org/a?x=&quot;y&quot;'>Read</a></div>","nested < > ".repeat(3000));
        let observed=html_page(&source).unwrap();assert!(observed.contains("Useful tail & evidence"));assert!(observed.contains("https://example.org/a?x=\"y\""));assert!(!observed.contains("nested < >"));
        let observed=html_page("<p>Keep this</p><div data-state='unfinished >").unwrap();assert!(observed.contains("Keep this"));assert!(observed.contains("unfinished markup"));
        let observed=html_page("<p>1 < 2 &amp; 3 > 1</p><p x=unquoted'value>After</p>").unwrap();assert!(observed.contains("1 < 2 & 3 > 1"));assert!(observed.contains("After"));
    }
    #[test]fn static_page_inert_nested_and_attribute_identity(){
        let observed=html_page("<template><template>HIDDEN_ONE</template>HIDDEN_TWO<script>\"</template>\";HIDDEN_THREE</script></template><noscript>HIDDEN_FOUR</noscript><p>Visible</p><a data-href='wrong' HREF='first' href='second'>Link</a>").unwrap();assert!(observed.contains("Visible"));assert!(!observed.contains("HIDDEN_"));assert!(observed.contains("\nfirst\n"));assert!(!observed.contains("second"));assert!(!observed.contains("wrong"));
        let source=format!("<a href='{}' href='wrong'>Link</a>","x".repeat(2049));assert!(!html_page(&source).unwrap().contains("wrong"));
        assert_eq!(page_entities("&日本日本日本日本日本日本日本日本日本日本日本日本;"),"&日本日本日本日本日本日本日本日本日本日本日本日本;");
    }
    #[test]fn static_page_resource_and_partial_report_bounds(){
        assert!(html_page(&"x".repeat(2*1024*1024+1)).is_err());
        let source=format!("<p>{}</p>{}","界".repeat(40000),"<a href='https://example.org/'>Link</a>".repeat(110));let observed=html_page(&source).unwrap();assert!(observed.len()<=120000);assert!(observed.contains("abbreviated"));assert!(html_page(&format!("<p>{}","x".repeat(100000))).unwrap().contains("abbreviated"));
        let source=format!("<p>Start</p>{}",format!("<a href='{}'>Link</a>","a".repeat(2048)).repeat(100));let observed=html_page(&source).unwrap();assert!(observed.len()<=120000);assert!(observed.contains("report abbreviated"));
    }
    #[test]fn free_search_default_needs_no_url_or_credentials(){
        let mut st=obj(vec![("web",obj(vec![("key",s("dormant-custom-secret")),("header",s("X-Private"))])),("profiles",Arr(vec![obj(vec![("key",s("model-secret"))])]))]);
        let request=obj(vec![("tool",s("web_search")),("args",obj(vec![("query",s("Rust + café & 日本"))]))]);
        assert_eq!(search_template(&st),DEFAULT_SEARCH_URL);let effect=web_effect(&mut st,&request).unwrap();assert!(["api.mwmbl.org","html.duckduckgo.com"].contains(&text(&effect,"host").as_str()));let wire=text(&effect,"wire");assert!(wire.contains("q=Rust%20%2B%20caf%C3%A9%20%26%20%E6%97%A5%E6%9C%AC HTTP/1.1"));assert!(!wire.contains("secret"));assert!(!wire.contains("Authorization"));assert!(!wire.contains("X-Private"));
        put(&mut st,"web",obj(vec![("search_url",s("https://html.duckduckgo.com/html/?q={query}")),("key",s("dormant-custom-secret"))]));assert!(!text(&web_effect(&mut st,&request).unwrap(),"wire").contains("secret"));
    }
    #[test]fn verification_pages_do_not_become_search_evidence(){
        assert!(search_page("<title>Verify you are human</title><p>Check</p>").is_err());assert!(search_page("<title>DuckDuckGo</title><form id='challenge-form' action='//duckduckgo.com/anomaly.js?q=x'></form>").is_err());
        assert!(search_page("<title>CAPTCHA research at DuckDuckGo</title><p>How to verify you are human: quoted article</p>").unwrap().contains("quoted article"));assert!(search_page("{\"title\":\"Verify you are human\"}").is_ok());assert!(search_page("<html><head><title>Search</title></head><body><template><title>Verify you are human</title><form id='challenge-form' action='/anomaly.js'></form></template><svg><title>Verify you are human</title></svg></body></html>").is_ok());
    }
    #[test]fn remote_arguments_have_distinct_validation(){assert!(validate_remote(&obj(vec![("tool",s("web_read")),("args",obj(vec![("url",s("https://example.org/path"))]))])).unwrap());assert!(validate_remote(&obj(vec![("tool",s("web_read")),("args",obj(vec![("url",s("file:///secret"))]))])).is_err());assert!(validate_remote(&obj(vec![("tool",s("mcp")),("args",obj(vec![("name",s("inspect")),("arguments",s("bad"))]))])).is_err());}
    #[test]
    fn page_route_asks_for_html() {
        let mut st=obj(vec![]);
        let request=obj(vec![("tool",s("web_read")),("args",obj(vec![("url",s("https://example.org/path"))]))]);
        let raw=text(&web_effect(&mut st,&request).unwrap(),"wire");
        assert!(raw.starts_with("GET /path HTTP/1.1\r\n"), "{raw}");
        assert!(raw.contains("\r\nAccept: text/html, application/json, text/plain\r\n"), "{raw}");
        assert_eq!(raw.matches("\r\nAccept:").count(), 1, "{raw}");
        assert!(raw.contains("\r\nUser-Agent: RLLM web reader\r\n"), "{raw}");
        assert_eq!(raw.matches("\r\nUser-Agent:").count(), 1, "{raw}");
    }
    #[test]
    fn local_listener_adopts_the_page_only_when_user_agent_is_present() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::sync::mpsc;
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();
        let port=listener.local_addr().unwrap().port();
        let (tx,rx)=mpsc::channel();
        std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut sock,_)=listener.accept().unwrap();
                sock.set_read_timeout(Some(std::time::Duration::from_secs(2))).ok();
                let mut buf=Vec::new();
                let mut tmp=[0u8;2048];
                loop {
                    match sock.read(&mut tmp) {
                        Ok(0)=>break,
                        Ok(n)=>{buf.extend_from_slice(&tmp[..n]);if buf.windows(4).any(|w|w==b"\r\n\r\n"){break;}}
                        Err(_)=>break,
                    }
                }
                let head=String::from_utf8_lossy(&buf);
                let identified=head.lines().any(|line|line.to_ascii_lowercase().starts_with("user-agent:"));
                let body=if identified{"<p>LOCAL_PAGE_MARKER</p>"}else{"denied"};
                let status=if identified{"200 OK"}else{"403 Forbidden"};
                let response=format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                sock.write_all(response.as_bytes()).unwrap();
                let _=tx.send(buf);
            }
        });
        fn exchange(port:u16,request:&[u8])->Vec<u8>{
            let mut sock=TcpStream::connect(("127.0.0.1",port)).unwrap();
            sock.set_read_timeout(Some(std::time::Duration::from_secs(2))).ok();
            sock.write_all(request).unwrap();
            let mut response=Vec::new();
            sock.read_to_end(&mut response).unwrap();
            response
        }
        let bare=format!("GET /owned-page HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nAccept-Encoding: identity\r\nContent-Length: 0\r\n\r\n");
        let refused=exchange(port,bare.as_bytes());
        assert!(refused.starts_with(b"HTTP/1.1 403 "), "{}", String::from_utf8_lossy(&refused));
        let err=super::super::web_document::read(&refused,8*1024*1024,&Null).unwrap_err();
        let (response_fault,diagnostic)=super::super::web_document::explain(err);
        assert!(response_fault);
        assert_eq!(super::super::web_operation::page_error(diagnostic),"Website returned HTTP 403; no page contents were accepted. This status concerns the requested website URL.");
        let url=format!("http://127.0.0.1:{port}/owned-page");
        let mut st=obj(vec![]);
        let request=obj(vec![("tool",s("web_read")),("args",obj(vec![("url",s(&url))]))]);
        let wire=text(&web_effect(&mut st,&request).unwrap(),"wire");
        let adopted_bytes=exchange(port,wire.as_bytes());
        assert!(adopted_bytes.starts_with(b"HTTP/1.1 200 "), "{}", String::from_utf8_lossy(&adopted_bytes));
        let (code,adopted)=super::super::web_document::read(&adopted_bytes,8*1024*1024,&Null).unwrap();
        assert_eq!(code,200);
        assert!(adopted.contains("LOCAL_PAGE_MARKER"), "{adopted}");
        let bare_received=rx.recv().unwrap();
        let product_received=rx.recv().unwrap();
        println!("LISTENER_BARE:{:?}", String::from_utf8(bare_received).unwrap());
        println!("LISTENER_PRODUCT:{:?}", String::from_utf8(product_received.clone()).unwrap());
        let product=String::from_utf8(product_received).unwrap();
        assert_eq!(product, wire);
        assert_eq!(product.matches("\r\nUser-Agent:").count(), 1);
        assert!(product.contains("\r\nAccept: text/html, application/json, text/plain\r\n"));
    }
}

#[cfg(test)] mod cancellation_scope_checks {
 use super::*;
 #[test] fn old_owner_refuses_file_folder_and_comparison_after_reset() {
  let exec=&super::super::super::execution::cancellation_token;
  super::super::super::execution::reset();let token=exec();
  let root=std::env::temp_dir().join(format!("rllm-cancel-scope-{}-{}",std::process::id(),now()));fs::create_dir_all(&root).unwrap();fs::write(root.join("a"),b"one").unwrap();fs::write(root.join("b"),b"two").unwrap();
  let mut access=operation::InspectionAccess{list:true,read:true,graph:false,budget:2*1024*1024,root_identity:None,cancellation_token:token,deadline:std::time::Instant::now()+std::time::Duration::from_secs(120)};
  super::super::super::execution::cancel();super::super::super::execution::reset();
  let args=obj(vec![("path",s("a")),("other",s("b"))]);
  assert!(inspect_scoped(&root,&root.join("a"),"a",&access).unwrap_err().contains("cancelled"));
  assert!(inspect_scoped(&root,&root,"",&access).unwrap_err().contains("cancelled"));
  assert!(compare_scoped(&root,&root.join("a"),&root.join("b"),&args,&access).unwrap_err().contains("cancelled"));
  access.cancellation_token=exec();assert!(inspect_scoped(&root,&root.join("a"),"a",&access).is_ok());assert!(inspect_scoped(&root,&root,"",&access).is_ok());assert!(compare_scoped(&root,&root.join("a"),&root.join("b"),&args,&access).is_ok());
 }
}

#[cfg(test)]
mod dotnet_inspect_checks {
    use super::*;
    const PE_TAIL: &str = "x86 instruction summary ended.\n";
    const FOOTER: &str = "Signatures and symbolic instruction IR describe the captured bytes. File sizes and sampled byte counts identify the observation coverage.";

    fn put16(buf: &mut [u8], at: usize, value: u16) {
        buf[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put32(buf: &mut [u8], at: usize, value: u32) {
        buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn push16(buf: &mut Vec<u8>, value: u16) {
        buf.extend_from_slice(&value.to_le_bytes());
    }
    fn push32(buf: &mut Vec<u8>, value: u32) {
        buf.extend_from_slice(&value.to_le_bytes());
    }
    fn pe(section: &[u8], cli_rva: u32, cli_size: u32, directories: u32) -> Vec<u8> {
        let mut file = vec![0u8; 0x200 + section.len()];
        file[0] = b'M';
        file[1] = b'Z';
        put32(&mut file, 0x3c, 0x80);
        file[0x80..0x84].copy_from_slice(b"PE\0\0");
        put16(&mut file, 0x84, 0x8664);
        put16(&mut file, 0x86, 1);
        put16(&mut file, 0x94, 240);
        let optional = 0x98;
        put16(&mut file, optional, 0x20b);
        put32(&mut file, optional + 60, 0x200);
        put32(&mut file, optional + 108, directories);
        if directories >= 15 {
            put32(&mut file, optional + 224, cli_rva);
            put32(&mut file, optional + 228, cli_size);
        } else {
            put32(&mut file, optional + 224, 0x2000);
            put32(&mut file, optional + 228, 72);
        }
        let section_at = optional + 240;
        file[section_at..section_at + 5].copy_from_slice(b".text");
        put32(&mut file, section_at + 8, section.len() as u32);
        put32(&mut file, section_at + 12, 0x2000);
        put32(&mut file, section_at + 16, section.len() as u32);
        put32(&mut file, section_at + 20, 0x200);
        file[0x200..0x200 + section.len()].copy_from_slice(section);
        file
    }
    fn clr_sized(metadata: &[u8], metadata_size: u32) -> Vec<u8> {
        let mut section = vec![0u8; 72 + metadata.len()];
        put32(&mut section, 0, 72);
        put16(&mut section, 4, 2);
        put16(&mut section, 6, 5);
        put32(&mut section, 8, 0x2000 + 72);
        put32(&mut section, 12, metadata_size);
        section[72..].copy_from_slice(metadata);
        section
    }
    fn version_root() -> Vec<u8> {
        let version = b"v4.0.30319\0\0";
        let mut out = Vec::new();
        out.extend_from_slice(b"BSJB");
        push16(&mut out, 1);
        push16(&mut out, 1);
        push32(&mut out, 0);
        push32(&mut out, version.len() as u32);
        out.extend_from_slice(version);
        push16(&mut out, 0);
        push16(&mut out, 0);
        out
    }
    fn cli_image(metadata: &[u8]) -> Vec<u8> {
        pe(&clr_sized(metadata, metadata.len() as u32), 0x2000, 72, 16)
    }
    struct Rm(std::path::PathBuf);
    impl Drop for Rm {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn cli_string(bytes: &[u8]) -> Option<String> {
        match super::super::super::dotnet_evidence::cli_evidence(bytes, &|| false).expect("not cancelled") {
            super::super::super::dotnet_evidence::CliText::Absent => None,
            super::super::super::dotnet_evidence::CliText::Observed(text) | super::super::super::dotnet_evidence::CliText::Bounded(text) => Some(text),
        }
    }
    fn inspect_bytes(label: &str, bytes: &[u8]) -> String {
        let root = std::env::temp_dir().join(format!("rllm-dotnet5-{label}-{}-{}", std::process::id(), now()));
        let _cleanup = Rm(root.clone());
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("sample.bin"), bytes).unwrap();
        super::super::super::execution::reset();
        let access = operation::InspectionAccess {
            list: true,
            read: true,
            graph: false,
            budget: 2 * 1024 * 1024,
            root_identity: None,
            cancellation_token: super::super::super::execution::cancellation_token(),
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(120),
        };
        inspect_scoped(&root, &root.join("sample.bin"), "sample.bin", &access).unwrap_or_else(|error| panic!("{label}: {error}"))
    }
    fn after_pe(shown: &str) -> &str {
        let rest = shown.split(PE_TAIL).nth(1).unwrap_or("");
        rest.split(FOOTER).next().unwrap_or("")
    }

    #[test]
    fn pe_without_cli_directory_keeps_inspect_text() {
        // Fails if a CLI-less PE gains a .NET section, or if the PE observation text is rewritten.
        let bytes = pe(b"BSJB is not a cli directory by itself", 0, 0, 14);
        assert!(matches!(super::super::super::dotnet_evidence::cli_evidence(&bytes, &|| false).expect("not cancelled"), super::super::super::dotnet_evidence::CliText::Absent));
        let shown = inspect_bytes("native", &bytes);
        let pe_text = super::super::pe_observation::analyze(&bytes, true, bytes.len() as u64, &|| false).unwrap();
        assert_eq!(shown.matches(pe_text.as_str()).count(), 1);
        assert_eq!(shown.matches(PE_TAIL).count(), 1);
        assert_eq!(after_pe(&shown), "\n");
        assert_eq!(shown.matches(".NET metadata and CIL observation\n").count(), 0);
        assert_eq!(shown.matches("BSJB is not a cli directory by itself").count(), 1);
        assert_eq!(pe_text.matches(".NET metadata and CIL observation\n").count(), 0);
    }

    #[test]
    fn cli_directory_appends_exact_dotnet_lines() {
        // Fails if inspect drops the CLI text or splices it into the PE observation.
        let bytes = cli_image(&version_root());
        let evidence = cli_string(&bytes).expect("cli image");
        let shown = inspect_bytes("cli", &bytes);
        let pe_text = super::super::pe_observation::analyze(&bytes, true, bytes.len() as u64, &|| false).unwrap();
        assert_eq!(shown.matches(pe_text.as_str()).count(), 1);
        assert_eq!(pe_text.matches(".NET metadata and CIL observation\n").count(), 0);
        assert_eq!(shown.matches(evidence.as_str()).count(), 1);
        assert_eq!(shown.matches(".NET metadata and CIL observation\n").count(), 1);
        assert_eq!(shown.matches("Metadata signature BSJB.\n").count(), 1);
        assert_eq!(shown.matches("Metadata version v4.0.30319.\n").count(), 1);
        assert_eq!(after_pe(&shown), format!("\n{evidence}\n"));
    }

    #[test]
    fn inconsistent_cli_extent_is_refused_by_name_in_inspect() {
        // Fails if an extent past the section is omitted or refused without the CLR directory name.
        let bytes = pe(b"short", 0x2000, 72, 16);
        let evidence = cli_string(&bytes).expect("inconsistent extent");
        let shown = inspect_bytes("extent", &bytes);
        let refusal = "Refusal: CLR data directory entry 14 size 72 at RVA 0x2000 points outside the mapped file.\n";
        assert_eq!(evidence.matches(refusal).count(), 1);
        assert_eq!(shown.matches(refusal).count(), 1);
        assert_eq!(evidence.matches("Metadata signature BSJB.\n").count(), 0);
        assert_eq!(shown.matches(evidence.as_str()).count(), 1);
        assert_eq!(after_pe(&shown), format!("\n{evidence}\n"));
    }

    #[test]
    fn truncated_metadata_stream_names_the_unread_header_in_inspect() {
        // Fails if the cut stream header is completed from BAITSTREAM, or if inspect omits the unread-header refusal.
        let mut meta = Vec::new();
        meta.extend_from_slice(b"BSJB");
        push16(&mut meta, 1);
        push16(&mut meta, 1);
        push32(&mut meta, 0);
        push32(&mut meta, 12);
        meta.extend_from_slice(b"v4.0.30319\0\0");
        push16(&mut meta, 0);
        push16(&mut meta, 2);
        push32(&mut meta, 0);
        push32(&mut meta, 0);
        meta.extend_from_slice(b"#~\0\0");
        push32(&mut meta, 0);
        assert_eq!(meta.len(), 48);
        let mut carried = meta.clone();
        push32(&mut carried, 0);
        carried.extend_from_slice(b"BAITSTREAM\0\0");
        let bytes = pe(&clr_sized(&carried, meta.len() as u32), 0x2000, 72, 16);
        let evidence = cli_string(&bytes).expect("truncated stream");
        let shown = inspect_bytes("truncated", &bytes);
        let refusal = "Refusal: metadata stream header truncated. No stream is listed.\n";
        assert_eq!(evidence.matches(refusal).count(), 1);
        assert_eq!(shown.matches(refusal).count(), 1);
        assert_eq!(evidence.matches("BAITSTREAM").count(), 0);
        assert_eq!(evidence.matches("#~").count(), 0);
        assert_eq!(shown.matches(evidence.as_str()).count(), 1);
    }

    fn align4(value: usize) -> usize {
        value.saturating_add(3) & !3
    }

    fn metadata(version: &str, streams: &[(&str, &[u8])]) -> Vec<u8> {
        let mut version_bytes = version.as_bytes().to_vec();
        version_bytes.push(0);
        while version_bytes.len() % 4 != 0 {
            version_bytes.push(0);
        }
        let mut header_len = 16 + version_bytes.len() + 4;
        for (name, _) in streams {
            header_len += 8 + align4(name.len() + 1);
        }
        let mut cursor = align4(header_len);
        let mut placed = Vec::new();
        for (name, data) in streams {
            placed.push((cursor, *data, *name));
            cursor = align4(cursor + data.len());
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"BSJB");
        push16(&mut out, 1);
        push16(&mut out, 1);
        push32(&mut out, 0);
        push32(&mut out, version_bytes.len() as u32);
        out.extend_from_slice(&version_bytes);
        push16(&mut out, 0);
        push16(&mut out, streams.len() as u16);
        for (offset, data, name) in &placed {
            push32(&mut out, *offset as u32);
            push32(&mut out, data.len() as u32);
            let mut name_bytes = name.as_bytes().to_vec();
            name_bytes.push(0);
            while name_bytes.len() % 4 != 0 {
                name_bytes.push(0);
            }
            out.extend_from_slice(&name_bytes);
        }
        assert_eq!(out.len(), header_len);
        for (offset, data, _) in &placed {
            while out.len() < *offset {
                out.push(0);
            }
            assert_eq!(out.len(), *offset);
            out.extend_from_slice(data);
        }
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out
    }

    fn tilde(entries: &[(u8, u32)], heaps: u8, body: &[u8]) -> Vec<u8> {
        let mut valid = 0u64;
        for (index, _) in entries {
            valid |= 1u64 << index;
        }
        let mut out = Vec::new();
        push32(&mut out, 0);
        out.push(2);
        out.push(0);
        out.push(heaps);
        out.push(1);
        out.extend_from_slice(&valid.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        for (_, count) in entries {
            push32(&mut out, *count);
        }
        out.extend_from_slice(body);
        out
    }

    fn strings(items: &[&str]) -> (Vec<u8>, Vec<u32>) {
        let mut heap = vec![0u8];
        let mut indexes = Vec::new();
        for item in items {
            indexes.push(heap.len() as u32);
            heap.extend_from_slice(item.as_bytes());
            heap.push(0);
        }
        (heap, indexes)
    }

    fn module_row(name_index: u16) -> Vec<u8> {
        let mut row = Vec::new();
        push16(&mut row, 0);
        push16(&mut row, name_index);
        push16(&mut row, 0);
        push16(&mut row, 0);
        push16(&mut row, 0);
        row
    }

    fn method_row(rva: u32) -> Vec<u8> {
        let mut row = Vec::new();
        push32(&mut row, rva);
        push16(&mut row, 0);
        push16(&mut row, 0);
        push16(&mut row, 0);
        push16(&mut row, 0);
        push16(&mut row, 1);
        row
    }

    fn clr(metadata: &[u8]) -> Vec<u8> {
        clr_sized(metadata, metadata.len() as u32)
    }

    fn access_for(root_label: &str) -> (std::path::PathBuf, operation::InspectionAccess) {
        let root = std::env::temp_dir().join(format!("rllm-dotnet7-{root_label}-{}-{}", std::process::id(), now()));
        fs::create_dir_all(&root).unwrap();
        super::super::super::execution::reset();
        let access = operation::InspectionAccess {
            list: true,
            read: true,
            graph: false,
            budget: 2 * 1024 * 1024,
            root_identity: None,
            cancellation_token: super::super::super::execution::cancellation_token(),
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(120),
        };
        (root, access)
    }

    fn assert_file_untouched(root: &std::path::Path, bytes: &[u8]) {
        let path = root.join("sample.bin");
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let mut names: Vec<_> = fs::read_dir(root).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        names.sort();
        assert_eq!(names, vec![std::ffi::OsString::from("sample.bin")]);
    }

    fn inspect_kept(label: &str, bytes: &[u8]) -> String {
        let (root, access) = access_for(label);
        let _cleanup = Rm(root.clone());
        let path = root.join("sample.bin");
        fs::write(&path, bytes).unwrap();
        let shown = super::inspect_scoped(&root, &path, "sample.bin", &access).unwrap_or_else(|error| panic!("{label}: {error}"));
        assert_file_untouched(&root, bytes);
        shown
    }

    fn inspect_cancel(label: &str, bytes: &[u8], cancelled: &impl Fn() -> bool) -> Result<String> {
        let (root, access) = access_for(label);
        let _cleanup = Rm(root.clone());
        let path = root.join("sample.bin");
        fs::write(&path, bytes).unwrap();
        let source = super::inspect_projected(&root, "sample.bin", &access)?;
        let outcome = super::inspect(&path, &source, cancelled, (&root, "sample.bin", &access));
        assert_file_untouched(&root, bytes);
        outcome
    }

    fn named_module_image(strings_size: u32, table_size: u32) -> Vec<u8> {
        let (heap, indexes) = strings(&["DemoModule"]);
        let body = module_row(indexes[0] as u16);
        let stream = tilde(&[(0x00, 1)], 0, &body);
        let mut root = metadata("v4.0.30319", &[("#~", stream.as_slice()), ("#Strings", heap.as_slice())]);
        let table_at = root.windows(3).position(|window| window == b"#~\0").unwrap();
        put32(&mut root, table_at - 4, table_size);
        let strings_at = root.windows(9).position(|window| window == b"#Strings\0").unwrap();
        put32(&mut root, strings_at - 4, strings_size);
        pe(&clr(&root), 0x2000, 72, 16)
    }

    fn padded_metadata(len: usize) -> Vec<u8> {
        let mut padded = version_root();
        assert!(len >= padded.len());
        padded.resize(len, 0);
        pe(&clr_sized(&padded, padded.len() as u32), 0x2000, 72, 16)
    }

    fn method_image(declared: u32, present: usize, body: &[u8]) -> (Vec<u8>, u32) {
        let (heap, indexes) = strings(&["Demo"]);
        let build = |rvas: &[u32]| {
            let mut rows = module_row(indexes[0] as u16);
            for rva in rvas {
                rows.extend(method_row(*rva));
            }
            let stream = tilde(&[(0x00, 1), (0x06, declared)], 0, &rows);
            metadata("v4.0.30319", &[("#~", stream.as_slice()), ("#Strings", heap.as_slice())])
        };
        let probe = build(&vec![0; present]);
        let base = 0x2000 + 72 + probe.len() as u32;
        let rvas: Vec<u32> = if body.is_empty() {
            vec![0; present]
        } else {
            (0..present).map(|index| base + (index * body.len()) as u32).collect()
        };
        let root = build(&rvas);
        assert_eq!(root.len(), probe.len());
        let mut section = clr(&root);
        for _ in 0..present {
            section.extend_from_slice(body);
        }
        (pe(&section, 0x2000, 72, 16), base)
    }

    #[test]
    fn huge_stream_declarations_with_few_bytes_stay_accepted() {
        // Fails if a #~ or #Strings size is charged or refused when the bytes actually present are few.
        let bytes = named_module_image(0xF000_0000, 0xF000_0000);
        let shown = inspect_kept("declared-few", &bytes);
        assert_eq!(shown.matches("table 0x00 Module rows 1.\n").count(), 1, "{shown}");
        assert_eq!(shown.matches("Module name: DemoModule.\n").count(), 1, "{shown}");
        assert_eq!(shown.matches("dotnet parse work cap").count(), 0, "{shown}");
        assert_eq!(shown.matches("dotnet declared size cap").count(), 0, "{shown}");
        assert_eq!(shown.matches("Refusal:").count(), 0, "{shown}");
        match super::super::super::dotnet_evidence::cli_evidence(&bytes, &|| false).expect("not cancelled") {
            super::super::super::dotnet_evidence::CliText::Observed(text) => {
                assert_eq!(text.matches("Module name: DemoModule.\n").count(), 1);
            }
            other => panic!("a large declaration was not an ordinary observation: {other:?}"),
        }
    }

    #[test]
    fn touched_bytes_over_the_budget_name_the_parse_work_cap() {
        // Fails if the mapped metadata length is not charged onto the parse work cap.
        let meta_len = super::super::super::dotnet_evidence::PARSE_WORK_CAP - 72 - 8;
        let bytes = padded_metadata(meta_len);
        let shown = inspect_kept("work-cap", &bytes);
        assert_eq!(shown.matches("Refusal: dotnet parse work cap.\n").count(), 1, "{shown}");
        assert_eq!(shown.matches("Refusal: dotnet parse work cap. Remaining streams, tables, rows, and names were not read.\n").count(), 1, "{shown}");
        assert_eq!(shown.matches("dotnet declared size cap").count(), 0, "{shown}");
        assert_eq!(shown.matches("table 0x00 ").count(), 0, "{shown}");
        assert_eq!(shown.matches("Metadata version v4.0.30319.\n").count(), 0, "{shown}");
        match super::super::super::dotnet_evidence::cli_evidence(&bytes, &|| false).expect("not cancelled") {
            super::super::super::dotnet_evidence::CliText::Bounded(text) => {
                assert!(text.contains("dotnet parse work cap"), "{text}");
            }
            other => panic!("work cap was not a bounded refusal: {other:?}"),
        }
    }

    #[test]
    fn declared_range_over_the_budget_uses_its_own_name() {
        // Fails if a present range whose length alone exceeds the budget is charged as the work cap.
        let bytes = padded_metadata(super::super::super::dotnet_evidence::PARSE_WORK_CAP + 64);
        let shown = inspect_kept("declared-cap", &bytes);
        assert_eq!(shown.matches("Refusal: dotnet declared size cap.\n").count(), 1, "{shown}");
        assert_eq!(shown.matches("Refusal: dotnet declared size cap. The declared range was not scanned.\n").count(), 1, "{shown}");
        assert_eq!(shown.matches("dotnet parse work cap").count(), 0, "{shown}");
        assert_eq!(shown.matches("Metadata signature BSJB.\n").count(), 0, "{shown}");
        match super::super::super::dotnet_evidence::cli_evidence(&bytes, &|| false).expect("not cancelled") {
            super::super::super::dotnet_evidence::CliText::Bounded(text) => {
                assert!(text.contains("dotnet declared size cap"), "{text}");
                assert!(!text.contains("dotnet parse work cap"), "{text}");
            }
            other => panic!("declared size was not a bounded refusal: {other:?}"),
        }
    }

    #[test]
    fn emitted_method_headers_stop_at_the_observation_byte_cap() {
        // Fails if method-header lines are accumulated past the observation byte cap, or if the cut is unnamed.
        let (bytes, base) = method_image(320, 320, &[0x02]);
        let shown = inspect_kept("output-cap", &bytes);
        let wrapped = after_pe(&shown);
        assert!(wrapped.starts_with('\n') && wrapped.ends_with('\n'), "{wrapped}");
        let dotnet = &wrapped[1..wrapped.len() - 1];
        let first = format!("MethodDef RVA {base:#x} header tiny, code size 0. IL was not decoded.\n");
        let last = format!("MethodDef RVA {:#x} header tiny, code size 0. IL was not decoded.\n", base + 319);
        assert_eq!(dotnet.matches(first.as_str()).count(), 1, "{dotnet}");
        assert_eq!(dotnet.matches(last.as_str()).count(), 0, "{dotnet}");
        assert_eq!(dotnet.matches("Refusal: dotnet observation byte cap.\n").count(), 1, "{dotnet}");
        assert_eq!(dotnet.matches("Refusal: dotnet observation byte cap. Later observation lines were not emitted.\n").count(), 1, "{dotnet}");
        assert!(dotnet.len() <= super::super::super::dotnet_evidence::OBSERVATION_BYTE_CAP, "{}", dotnet.len());
        assert!(dotnet.ends_with("Later observation lines were not emitted.\n"), "{dotnet}");
        match super::super::super::dotnet_evidence::cli_evidence(&bytes, &|| false).expect("not cancelled") {
            super::super::super::dotnet_evidence::CliText::Bounded(_) => {}
            other => panic!("output cap was not a bounded refusal: {other:?}"),
        }
    }

    #[test]
    fn cancellation_during_partial_methods_is_inspection_cancelled() {
        // Fails if partial_methods does not poll the cancellation token.
        // The at-cap image reads every MethodDef row and does not enter partial_methods.
        // The over-cap image takes that error path and scans the same rows again.
        // Those two files differ by the declared row count, so their poll totals differ
        // by the partial_methods charges. A trip at the at-cap total therefore falls
        // inside that second scan, and only while the scan still polls.
        let present = super::super::super::dotnet_evidence::METHOD_ROW_CAP;
        let (at_cap, _) = method_image(present as u32, present, &[]);
        let (over, _) = method_image(present as u32 + 1, present, &[]);
        let run = |bytes: &[u8], trip: u32| {
            let calls = std::cell::Cell::new(0u32);
            let cancel = || {
                let seen = calls.get();
                calls.set(seen + 1);
                seen >= trip
            };
            let outcome = inspect_cancel("partial-methods", bytes, &cancel);
            (outcome, calls.get())
        };
        let (finished_at, at_calls) = run(&at_cap, u32::MAX);
        let at_shown = finished_at.expect("at-cap image");
        assert_eq!(at_shown.matches("dotnet method row cap").count(), 0, "{at_shown}");
        let (still, _) = run(&at_cap, at_calls);
        assert!(still.is_ok(), "trip landed before partial_methods: {still:?}");
        let (finished_over, _) = run(&over, u32::MAX);
        let over_shown = finished_over.expect("method row cap path");
        assert!(over_shown.contains("dotnet method row cap"), "{over_shown}");
        let (stopped, _) = run(&over, at_calls);
        assert_eq!(stopped.expect_err("cancel during partial_methods"), "Inspection cancelled");
    }

    #[test]
    fn malformed_image_stays_observed_while_a_cap_is_bounded() {
        // Fails if a cap refusal and a malformed image share one presence value.
        let malformed = pe(b"short", 0x2000, 72, 16);
        match super::super::super::dotnet_evidence::cli_evidence(&malformed, &|| false).expect("not cancelled") {
            super::super::super::dotnet_evidence::CliText::Observed(text) => {
                assert_eq!(text.matches("Refusal: CLR data directory entry 14 size 72 at RVA 0x2000 points outside the mapped file.\n").count(), 1);
                assert_eq!(text.matches("dotnet parse work cap").count(), 0);
                assert_eq!(text.matches("dotnet declared size cap").count(), 0);
            }
            other => panic!("malformed image was not Observed: {other:?}"),
        }
        let shown = inspect_kept("malformed-vs-cap", &malformed);
        assert_eq!(shown.matches("Refusal: CLR data directory entry 14 size 72 at RVA 0x2000 points outside the mapped file.\n").count(), 1);
    }
}

#[cfg(test)]
mod capture_file_checks {
    use super::*;

    struct Rm(std::path::PathBuf);
    impl Drop for Rm {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320u32 & 0u32.wrapping_sub(crc & 1));
            }
        }
        !crc
    }

    fn push_u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    /// Stored ZIP. Local and central DOS time is 0x8120 so header byte 0x81 is present.
    fn zip(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut locals = Vec::new();
        let mut index = Vec::new();
        for (name, data) in members {
            let offset = locals.len() as u32;
            let name_bytes = name.as_bytes();
            push_u32(&mut locals, 0x04034b50);
            push_u16(&mut locals, 20);
            push_u16(&mut locals, 0);
            push_u16(&mut locals, 0);
            push_u16(&mut locals, 0x8120);
            push_u16(&mut locals, 0);
            push_u32(&mut locals, crc32(data));
            push_u32(&mut locals, data.len() as u32);
            push_u32(&mut locals, data.len() as u32);
            push_u16(&mut locals, name_bytes.len() as u16);
            push_u16(&mut locals, 0);
            locals.extend_from_slice(name_bytes);
            locals.extend_from_slice(data);
            index.push(((*name).to_string(), data.to_vec(), offset));
        }
        let dir_at = locals.len();
        let mut out = locals;
        for (name, data, offset) in &index {
            let name_bytes = name.as_bytes();
            push_u32(&mut out, 0x02014b50);
            push_u16(&mut out, 20);
            push_u16(&mut out, 20);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0x8120);
            push_u16(&mut out, 0);
            push_u32(&mut out, crc32(data));
            push_u32(&mut out, data.len() as u32);
            push_u32(&mut out, data.len() as u32);
            push_u16(&mut out, name_bytes.len() as u16);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0);
            push_u16(&mut out, 0);
            push_u32(&mut out, 0);
            push_u32(&mut out, *offset);
            out.extend_from_slice(name_bytes);
        }
        let dir_size = out.len() - dir_at;
        let count = index.len() as u16;
        push_u32(&mut out, 0x06054b50);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        push_u16(&mut out, count);
        push_u16(&mut out, count);
        push_u32(&mut out, dir_size as u32);
        push_u32(&mut out, dir_at as u32);
        push_u16(&mut out, 0);
        out
    }

    fn request(method: &str, url: &str) -> Vec<u8> {
        format!("{method} {url} HTTP/1.1\r\nHost: example.test\r\n\r\n").into_bytes()
    }

    fn response(status: u16, mime: &str, body: &str) -> Vec<u8> {
        format!("HTTP/1.1 {status} OK\r\nContent-Type: {mime}\r\n\r\n{body}").into_bytes()
    }

    fn session_archive() -> Vec<u8> {
        let post = request("POST", "https://example.test/submit");
        let created = response(201, "application/json", "{\"ok\":1}");
        let get = request("GET", "https://example.test/item");
        let page = response(200, "text/html", "hello");
        zip(&[
            ("index.dat", b"session"),
            ("raw/2_c.txt", &post),
            ("raw/2_s.txt", &created),
            ("raw/10_c.txt", &get),
            ("raw/10_s.txt", &page),
        ])
    }

    fn workspace(label: &str) -> (Rm, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("rllm-capture-file-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let root = dir.canonicalize().unwrap();
        (Rm(dir), root)
    }

    fn access() -> operation::InspectionAccess {
        super::super::super::execution::reset();
        operation::InspectionAccess {
            list: true,
            read: true,
            graph: false,
            budget: 2 * 1024 * 1024,
            root_identity: None,
            cancellation_token: super::super::super::execution::cancellation_token(),
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(120),
        }
    }

    fn inspect_file(root: &std::path::Path, name: &str) -> String {
        inspect_scoped(root, &root.join(name), name, &access()).unwrap_or_else(|error| panic!("{name}: {error}"))
    }

    fn capture_section(shown: &str) -> Json {
        let marker = "Saved capture file\n";
        let at = shown.find(marker).unwrap_or_else(|| panic!("inspect did not publish a saved capture section:\n{shown}"));
        let line = shown[at + marker.len()..].lines().next().unwrap_or("");
        parse(line).unwrap_or_else(|error| panic!("saved capture section is not typed JSON: {error}: {line}"))
    }

    fn entry<'a>(capture: &'a Json, index: usize) -> &'a Json {
        &capture.get("entries").unwrap().array().unwrap()[index]
    }

    fn assert_row(capture: &Json, at: usize, session: &str, method: &str, url: &str, status: &str, mime: &str, bytes: &str) {
        let row = entry(capture, at);
        assert_eq!(row.get("index").unwrap(), &Num(session.into()), "{row:?}");
        assert_eq!(row.get("method").unwrap().text().unwrap(), method, "{row:?}");
        assert_eq!(row.get("url").unwrap().text().unwrap(), url, "{row:?}");
        assert_eq!(row.get("status").unwrap(), &Num(status.into()), "{row:?}");
        assert_eq!(row.get("mime").unwrap().text().unwrap(), mime, "{row:?}");
        assert_eq!(row.get("bytes").unwrap(), &Num(bytes.into()), "{row:?}");
    }

    fn retained(report: &str, path: &str) -> Json {
        let args = obj(vec![("path", s(path))]);
        let mut st = obj(vec![("approval_root", s("/ws")), ("approval_root_identity", s("ws-1"))]);
        let record = obj(vec![
            ("schema", Num("1".into())),
            ("id", s("rec-file")),
            ("recorded_at", s("1")),
            ("tool", s("inspect")),
            ("args", args.clone()),
            ("workspace", s("/ws")),
            ("workspace_identity", s("ws-1")),
            ("policy", harness_rules::observation_binding(&st, "inspect", &args)),
            ("chat", s("c")),
            ("agent", s("a")),
            ("task", s("t")),
            ("mode", s("inspect")),
            ("depth", Num("0".into())),
            ("report", s(report)),
        ]);
        put(&mut st, "investigation_records", obj(vec![("schema", Num("1".into())), ("items", Arr(vec![record]))]));
        let output = observation_records::query(&st, &obj(vec![("id", s("rec-file"))]), false).unwrap_or_else(|error| panic!("{error}"));
        parse(&output).unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn inspect_reads_har_and_saz_files_into_typed_rows() {
        // Fails if inspect never parses the workspace file, if SAZ bytes must be valid
        // UTF-8, if session 10 is ordered before session 2, or if the observations
        // route only recovers rows by treating the rendered report as the archive.
        let (held, root) = workspace("rows");
        let har = br#"{"log":{"version":"1.2","entries":[{"request":{"method":"GET","url":"https://example.test/item"},"response":{"status":200,"content":{"mimeType":"text/html","size":12}}}]}}"#;
        fs::write(root.join("traffic.har"), har).unwrap();
        let saz = session_archive();
        assert!(std::str::from_utf8(&saz).is_err(), "archive fixture was valid UTF-8");
        assert!(saz.len() > 12 && saz[11] >= 0x80, "local header time field has no byte >= 0x80");
        fs::write(root.join("traffic.saz"), &saz).unwrap();
        let har_shown = inspect_file(&root, "traffic.har");
        let har_rows = capture_section(&har_shown);
        assert_eq!(har_rows.get("refusal").unwrap(), &Null);
        assert!(har_rows.get("caps").unwrap().array().unwrap().is_empty());
        assert_eq!(har_rows.get("entries").unwrap().array().unwrap().len(), 1);
        assert_row(&har_rows, 0, "0", "GET", "https://example.test/item", "200", "text/html", "12");
        let har_kept = retained(&har_shown, "traffic.har");
        assert_eq!(har_kept.get("saved_capture").unwrap(), &har_rows);
        assert_eq!(har_kept.get("report").unwrap().text().unwrap(), har_shown);
        let saz_shown = inspect_file(&root, "traffic.saz");
        let saz_rows = capture_section(&saz_shown);
        assert_eq!(saz_rows.get("refusal").unwrap(), &Null);
        assert!(saz_rows.get("caps").unwrap().array().unwrap().is_empty());
        assert_eq!(saz_rows.get("entries").unwrap().array().unwrap().len(), 2);
        assert_row(&saz_rows, 0, "2", "POST", "https://example.test/submit", "201", "application/json", "8");
        assert_row(&saz_rows, 1, "10", "GET", "https://example.test/item", "200", "text/html", "5");
        assert_ne!(saz_shown.as_bytes(), saz.as_slice());
        let saz_kept = retained(&saz_shown, "traffic.saz");
        assert_eq!(saz_kept.get("saved_capture").unwrap(), &saz_rows);
        assert_eq!(saz_kept.get("report").unwrap().text().unwrap(), saz_shown);
        drop(held);
    }

    #[test]
    fn inspect_names_a_non_capture_and_a_truncated_archive() {
        // Fails if a non-capture file is a success with no refusal, or if a cut
        // archive still yields rows or an unnamed error.
        let (held, root) = workspace("refuse");
        fs::write(root.join("notes.har"), b"hello from a plain file\n").unwrap();
        let plain = capture_section(&inspect_file(&root, "notes.har"));
        assert!(plain.get("entries").unwrap().array().unwrap().is_empty(), "{plain:?}");
        assert_eq!(plain.get("refusal").unwrap().text().unwrap(), "Refusal: file is not a saved capture.");
        let mut cut = session_archive();
        cut.truncate(cut.len() - 20);
        assert!(cut.starts_with(b"PK"));
        fs::write(root.join("cut.saz"), &cut).unwrap();
        let truncated = capture_section(&inspect_file(&root, "cut.saz"));
        assert!(truncated.get("entries").unwrap().array().unwrap().is_empty(), "{truncated:?}");
        assert_eq!(
            truncated.get("refusal").unwrap().text().unwrap(),
            "Refusal: saved capture archive directory is corrupt or truncated."
        );
        fs::write(root.join("notes.txt"), b"hello from a plain file\n").unwrap();
        let ordinary = inspect_file(&root, "notes.txt");
        assert!(!ordinary.contains("Saved capture file"), "{ordinary}");
        assert!(!ordinary.contains("Refusal: file is not a saved capture."), "{ordinary}");
        let big = root.join("big.saz");
        fs::File::create(&big).unwrap().set_len(super::super::super::archive::UNPACK_BYTE_LIMIT as u64 + 1).unwrap();
        let oversized = capture_section(&inspect_file(&root, "big.saz"));
        assert!(oversized.get("entries").unwrap().array().unwrap().is_empty(), "{oversized:?}");
        assert_eq!(oversized.get("refusal").unwrap().text().unwrap(), "Refusal: saved capture file exceeds the reader limit.");
        let bad = [0xff, 0xfe, b'{'];
        fs::write(root.join("bad.har"), bad).unwrap();
        let unusable = capture_section(&inspect_file(&root, "bad.har"));
        assert!(unusable.get("entries").unwrap().array().unwrap().is_empty(), "{unusable:?}");
        assert_eq!(unusable.get("refusal").unwrap().text().unwrap(), "Refusal: saved capture JSON is unusable.");
        drop(held);
    }

    fn published_capture(shown: &str) -> Option<Json> {
        let marker = "Saved capture file\n";
        let at = shown.find(marker)?;
        let line = shown[at + marker.len()..].lines().next().unwrap_or("");
        Some(parse(line).unwrap_or_else(|error| panic!("saved capture section is not typed JSON: {error}: {line}")))
    }

    #[test]
    fn uppercase_capture_extension_reaches_the_same_view_as_lowercase() {
        // Fails if .HAR / .Har / .SAZ stay off the capture route, or if notes.TXT grows a capture section.
        let (held, root) = workspace("case");
        let har = br#"{"log":{"version":"1.2","entries":[{"request":{"method":"GET","url":"https://example.test/item"},"response":{"status":200,"content":{"mimeType":"text/html","size":12}}}]}}"#;
        fs::write(root.join("traffic.har"), har).unwrap();
        fs::write(root.join("traffic.HAR"), har).unwrap();
        fs::write(root.join("traffic.Har"), har).unwrap();
        let lower_shown = inspect_file(&root, "traffic.har");
        let upper_shown = inspect_file(&root, "traffic.HAR");
        let mixed_shown = inspect_file(&root, "traffic.Har");
        let lower = published_capture(&lower_shown);
        let upper = published_capture(&upper_shown);
        let mixed = published_capture(&mixed_shown);
        assert!(lower.is_some(), "lowercase traffic.har published no capture section");
        assert_eq!(upper, lower, "traffic.HAR did not reach the same capture view as traffic.har");
        assert_eq!(mixed, lower, "traffic.Har did not reach the same capture view as traffic.har");
        let rows = upper.as_ref().unwrap();
        assert_eq!(rows.get("refusal").unwrap(), &Null);
        assert_row(rows, 0, "0", "GET", "https://example.test/item", "200", "text/html", "12");
        let kept = retained(&upper_shown, "traffic.HAR");
        assert_eq!(kept.get("saved_capture").unwrap(), rows);
        assert_eq!(kept.get("report").unwrap().text().unwrap(), upper_shown);

        let saz = session_archive();
        fs::write(root.join("traffic.saz"), &saz).unwrap();
        fs::write(root.join("traffic.SAZ"), &saz).unwrap();
        let saz_lower = published_capture(&inspect_file(&root, "traffic.saz"));
        let saz_upper = published_capture(&inspect_file(&root, "traffic.SAZ"));
        assert!(saz_lower.is_some(), "lowercase traffic.saz published no capture section");
        assert_eq!(saz_upper, saz_lower, "traffic.SAZ did not reach the same capture view as traffic.saz");
        assert_eq!(saz_upper.as_ref().unwrap().get("entries").unwrap().array().unwrap().len(), 2);

        fs::write(root.join("notes.har"), b"hello from a plain file\n").unwrap();
        fs::write(root.join("notes.HAR"), b"hello from a plain file\n").unwrap();
        let plain_lower = published_capture(&inspect_file(&root, "notes.har"));
        let plain_upper = published_capture(&inspect_file(&root, "notes.HAR"));
        assert_eq!(plain_upper, plain_lower, "notes.HAR did not reach the same refusal as notes.har");
        assert_eq!(
            plain_upper.as_ref().unwrap().get("refusal").unwrap().text().unwrap(),
            "Refusal: file is not a saved capture."
        );

        fs::write(root.join("notes.txt"), b"hello from a plain file\n").unwrap();
        fs::write(root.join("notes.TXT"), b"hello from a plain file\n").unwrap();
        let ordinary = inspect_file(&root, "notes.txt");
        let ordinary_upper = inspect_file(&root, "notes.TXT");
        assert!(!ordinary_upper.contains("Saved capture file"), "{ordinary_upper}");
        assert!(!ordinary_upper.contains("Refusal: file is not a saved capture."), "{ordinary_upper}");
        assert_eq!(
            ordinary_upper.replace("notes.TXT", "notes.txt"),
            ordinary,
            "notes.TXT left today's ordinary-file display"
        );
        drop(held);
    }
}
