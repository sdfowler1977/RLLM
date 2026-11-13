mod twin;

use twin::Value;

const SCOPE: &str = "Owner execution of the unchanged production VoiceSession and exact Activity event-input guard against controlled Android notification/service/permission/clock/wake/lifecycle adapters. Separate full Android35SDK compilation passes; this is not physical screen-off, battery, route, notification or lifecycle acceptance.";
const REPORT: &str = "output/checks/voice_session.json";

const CHECKS: [&str; 34] = [
    "visible current UI contributes actual input",
    "background callbacks cannot replay stale form or draft",
    "first activity return preserves core state before UI refresh",
    "refreshed visible UI resumes ordinary input capture",
    "missing session intent cannot crash or start capture",
    "background cannot create a microphone service",
    "missing runtime permission refuses only background startup",
    "disabled notifications prevent background capture",
    "disabled voice channel also prevents capture",
    "expired and oversized leases are refused",
    "platform start denial remains scoped",
    "failed start retires pending ownership",
    "startup waits for actual service admission",
    "service never requests process restart",
    "actual service admission reaches microphone owner",
    "current service declares microphone and playback types",
    "screen-off wake lease is bounded and non-reference-counted",
    "notification stop is immutable and session-bound",
    "existing background service can retain its current capture owner",
    "old notification cannot stop current service",
    "unchanged voice status does not flood notifications",
    "owned Stop requests teardown",
    "teardown releases audio owner and wake resources",
    "prior session notification cannot stop replacement",
    "notification removal stops background audio through health check",
    "expiry stops service without extending consent",
    "cold process cannot replay a saved start intent",
    "failed foreground admission never starts microphone",
    "changed conversation session retires current service",
    "task removal stops optional service",
    "platform health exception stops only the optional service",
    "platform foreground cleanup exception still releases wake and owner",
    "supported legacy foreground overload 26",
    "supported legacy foreground overload 29",
];

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let passed = run(&root);

    let body = twin::obj(vec![
        ("passed".into(), twin::n(&passed.len().to_string())),
        (
            "checks".into(),
            Value::Arr(passed.iter().map(|name| twin::s(name)).collect()),
        ),
        ("scope".into(), twin::s(SCOPE)),
    ]);
    twin::report_value("voice_session", &body, true);

    let written = std::fs::read_to_string(REPORT)
        .unwrap_or_else(|error| panic!("{REPORT}: {error}"));
    let expected = twin::pretty(&body);
    if written.trim_end() != expected.trim_end() {
        panic!("{REPORT} does not match the voice_session report");
    }
}

