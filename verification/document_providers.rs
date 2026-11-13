mod twin;

use twin::{b, n, obj, s, Controller, Value};

const SCOPE: &str = "Owner public core events plus unchanged production DocumentAccess and exact Activity selection-commit method against controlled Android URI/grant/resolver and state-delivery adapters. Actual Android provider account, picker, grant and lifecycle execution requires installed-device acceptance; no application-owned cloud OAuth or SMB is claimed.";
const REPORT: &str = "output/checks/document_providers.json";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = twin::create_scratch(&root, "document-providers");
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let mut controller = Controller::spawn(&root_text);
    let mut passed = run_core(&mut controller, &scratch.to_string_lossy());
    controller.finish();
    passed.extend(run_java(&root, &scratch));
    let _ = std::fs::remove_dir_all(&scratch);

    let body = obj(vec![
        ("passed".into(), n(&passed.len().to_string())),
        ("checks".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("document_providers", &body, true);
    let written = std::fs::read_to_string(REPORT).unwrap_or_else(|error| panic!("{REPORT}: {error}"));
    if !written.contains(SCOPE) || !written.contains("\"passed\": 51") {
        panic!("{REPORT} has the wrong shape");
    }
}

fn run_core(controller: &mut Controller, root: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let resumed = controller.event(None, "resume", obj(vec![]), root);
    let mut state = reply_state(&resumed).clone();
    let storage = controller.event(Some(&state), "workspace_storage", obj(vec![]), root);
    let last_event = match storage
        .get("ui")
        .and_then(|ui| ui.get("modal"))
        .and_then(|modal| modal.get("buttons"))
    {
        Some(Value::Arr(buttons)) => buttons.last().and_then(|button| match button.get("event") {
            Some(Value::Str(text)) => Some(text.clone()),
            _ => None,
        }),
        _ => None,
    };
    keep(
        &mut passed,
        "provider navigation is generic",
        last_event.as_deref() == Some("document_providers"),
    );
    state = reply_state(&storage).clone();

    for action in [
        "document_providers",
        "google_drive",
        "cloud_setup",
        "workspace_permissions",
    ] {
        let reply = controller.event(Some(&state), action, obj(vec![]), root);
        let name = format!("provider-neutral visible controls {action}");
        let neutral = match reply.get("ui") {
            Some(ui) => !ui.to_string().to_lowercase().contains("google"),
            None => false,
        };
        keep(&mut passed, &name, neutral);
        state = reply_state(&reply).clone();
    }

    let sign_in = controller.event(Some(&state), "cloud_sign_in", obj(vec![]), root);
    state = reply_state(&sign_in).clone();
    keep(
        &mut passed,
        "account setup has no provider credential contract",
        sign_in.get("effect") == Some(&obj(vec![("kind".into(), s("cloud_account"))])),
    );

    let first = "content://one.example.documents/document/first";
    let second = "content://another.provider/document/second";
    let selected = choose(controller, &state, first, root);
    state = reply_state(&selected).clone();
    let selected = choose(controller, &state, second, root);
    state = reply_state(&selected).clone();
    keep(
        &mut passed,
        "two unrelated providers retain actual selected grant ownership",
        field(&state, "cloud_grants") == &Value::Arr(vec![s(first), s(second)])
            && field(&state, "cloud_document").get("uri") == Some(&s(second)),
    );

    let opened = controller.event(Some(&state), "cloud_open", obj(vec![]), root);
    state = reply_state(&opened).clone();
    keep(
        &mut passed,
        "external opening creates no pending read owner",
        opened.get("effect").and_then(|effect| effect.get("kind")).map(Value::str_) == Some("cloud_open")
            && absent(&state, "cloud_expected"),
    );

    let read = controller.event(Some(&state), "cloud_read", obj(vec![]), root);
    state = reply_state(&read).clone();
    let read_ticket = read
        .get("effect")
        .and_then(|effect| effect.get("ticket"))
        .map(Value::str_)
        .unwrap_or_else(|| panic!("cloud_read produced no ticket"))
        .to_string();
    let owner = field(&state, "cloud_expected").clone();
    let browse = controller.event(Some(&state), "cloud_browse", obj(vec![]), root);
    state = reply_state(&browse).clone();
    keep(
        &mut passed,
        "new picker cannot orphan a document read",
        browse.get("effect") == Some(&Value::Null)
            && state.get("busy") == Some(&b(true))
            && field(&state, "cloud_expected") == &owner,
    );

    let disconnect = controller.event(Some(&state), "cloud_disconnect", obj(vec![]), root);
    state = reply_state(&disconnect).clone();
    let release = disconnect.get("effect").unwrap_or_else(|| panic!("cloud_disconnect produced no effect")).clone();
    let release_ticket = release.get("ticket").map(Value::str_).unwrap_or_else(|| panic!("release has no ticket")).to_string();
    keep(
        &mut passed,
        "disconnect settles only its owned read",
        state.get("busy") == Some(&b(false))
            && release.get("uris") == Some(&Value::Arr(vec![s(first), s(second)]))
            && field(&state, "cloud_grants") == &Value::Arr(vec![s(first), s(second)]),
    );

    let stale = controller.event(
        Some(&state),
        "cloud_text_result",
        obj(vec![
            ("ticket".into(), s(&read_ticket)),
            ("content".into(), s("STALE_READ")),
        ]),
        root,
    );
    state = reply_state(&stale).clone();
    keep(
        &mut passed,
        "disconnect retires late read text",
        !stale.to_string().contains("STALE_READ"),
    );

    let failed_release = controller.event(
        Some(&state),
        "cloud_release_result",
        obj(vec![
            ("ticket".into(), s(&release_ticket)),
            ("cloud_response".into(), obj(vec![("released".into(), Value::Arr(vec![s(first)]))])),
        ]),
        root,
    );
    state = reply_state(&failed_release).clone();
    keep(
        &mut passed,
        "failed grant release remains retryable while access is off",
        field(&state, "cloud_grants") == &Value::Arr(vec![s(second)])
            && state
                .get("storage_access")
                .and_then(|access| access.get("cloud"))
                == Some(&b(false))
            && state.get("notice").map(Value::str_).unwrap_or("").contains("retry"),
    );

    let retry = controller.event(Some(&state), "cloud_disconnect", obj(vec![]), root);
    state = reply_state(&retry).clone();
    let retry_ticket = retry
        .get("effect")
        .and_then(|effect| effect.get("ticket"))
        .map(Value::str_)
        .unwrap_or_else(|| panic!("retry produced no release ticket"))
        .to_string();
    let released = controller.event(
        Some(&state),
        "cloud_release_result",
        obj(vec![
            ("ticket".into(), s(&retry_ticket)),
            ("cloud_response".into(), obj(vec![("released".into(), Value::Arr(vec![s(second)]))])),
        ]),
        root,
    );
    state = reply_state(&released).clone();
    keep(
        &mut passed,
        "successful retry removes only confirmed grant records",
        field(&state, "cloud_grants") == &Value::Arr(vec![])
            && absent(&state, "cloud_expected"),
    );

    let snapshot = state.clone();
    let late = controller.event(
        Some(&snapshot),
        "cloud_release_result",
        obj(vec![
            ("ticket".into(), s(&retry_ticket)),
            (
                "cloud_response".into(),
                obj(vec![(
                    "released".into(),
                    Value::Arr(vec![s(first), s(second)]),
                )]),
            ),
        ]),
        root,
    );
    keep(
        &mut passed,
        "late cleanup callback preserves state exactly",
        reply_state(&late) == &snapshot && late.get("effect") == Some(&Value::Null),
    );

    let resumed = controller.event(None, "resume", obj(vec![]), root);
    let mut owned = reply_state(&resumed).clone();
    let selected = choose(controller, &owned, first, root);
    owned = reply_state(&selected).clone();
    put(
        &mut owned,
        "profiles",
        Value::Arr(vec![obj(vec![
            ("name".into(), s("Configured primary")),
            ("model".into(), s("primary")),
            ("endpoint".into(), s("http://primary.test/model")),
            ("plain".into(), b(true)),
            ("stream".into(), b(false)),
            ("protocol".into(), s("chat")),
        ])]),
    );
    let sent = controller.event(
        Some(&owned),
        "send",
        obj(vec![("draft".into(), s("Keep this primary request active."))]),
        root,
    );
    owned = reply_state(&sent).clone();
    let model = field(&owned, "model_expected").clone();
    let read = controller.event(Some(&owned), "cloud_read", obj(vec![]), root);
    owned = reply_state(&read).clone();
    keep(
        &mut passed,
        "document read cannot replace an active model owner",
        read.get("effect") == Some(&Value::Null)
            && owned.get("busy") == Some(&b(true))
            && field(&owned, "model_expected") == &model,
    );

    let disconnect = controller.event(Some(&owned), "cloud_disconnect", obj(vec![]), root);
    owned = reply_state(&disconnect).clone();
    let release_ticket = disconnect
        .get("effect")
        .and_then(|effect| effect.get("ticket"))
        .map(Value::str_)
        .unwrap_or_else(|| panic!("disconnect produced no release ticket"))
        .to_string();
    let released = controller.event(
        Some(&owned),
        "cloud_release_result",
        obj(vec![
            ("ticket".into(), s(&release_ticket)),
            ("cloud_response".into(), obj(vec![("released".into(), Value::Arr(vec![s(first)]))])),
        ]),
        root,
    );
    owned = reply_state(&released).clone();
    keep(
        &mut passed,
        "grant cleanup retains unrelated primary work",
        owned.get("busy") == Some(&b(true)) && field(&owned, "model_expected") == &model,
    );

    for uri in [
        "content://",
        "content:///document/empty",
        "content://host",
        "content://host/",
        "content://user@host/document/a",
        "content://host/document/a#fragment",
        "content://host/document/a\0",
        "file:///tmp/a",
        "https://host/document/a",
    ] {
        let resumed = controller.event(None, "resume", obj(vec![]), root);
        let before = py_get(reply_state(&resumed), "cloud_document").cloned();
        let selected = choose(controller, reply_state(&resumed), uri, root);
        let selected_state = reply_state(&selected);
        keep(
            &mut passed,
            &format!("invalid URI cannot become selected {}", repr(uri)),
            is_null(selected.get("effect")) && py_get(selected_state, "cloud_document") == before.as_ref(),
        );
    }

    let resumed = controller.event(None, "resume", obj(vec![]), root);
    let mut bounded = reply_state(&resumed).clone();
    let grants = (0..63)
        .map(|index| s(&format!("content://owned.example/document/{index}")))
        .collect();
    put(&mut bounded, "cloud_grants", Value::Arr(grants));
    let selected = choose(controller, &bounded, second, root);
    bounded = reply_state(&selected).clone();
    keep(
        &mut passed,
        "grant ownership remains bounded at64",
        field(&bounded, "cloud_grants").arr().len() == 64,
    );
    let browse = controller.event(Some(&bounded), "cloud_browse", obj(vec![]), root);
    keep(
        &mut passed,
        "full grant allowance refuses another picker without discarding grants",
        is_null(browse.get("effect"))
            && match reply_state(&browse).get("cloud_grants") {
                Some(Value::Arr(items)) => items.len() == 64,
                _ => false,
            },
    );
    passed
}

fn choose(controller: &mut Controller, state: &Value, uri: &str, root: &str) -> Value {
    let browse = controller.event(Some(state), "cloud_browse", obj(vec![]), root);
    let kind_ok = matches!(
        browse.get("effect").and_then(|effect| effect.get("kind")),
        Some(Value::Str(kind)) if kind == "cloud_document"
    );
    if !kind_ok {
        panic!("cloud_browse returned the wrong effect: {}", browse.to_string());
    }
    let ticket = browse
        .get("effect")
        .and_then(|effect| effect.get("ticket"))
        .cloned()
        .unwrap_or_else(|| panic!("cloud_browse produced no ticket"));
    let next = reply_state(&browse).clone();
    controller.event(
        Some(&next),
        "cloud_result",
        obj(vec![
            ("ticket".into(), ticket),
            (
                "cloud_response".into(),
                obj(vec![
                    ("uri".into(), s(uri)),
                    ("name".into(), s("selected.txt")),
                    ("mime".into(), s("text/plain")),
                ]),
            ),
        ]),
        root,
    )
}

fn run_java(root: &std::path::Path, scratch: &std::path::Path) -> Vec<String> {
    let source_root = scratch.join("java");
    let mut sources = Vec::new();
    write_source(
        &source_root.join("android/net/Uri.java"),
        r#"package android.net;public final class Uri{String raw;java.net.URI uri;private Uri(String s){raw=s;try{uri=new java.net.URI(s);}catch(Exception ignored){}}public static Uri parse(String s){return new Uri(s);}public String getScheme(){return uri==null?null:uri.getScheme();}public String getAuthority(){return uri==null?null:uri.getAuthority();}public String getPath(){return uri==null?null:uri.getPath();}public String getFragment(){return uri==null?null:uri.getFragment();}public String toString(){return raw;}public boolean equals(Object o){return o instanceof Uri&&raw.equals(((Uri)o).raw);}public int hashCode(){return raw.hashCode();}}"#,
        &mut sources,
    );
    write_source(
        &source_root.join("android/content/Intent.java"),
        "package android.content;public class Intent{public static final int FLAG_GRANT_READ_URI_PERMISSION=1,FLAG_GRANT_PERSISTABLE_URI_PERMISSION=64;}",
        &mut sources,
    );
    write_source(
        &source_root.join("android/content/UriPermission.java"),
        "package android.content;import android.net.Uri;public class UriPermission{Uri uri;boolean read,write;public UriPermission(String u,boolean r,boolean w){uri=Uri.parse(u);read=r;write=w;}public Uri getUri(){return uri;}public boolean isReadPermission(){return read;}public boolean isWritePermission(){return write;}}",
        &mut sources,
    );
    write_source(
        &source_root.join("android/provider/DocumentsContract.java"),
        "package android.provider;import android.net.Uri;public class DocumentsContract{public static boolean isTreeUri(Uri uri){return uri.getPath().startsWith(\"/tree/\");}}",
        &mut sources,
    );
    write_source(
        &source_root.join("android/content/ContentResolver.java"),
        r#"package android.content;import android.net.Uri;import java.util.*;public class ContentResolver{public List<UriPermission> grants=new ArrayList<>();public List<String> releases=new ArrayList<>(),takes=new ArrayList<>();public Set<String> denied=new HashSet<>();public List<UriPermission> getPersistedUriPermissions(){return new ArrayList<>(grants);}public void releasePersistableUriPermission(Uri uri,int flags){if(flags!=1)throw new AssertionError("non-read grant released");if(denied.contains(uri.toString()))throw new SecurityException();releases.add(uri.toString());grants.removeIf(g->g.getUri().equals(uri));}public void takePersistableUriPermission(Uri uri,int flags){if(flags!=1)throw new AssertionError("non-read grant taken");takes.add(uri.toString());if(grants.stream().noneMatch(g->g.getUri().equals(uri)))grants.add(new UriPermission(uri.toString(),true,false));}}"#,
        &mut sources,
    );
    write_source(
        &source_root.join("org/json/JSONArray.java"),
        "package org.json;import java.util.*;public class JSONArray{List<String> values=new ArrayList<>();public JSONArray put(Object value){values.add(String.valueOf(value));return this;}public int length(){return values.size();}public String optString(int i){return i>=0&&i<values.size()?values.get(i):\"\";}public String toString(){return values.toString();}}",
        &mut sources,
    );
    write_source(
        &source_root.join("org/json/JSONException.java"),
        "package org.json;public class JSONException extends Exception{}",
        &mut sources,
    );
    write_source(
        &source_root.join("org/json/JSONObject.java"),
        "package org.json;import java.util.*;public class JSONObject{Map<String,Object> values=new HashMap<>();public JSONObject put(String k,Object v)throws JSONException{values.put(k,v);return this;}public String optString(String k){Object v=values.get(k);return v instanceof String?(String)v:\"\";}public JSONObject optJSONObject(String k){Object v=values.get(k);return v instanceof JSONObject?(JSONObject)v:null;}}",
        &mut sources,
    );

    let activity = std::fs::read_to_string(root.join("platform/com/local/client/Activity.java"))
        .unwrap_or_else(|error| panic!("Activity.java: {error}"));
    let start = activity
        .find("    private void completeCloudSelection(")
        .unwrap_or_else(|| panic!("Activity.java has no completeCloudSelection"));
    let end = activity[start..]
        .find("    private boolean sourceAllowed(")
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("Activity.java has no sourceAllowed after completeCloudSelection"));
    let selection = &activity[start..end];

    let java = format!(
        r#"package com.local.client;import android.net.*;import android.content.*;import org.json.*;import java.util.*;
public class DocumentChecks{{
 boolean closed,phoneStorageAccess=true,cloudStorageAccess=true,adopt=true;String cloudOwner="current";JSONObject state=new JSONObject();ContentResolver resolver=new ContentResolver();int deliveries;String last="";
 ContentResolver getContentResolver(){{return resolver;}}private void selectedDocumentAccess(Uri uri)throws java.io.IOException{{DocumentAccess.check(uri,phoneStorageAccess,cloudStorageAccess,true);}}private String safe(Exception e){{return e.getClass().getName();}}private JSONObject cloudResponse(String ticket,JSONObject response)throws JSONException{{return new JSONObject().put("ticket",ticket).put("cloud_response",response);}}private void dispatch(String action,JSONObject payload){{deliveries++;last=action;if(adopt&&action.equals("cloud_result"))try{{state.put("cloud_document",payload.optJSONObject("cloud_response"));cloudOwner="";}}catch(Exception e){{throw new RuntimeException(e);}}}}
 {selection}
 static List<String> passed=new ArrayList<>();static void check(String n,boolean v){{if(!v)throw new AssertionError(n);passed.add(n);}}static boolean blocked(String uri,boolean phone,boolean providers,boolean selected){{try{{DocumentAccess.check(Uri.parse(uri),phone,providers,selected);return false;}}catch(Exception expected){{return true;}}}}
 public static void main(String[] args)throws Exception{{
  String remote="content://unregistered.example/document/source",other="content://second.provider/document/other",local="content://com.android.externalstorage.documents/document/primary%3Asource.txt",tree="content://unregistered.example/tree/workspace",shared="content://unregistered.example/document/shared";
  check("unknown providers are accepted without package discovery",!blocked(remote,false,true,true));check("another provider uses the same native path",!blocked(other,false,true,true));check("unknown provider cannot bypass disabled provider access",blocked(remote,true,false,false));check("local phone file retains its separate phone policy",blocked(local,false,true,false)&&!blocked(local,true,false,false));check("explicit provider selection also requires provider access for a local file",blocked(local,true,false,true));
  for(String uri:new String[]{{"content://","content:///document/a","content://host/","content://user@host/document/a","content://host/document/a#part","file:///tmp/a","content://host/document/a\n"}})check("native URI refusal "+uri.replace("\n","newline"),blocked(uri,true,true,true));
  ContentResolver resolver=new ContentResolver();resolver.grants.add(new UriPermission(remote,true,false));resolver.grants.add(new UriPermission(other,true,false));resolver.grants.add(new UriPermission(tree,true,true));resolver.grants.add(new UriPermission(shared,true,true));resolver.grants.add(new UriPermission(local,true,false));resolver.denied.add(other);
  JSONArray owned=new JSONArray().put(remote).put(other).put(tree).put(shared).put(local);JSONArray released=DocumentAccess.release(resolver,owned,local);check("cleanup releases only its owned standalone read permission",resolver.releases.equals(Arrays.asList(remote)));check("cleanup preserves unrelated writable tree and selected phone grants",resolver.grants.size()==4);check("failed cleanup remains absent from confirmation",released.length()==4&&!released.toString().contains(other));resolver.denied.clear();released=DocumentAccess.release(resolver,new JSONArray().put(other),local);check("failed grant cleanup succeeds on explicit retry",released.length()==1&&resolver.releases.equals(Arrays.asList(remote,other)));check("already missing grant cleanup is idempotent",DocumentAccess.release(resolver,new JSONArray().put(remote),local).length()==1);
  Uri uri=Uri.parse(remote);JSONObject response=new JSONObject().put("cloud_response",new JSONObject().put("uri",remote));DocumentChecks current=new DocumentChecks();current.completeCloudSelection("current",uri,65,response);check("actual selection persists and adopts a current read grant",current.resolver.takes.equals(Arrays.asList(remote))&&current.state.optJSONObject("cloud_document").optString("uri").equals(remote));
  DocumentChecks stale=new DocumentChecks();stale.completeCloudSelection("retired",uri,65,response);check("retired selection cannot persist any URI grant",stale.resolver.takes.isEmpty()&&stale.deliveries==0);stale.closed=true;stale.completeCloudSelection("current",uri,65,response);check("closed activity cannot persist a returned grant",stale.resolver.takes.isEmpty());
  DocumentChecks revoked=new DocumentChecks();revoked.cloudStorageAccess=false;revoked.completeCloudSelection("current",uri,65,response);check("permission revocation before UI delivery refuses persistence",revoked.resolver.takes.isEmpty()&&revoked.last.equals("cloud_failure"));
  DocumentChecks refused=new DocumentChecks();refused.adopt=false;refused.completeCloudSelection("current",uri,65,response);check("failed core adoption releases only the newly acquired read grant",refused.resolver.releases.equals(Arrays.asList(remote))&&refused.resolver.grants.isEmpty());
  DocumentChecks existing=new DocumentChecks();existing.adopt=false;existing.resolver.grants.add(new UriPermission(remote,true,false));existing.completeCloudSelection("current",uri,65,response);check("failed adoption preserves a pre-existing read grant",existing.resolver.releases.isEmpty()&&existing.resolver.grants.size()==1);
  DocumentChecks temporary=new DocumentChecks();temporary.completeCloudSelection("current",uri,1,response);check("temporary provider permission is usable without invented persistence",temporary.resolver.takes.isEmpty()&&temporary.state.optJSONObject("cloud_document")!=null);
  for(String name:passed)System.out.println(name);
 }}
}}"#
    );
    write_source(
        &source_root.join("com/local/client/DocumentChecks.java"),
        &java,
        &mut sources,
    );
    sources.push(root.join("platform/com/local/client/DocumentAccess.java"));

    let jdk = std::env::var_os("CLIENT_JDK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| panic!("CLIENT_JDK is not set"));
    let classes = scratch.join("classes");
    std::fs::create_dir_all(&classes).unwrap_or_else(|error| panic!("classes: {error}"));
    let mut compile = std::process::Command::new(jdk.join("bin/javac"));
    compile.arg("--release").arg("8").arg("-d").arg(&classes);
    for source in &sources {
        compile.arg(source);
    }
    let output = compile.output().unwrap_or_else(|error| panic!("javac: {error}"));
    if !output.status.success() {
        panic!("javac failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    let output = std::process::Command::new(jdk.join("bin/java"))
        .arg("-cp")
        .arg(&classes)
        .arg("com.local.client.DocumentChecks")
        .output()
        .unwrap_or_else(|error| panic!("java: {error}"));
    if !output.status.success() {
        panic!("DocumentChecks failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("DocumentChecks output: {error}"))
        .lines()
        .map(str::to_string)
        .collect()
}

fn write_source(path: &std::path::Path, body: &str, sources: &mut Vec<std::path::PathBuf>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap_or_else(|error| panic!("{}: {error}", path.parent().unwrap().display()));
    std::fs::write(path, body).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    sources.push(path.to_path_buf());
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn reply_state(value: &Value) -> &Value {
    value.get("state").unwrap_or_else(|| panic!("event returned no state: {}", value.to_string()))
}

fn absent(value: &Value, key: &str) -> bool {
    matches!(value.get(key), None | Some(Value::Null))
}

fn is_null(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Null))
}

fn py_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value.get(key) {
        None | Some(Value::Null) => None,
        Some(other) => Some(other),
    }
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
}

fn put(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else {
        panic!("expected object");
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn repr(value: &str) -> String {
    let mut out = String::from("'");
    for ch in value.chars() {
        match ch {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\x00"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('\'');
    out
}