fn run(root: &std::path::Path) -> Vec<String> {
    let scratch = Scratch::create(root);
    let mut passed = Vec::new();

    let source = std::fs::read_to_string(root.join("platform/com/local/client/Activity.java"))
        .unwrap_or_else(|error| panic!("could not read Activity.java: {error}"));
    let input_start = source
        .find("    private JSONObject eventInputs()")
        .unwrap_or_else(|| panic!("Activity.eventInputs() not found"));
    let input_end = source[input_start..]
        .find("    private void dispatch(String action)")
        .map(|offset| input_start + offset)
        .unwrap_or_else(|| panic!("Activity.dispatch() not found"));
    let input_method = &source[input_start..input_end];

    let java = format!(
        r#"package com.local.client;
import android.app.*;import android.content.*;import android.content.pm.*;import android.os.*;import org.json.*;import java.util.*;
public class VoiceSessionChecks{{
 private boolean ratePaused,uiStale;private int inputReads;private JSONObject inputs(){{inputReads++;return new JSONObject().put("draft","visible draft");}}
 {input_method}
 static List<String> passed=new ArrayList<>();static void check(String n,boolean v){{if(!v)throw new AssertionError(n);passed.add(n);}}
 static JSONObject effect(String id,long lease){{return new JSONObject().put("voice_session",id).put("voice_lease_ms",lease);}}
 static JSONObject state(String id){{return new JSONObject().put("voice_open",true).put("voice_consent",id).put("voice_settings",new JSONObject().put("background",true));}}
 static boolean refused(Activity a,JSONObject e){{try{{VoiceSession.start(a,e);return false;}}catch(Exception expected){{return true;}}}}
 public static void main(String[] args)throws Exception{{
  VoiceSessionChecks inputs=new VoiceSessionChecks();check("visible current UI contributes actual input",inputs.eventInputs().optString("draft").equals("visible draft")&&inputs.inputReads==1);inputs.ratePaused=true;check("background callbacks cannot replay stale form or draft",inputs.eventInputs().optString("draft").isEmpty()&&inputs.inputReads==1);inputs.ratePaused=false;inputs.uiStale=true;check("first activity return preserves core state before UI refresh",inputs.eventInputs().optString("draft").isEmpty()&&inputs.inputReads==1);inputs.uiStale=false;check("refreshed visible UI resumes ordinary input capture",inputs.eventInputs().optString("draft").equals("visible draft")&&inputs.inputReads==2);
  Activity a=new Activity();VoiceSession malformed=new VoiceSession();malformed.onStartCommand(new Intent(a,VoiceSession.class),0,1);check("missing session intent cannot crash or start capture",malformed.stopped&&a.ready==0);malformed.onDestroy();a.visible=false;check("background cannot create a microphone service",refused(a,effect("one",10000)));a.visible=true;
  a.permission=-1;check("missing runtime permission refuses only background startup",refused(a,effect("one",10000)));a.permission=0;
  Context.manager.enabled=false;check("disabled notifications prevent background capture",refused(a,effect("one",10000)));Context.manager.enabled=true;
  Context.manager.createNotificationChannel(new NotificationChannel("voice","Voice",0));check("disabled voice channel also prevents capture",refused(a,effect("one",10000)));Context.manager.channels.clear();
  check("expired and oversized leases are refused",refused(a,effect("one",0))&&refused(a,effect("one",600001)));
  a.launchFailure=true;check("platform start denial remains scoped",refused(a,effect("one",10000)));a.launchFailure=false;
  check("failed start retires pending ownership",!VoiceSession.pending(a,"one"));
  VoiceSession.start(a,effect("one",10000));Intent first=a.launched;check("startup waits for actual service admission",VoiceSession.pending(a,"one")&&!VoiceSession.owns(a,"one")&&a.ready==0);
  VoiceSession s=new VoiceSession();check("service never requests process restart",s.onStartCommand(first,0,1)==Service.START_NOT_STICKY);
  check("actual service admission reaches microphone owner",VoiceSession.owns(a,"one")&&a.ready==1);
  check("current service declares microphone and playback types",s.types==(ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE|ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK));
  check("screen-off wake lease is bounded and non-reference-counted",Context.power.last.isHeld()&&!Context.power.last.referenceCounted&&Context.power.last.until==SystemClock.now+10000);
  PendingIntent stop=s.notification.actions.get(0).intent;check("notification stop is immutable and session-bound",(stop.flags&PendingIntent.FLAG_IMMUTABLE)!=0&&stop.intent.getStringExtra("session").equals("one")&&stop.intent.data.value.contains("one"));
  int ready=a.ready;a.visible=false;s.onStartCommand(first,0,2);check("existing background service can retain its current capture owner",a.ready==ready+1&&VoiceSession.owns(a,"one"));
  s.onStartCommand(new Intent(a,VoiceSession.class).setAction("stop").putExtra("session","old"),0,3);check("old notification cannot stop current service",!s.stopped);
  int updates=Context.manager.updates;VoiceSession.reconcile(a,state("one"),"Listening");VoiceSession.reconcile(a,state("one"),"Listening");check("unchanged voice status does not flood notifications",Context.manager.updates==updates+1);
  s.onStartCommand(stop.intent,0,4);check("owned Stop requests teardown",s.stopped);s.onDestroy();check("teardown releases audio owner and wake resources",a.ended==1&&!VoiceSession.owns(a,"one")&&!Context.power.last.isHeld());
  a.visible=true;VoiceSession.start(a,effect("two",5000));VoiceSession next=new VoiceSession();next.onStartCommand(a.launched,0,1);next.onStartCommand(stop.intent,0,2);check("prior session notification cannot stop replacement",!next.stopped&&VoiceSession.owns(a,"two"));
  Context.manager.channels.get("voice").importance=0;Handler.advance(2000);check("notification removal stops background audio through health check",next.stopped);next.onDestroy();Context.manager.channels.clear();
  VoiceSession.start(a,effect("three",1000));VoiceSession expiring=new VoiceSession();expiring.onStartCommand(a.launched,0,1);Handler.advance(1001);check("expiry stops service without extending consent",expiring.stopped&&!VoiceSession.owns(a,"three"));expiring.onDestroy();
  VoiceSession cold=new VoiceSession();int before=a.ready;cold.onStartCommand(first,0,1);check("cold process cannot replay a saved start intent",cold.stopped&&a.ready==before);cold.onDestroy();
  VoiceSession.start(a,effect("four",5000));VoiceSession denied=new VoiceSession();denied.denyForeground=true;denied.onStartCommand(a.launched,0,1);check("failed foreground admission never starts microphone",denied.stopped&&a.ready==before&&!VoiceSession.owns(a,"four"));denied.onDestroy();
  VoiceSession.start(a,effect("five",5000));VoiceSession changed=new VoiceSession();changed.onStartCommand(a.launched,0,1);VoiceSession.reconcile(a,state("another"),"Listening");check("changed conversation session retires current service",changed.stopped);changed.onDestroy();
  VoiceSession.start(a,effect("six",5000));VoiceSession removed=new VoiceSession();removed.onStartCommand(a.launched,0,1);removed.onTaskRemoved(first);check("task removal stops optional service",removed.stopped);removed.onDestroy();
  VoiceSession.start(a,effect("seven",5000));VoiceSession guarded=new VoiceSession();guarded.onStartCommand(a.launched,0,1);Context.manager.throwRead=true;Handler.advance(2000);check("platform health exception stops only the optional service",guarded.stopped);Context.manager.throwRead=false;guarded.denyStop=true;guarded.onDestroy();check("platform foreground cleanup exception still releases wake and owner",!VoiceSession.owns(a,"seven")&&!Context.power.last.isHeld());
  for(int sdk:new int[]{{26,29}}){{Build.VERSION.SDK_INT=sdk;String id="sdk-"+sdk;VoiceSession.start(a,effect(id,5000));VoiceSession legacy=new VoiceSession();legacy.onStartCommand(a.launched,0,1);check("supported legacy foreground overload "+sdk,VoiceSession.owns(a,id)&&legacy.types==(sdk==29?ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK:0));VoiceSession.release(a);legacy.onDestroy();}}
  for(String name:passed)System.out.println(name);
 }}
}}
"#,
        input_method = input_method
    );

    let stubs = stub_sources();
    for (name, body) in &stubs {
        let path = scratch.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap_or_else(|| panic!("stub has no parent")))
            .unwrap_or_else(|error| panic!("could not create stub directory: {error}"));
        std::fs::write(&path, body)
            .unwrap_or_else(|error| panic!("could not write {}: {error}", path.display()));
    }

    let checks_path = scratch.path().join("com/local/client/VoiceSessionChecks.java");
    std::fs::create_dir_all(checks_path.parent().unwrap_or_else(|| panic!("checks has no parent")))
        .unwrap_or_else(|error| panic!("could not create checks directory: {error}"));
    std::fs::write(&checks_path, java)
        .unwrap_or_else(|error| panic!("could not write {}: {error}", checks_path.display()));

    let production = root.join("platform/com/local/client/VoiceSession.java");
    let jdk = std::env::var("CLIENT_JDK").unwrap_or_else(|_| panic!("CLIENT_JDK is not set"));
    let jdk = std::path::Path::new(&jdk);
    let javac = jdk.join("bin/javac");
    let java = jdk.join("bin/java");

    let mut javac_args = vec![
        javac.to_string_lossy().into_owned(),
        "--release".into(),
        "8".into(),
        "-d".into(),
        scratch.path().to_string_lossy().into_owned(),
    ];
    for (name, _) in &stubs {
        javac_args.push(scratch.path().join(name).to_string_lossy().into_owned());
    }
    javac_args.push(checks_path.to_string_lossy().into_owned());
    javac_args.push(production.to_string_lossy().into_owned());
    let compiled = std::process::Command::new(&javac_args[0])
        .args(&javac_args[1..])
        .output()
        .unwrap_or_else(|error| panic!("could not start javac: {error}"));
    if !compiled.status.success() {
        panic!(
            "javac failed: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
    }

    // The leg runs the Java subject with subprocess.run(timeout=20). std's Command::output
    // has no bound, so a hung subject would hang the twin instead of failing it.
    let mut subject = std::process::Command::new(java)
        .arg("-cp")
        .arg(scratch.path())
        .arg("com.local.client.VoiceSessionChecks")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("could not start java: {error}"));
    let stdout_pipe = subject.stdout.take().expect("java stdout pipe");
    let stderr_pipe = subject.stderr.take().expect("java stderr pipe");
    let stdout_thread = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut pipe = stdout_pipe;
        std::io::Read::read_to_end(&mut pipe, &mut bytes).ok();
        bytes
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut pipe = stderr_pipe;
        std::io::Read::read_to_end(&mut pipe, &mut bytes).ok();
        bytes
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let status = loop {
        match subject.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = subject.kill();
                    let _ = subject.wait();
                    panic!("voice session subject exceeded its 20 second bound");
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => panic!("could not wait for java: {error}"),
        }
    };
    let success = status.success();
    let stdout_bytes = stdout_thread.join().unwrap_or_default();
    let stderr_bytes = stderr_thread.join().unwrap_or_default();
    if !success {
        panic!(
            "voice session checks failed: {}",
            String::from_utf8_lossy(&stderr_bytes)
        );
    }

    let output = String::from_utf8(stdout_bytes)
        .unwrap_or_else(|error| panic!("voice session output is not UTF-8: {error}"));
    let names: Vec<&str> = output.lines().collect();
    if names.len() != CHECKS.len() {
        panic!(
            "voice session produced {} checks, expected {}",
            names.len(),
            CHECKS.len()
        );
    }
    for (index, (got, expected)) in names.iter().zip(CHECKS.iter()).enumerate() {
        if got != expected {
            panic!("voice session check {index}: got {got}, expected {expected}");
        }
        passed.push((*expected).to_string());
    }
    passed
}

fn stub_sources() -> Vec<(&'static str, &'static str)> {
    vec![
        ("android/R.java", "package android;public class R{public static class drawable{public static int ic_btn_speak_now=1;}}"),
        ("android/os/IBinder.java", "package android.os;public interface IBinder{}"),
        ("android/os/Build.java", "package android.os;public class Build{public static class VERSION{public static int SDK_INT=35;}}"),
        ("android/os/SystemClock.java", "package android.os;public class SystemClock{public static long now=1000;public static long elapsedRealtime(){return now;}}"),
        ("android/os/Looper.java", "package android.os;public class Looper{public static Looper getMainLooper(){return new Looper();}}"),
        ("android/os/Handler.java", r#"package android.os;import java.util.*;public class Handler{static class Job{Handler h;Runnable r;long at;Job(Handler h,Runnable r,long d){this.h=h;this.r=r;at=SystemClock.now+d;}}static List<Job> jobs=new ArrayList<>();public Handler(Looper l){}public void postDelayed(Runnable r,long d){jobs.add(new Job(this,r,d));}public void removeCallbacksAndMessages(Object token){jobs.removeIf(j->j.h==this);}public static void advance(long d){SystemClock.now+=d;for(Job j:new ArrayList<>(jobs))if(j.at<=SystemClock.now&&jobs.remove(j))j.r.run();}}"#),
        ("android/os/PowerManager.java", r#"package android.os;public class PowerManager{public static int PARTIAL_WAKE_LOCK=1;public WakeLock last;public WakeLock newWakeLock(int kind,String tag){return last=new WakeLock();}public static class WakeLock{public long until;public boolean released,referenceCounted=true;public void setReferenceCounted(boolean v){referenceCounted=v;}public void acquire(long ms){until=SystemClock.now+ms;}public boolean isHeld(){return !released&&SystemClock.now<until;}public void release(){released=true;}}}"#),
        ("android/net/Uri.java", "package android.net;public class Uri{public String value;public static Uri parse(String s){Uri u=new Uri();u.value=s;return u;}}"),
        ("android/content/pm/PackageManager.java", "package android.content.pm;public class PackageManager{public static final int PERMISSION_GRANTED=0;}"),
        ("android/content/pm/ServiceInfo.java", "package android.content.pm;public class ServiceInfo{public static final int FOREGROUND_SERVICE_TYPE_MICROPHONE=128,FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK=2;}"),
        ("android/content/Intent.java", r#"package android.content;import java.util.*;import android.net.Uri;public class Intent{public static final int FLAG_ACTIVITY_SINGLE_TOP=1;public String action;public Uri data;public Map<String,String> extras=new HashMap<>();public Intent(Context c,Class<?> cls){}public Intent setAction(String a){action=a;return this;}public String getAction(){return action;}public Intent setData(Uri u){data=u;return this;}public Intent putExtra(String k,String v){extras.put(k,v);return this;}public String getStringExtra(String k){return extras.get(k);}public Intent addFlags(int f){return this;}}"#),
        ("android/content/Context.java", r#"package android.content;import android.app.*;import android.os.*;public class Context{public static final String NOTIFICATION_SERVICE="notification",POWER_SERVICE="power";public static NotificationManager manager=new NotificationManager();public static PowerManager power=new PowerManager();public int permission=0;public Intent launched;public boolean launchFailure;public Object getSystemService(String key){return key.equals(POWER_SERVICE)?power:manager;}public int checkSelfPermission(String key){return permission;}public Object startForegroundService(Intent i){if(launchFailure)throw new SecurityException();launched=i;return null;}public boolean stopService(Intent i){return true;}}"#),
        ("android/app/NotificationChannel.java", "package android.app;public class NotificationChannel{public String id;public int importance;public NotificationChannel(String i,String n,int v){id=i;importance=v;}public int getImportance(){return importance;}}"),
        ("android/app/NotificationManager.java", r#"package android.app;import java.util.*;public class NotificationManager{public static int IMPORTANCE_LOW=2,IMPORTANCE_NONE=0;public boolean enabled=true,throwRead;public int updates;public Notification last;public Map<String,NotificationChannel> channels=new HashMap<>();public boolean areNotificationsEnabled(){if(throwRead)throw new SecurityException();return enabled;}public NotificationChannel getNotificationChannel(String id){return channels.get(id);}public void createNotificationChannel(NotificationChannel c){channels.putIfAbsent(c.id,c);}public void notify(int id,Notification n){updates++;last=n;}}"#),
        ("android/app/PendingIntent.java", "package android.app;import android.content.*;public class PendingIntent{public static final int FLAG_UPDATE_CURRENT=1,FLAG_IMMUTABLE=2;public Intent intent;public int flags;public static PendingIntent getService(Context c,int code,Intent i,int f){PendingIntent p=new PendingIntent();p.intent=i;p.flags=f;return p;}public static PendingIntent getActivity(Context c,int code,Intent i,int f){return getService(c,code,i,f);}}"),
        ("android/app/Notification.java", r#"package android.app;import java.util.*;public class Notification{public static String CATEGORY_SERVICE="service";public String text;public List<Action> actions=new ArrayList<>();public static class Action{public PendingIntent intent;public static class Builder{Action a=new Action();public Builder(Object icon,String title,PendingIntent p){a.intent=p;}public Action build(){return a;}}}public static class Builder{Notification n=new Notification();public Builder(android.content.Context c,String channel){}public Builder setSmallIcon(int v){return this;}public Builder setContentTitle(String s){return this;}public Builder setContentText(String s){n.text=s;return this;}public Builder setContentIntent(PendingIntent i){return this;}public Builder setCategory(String s){return this;}public Builder setOngoing(boolean v){return this;}public Builder setOnlyAlertOnce(boolean v){return this;}public Builder addAction(Action a){n.actions.add(a);return this;}public Notification build(){return n;}}}"#),
        ("android/app/Service.java", r#"package android.app;import android.content.*;import android.os.*;public class Service extends Context{public static final int START_NOT_STICKY=2,STOP_FOREGROUND_REMOVE=1;public boolean stopped,foreground,denyForeground,denyStop;public int types;public Notification notification;public void stopSelf(){stopped=true;}public void startForeground(int id,Notification n){if(denyForeground)throw new SecurityException();foreground=true;notification=n;}public void startForeground(int id,Notification n,int flags){types=flags;startForeground(id,n);}public void stopForeground(int v){foreground=false;if(denyStop)throw new IllegalStateException();}public IBinder onBind(Intent i){return null;}public int onStartCommand(Intent i,int flags,int id){return 0;}public void onTaskRemoved(Intent i){}public void onDestroy(){}}"#),
        ("org/json/JSONObject.java", r#"package org.json;import java.util.*;public class JSONObject{Map<String,Object> data=new HashMap<>();public JSONObject put(String k,Object v){data.put(k,v);return this;}public String getString(String k){return (String)data.get(k);}public long getLong(String k){return ((Number)data.get(k)).longValue();}public String optString(String k){Object v=data.get(k);return v instanceof String?(String)v:"";}public boolean optBoolean(String k){return Boolean.TRUE.equals(data.get(k));}public JSONObject optJSONObject(String k){Object v=data.get(k);return v instanceof JSONObject?(JSONObject)v:null;}}"#),
        ("com/local/client/Activity.java", r#"package com.local.client;import android.content.*;public class Activity extends Context{public boolean visible=true;public int ready,ended;public String last;boolean voiceForeground(){return visible;}void voiceSessionReady(String s){ready++;last=s;}void voiceSessionStopped(String s){ended++;last=s;}}"#),
    ]
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn create(root: &std::path::Path) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_else(|error| panic!("system clock is before the unix epoch: {error}"))
            .as_nanos();
        let parent = root.join("output");
        std::fs::create_dir_all(&parent)
            .unwrap_or_else(|error| panic!("could not create {}: {error}", parent.display()));
        for attempt in 0..64 {
            let path = parent.join(format!(
                "voice-session-{}-{nanos}-{attempt}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create {}: {error}", path.display()),
            }
        }
        panic!("could not reserve a unique voice-session scratch directory");
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}