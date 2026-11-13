//! Production receiver/focus bindings under controlled Android callback delivery.
mod twin;

use std::path::{Path, PathBuf};
use twin::{n, obj, report_value, s, Value};

const SCOPE: &str = "Owner execution of actual production AudioRoute and extracted unchanged Media audioFocus method against controlled Android callback/registration/focus adapters. Actual Android35SDK compilation is separate. No physical wired/Bluetooth/call/audio-focus/lifecycle acceptance is claimed.";

const JAVA: &str = r#"
package com.local.client;
import android.content.*;import android.media.*;import android.os.*;import java.util.*;
final class AudioRouteChecks{
 static final List<String> passed=new ArrayList<>();static void check(String n,boolean v){if(!v)throw new AssertionError(n);passed.add(n);}
 private Context activity=new Context();private AudioManager audio;private AudioFocusRequest focus;private Handler handler=new Handler();private int previewVersion=7,stops;
 private void audioRouteLost(){stops++;}
 AUDIO_FOCUS_METHOD
 public static void main(String[] args){
  Context context=new Context();int[] changes={0};AudioRoute route=new AudioRoute(context,()->changes[0]++);
  check("receiver registration is absent until an audio operation starts",context.registrations==0);
  Build.VERSION.SDK_INT=35;check("current Android registers the protected system route action",route.watch()&&context.flags==Context.RECEIVER_EXPORTED&&context.filter.action.equals(AudioManager.ACTION_AUDIO_BECOMING_NOISY));
  BroadcastReceiver first=context.receiver;check("repeated audio operations retain one live receiver",route.watch()&&context.registrations==1);
  first.onReceive(context,new Intent("unrelated.action"));first.onReceive(context,null);check("unrelated and absent route intents cannot stop audio",changes[0]==0);
  first.onReceive(context,new Intent(AudioManager.ACTION_AUDIO_BECOMING_NOISY));check("actual receiver callback delivers a current route interruption",changes[0]==1);
  route.stop();first.onReceive(context,new Intent(AudioManager.ACTION_AUDIO_BECOMING_NOISY));check("retired route callback remains inert",changes[0]==1&&context.retirements==1);
  check("new audio operation can register after interruption",route.watch()&&context.registrations==2);first.onReceive(context,new Intent(AudioManager.ACTION_AUDIO_BECOMING_NOISY));check("previous registration cannot interrupt a new audio owner",changes[0]==1);
  BroadcastReceiver second=context.receiver;second.onReceive(context,new Intent(AudioManager.ACTION_AUDIO_BECOMING_NOISY));check("replacement receiver still delivers current route loss",changes[0]==2);
  route.close();route.close();second.onReceive(context,new Intent(AudioManager.ACTION_AUDIO_BECOMING_NOISY));check("closed lifecycle unregisters once and cannot restart capture",context.retirements==2&&changes[0]==2&&!route.watch());
  Build.VERSION.SDK_INT=26;Context legacy=new Context();AudioRoute older=new AudioRoute(legacy,()->changes[0]++);check("Android8 uses its supported receiver overload",older.watch()&&legacy.flags==0);older.close();
  Context denied=new Context();denied.denied=true;AudioRoute refused=new AudioRoute(denied,()->changes[0]++);check("platform registration denial refuses only this optional audio binding",!refused.watch()&&denied.registrations==0);denied.denied=false;check("registration becomes available after a scoped denial",refused.watch());BroadcastReceiver retired=denied.receiver;denied.unregisterFails=true;refused.close();retired.onReceive(denied,new Intent(AudioManager.ACTION_AUDIO_BECOMING_NOISY));check("already-retired platform receiver cannot deliver after close",changes[0]==2);
  AudioRouteChecks test=new AudioRouteChecks();AudioManager manager=new AudioManager();test.activity.service=manager;
  check("actual focus factory accepts a granted media route",test.audioFocus(false));AudioFocusRequest current=manager.request;check("spoken content explicitly requests pause on duck with the main handler",current.pauseOnDuck&&current.handler==test.handler&&current.attributes.type==AudioAttributes.CONTENT_TYPE_SPEECH&&current.attributes.usage==AudioAttributes.USAGE_MEDIA);
  for(int event:new int[]{AudioManager.AUDIOFOCUS_LOSS,AudioManager.AUDIOFOCUS_LOSS_TRANSIENT,AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK}){int before=test.stops;current.listener.onAudioFocusChange(event);check("focus event interrupts current speech "+event,test.stops==before+1);}
  int before=test.stops;current.listener.onAudioFocusChange(AudioManager.AUDIOFOCUS_GAIN);check("regaining focus never rearms microphone or playback",test.stops==before);
  test.previewVersion++;current.listener.onAudioFocusChange(AudioManager.AUDIOFOCUS_LOSS);check("retired focus owner cannot stop a newer preview",test.stops==before);
  check("communication route retains matching speech attributes",test.audioFocus(true)&&manager.request.attributes.usage==AudioAttributes.USAGE_VOICE_COMMUNICATION&&manager.request.attributes.type==AudioAttributes.CONTENT_TYPE_SPEECH);
  manager.result=0;check("actual focus denial remains a scoped unsuccessful result",!test.audioFocus(false));
  for(String name:passed)System.out.println(name);
 }
}
"#;

fn main() {
    let passed = run();
    report_value(
        "audio_route",
        &obj(vec![
            ("passed".into(), n(&passed.len().to_string())),
            (
                "checks".into(),
                Value::Arr(passed.iter().map(|name| s(name)).collect()),
            ),
            ("scope".into(), s(SCOPE)),
        ]),
        true,
    );
    println!("{} audio_route checks passed", passed.len());
}

fn run() -> Vec<String> {
    let project = project_root();
    let media = std::fs::read_to_string(project.join("platform/com/local/client/Media.java"))
        .unwrap_or_else(|error| panic!("could not read Media.java: {error}"));
    let method = extract_audio_focus(&media).to_string();
    let scratch = twin::create_scratch(&project, "audio-route");
    let result = std::panic::catch_unwind(|| execute_java(&project, &scratch, &method));
    // The leg's TemporaryDirectory context manager lets a cleanup failure raise
    // after the body and replace an in-flight error, so the twin must not swallow it.
    let cleaned = std::fs::remove_dir_all(&scratch);
    match (result, cleaned) {
        (Ok(passed), Ok(())) => passed,
        (Ok(_), Err(error)) => panic!("could not remove {}: {error}", scratch.display()),
        (Err(payload), Ok(())) => std::panic::resume_unwind(payload),
        (Err(_), Err(error)) => panic!("could not remove {}: {error}", scratch.display()),
    }
}

fn execute_java(project: &Path, scratch: &Path, method: &str) -> Vec<String> {
    let source = JAVA.replace(" AUDIO_FOCUS_METHOD", method);
    if source.contains("AUDIO_FOCUS_METHOD") {
        panic!("audioFocus method was not inserted into AudioRouteChecks");
    }
    let checks = scratch.join("com/local/client/AudioRouteChecks.java");
    std::fs::create_dir_all(checks.parent().unwrap_or_else(|| panic!("checks path has no parent")))
        .unwrap_or_else(|error| panic!("could not create {}: {error}", checks.display()));
    std::fs::write(&checks, &source).unwrap_or_else(|error| panic!("could not write {}: {error}", checks.display()));

    let mut files = Vec::new();
    for (relative, body) in stubs() {
        let path = scratch.join(relative);
        std::fs::create_dir_all(path.parent().unwrap_or_else(|| panic!("stub has no parent")))
            .unwrap_or_else(|error| panic!("could not create {}: {error}", path.display()));
        std::fs::write(&path, body).unwrap_or_else(|error| panic!("could not write {}: {error}", path.display()));
        files.push(path);
    }
    files.push(project.join("platform/com/local/client/AudioRoute.java"));
    files.push(checks);

    let jdk = std::env::var_os("CLIENT_JDK").unwrap_or_else(|| panic!("CLIENT_JDK is not set"));
    let javac = Path::new(&jdk).join("bin/javac");
    let java = Path::new(&jdk).join("bin/java");
    let mut compile = std::process::Command::new(&javac);
    compile.arg("--release").arg("8").arg("-d").arg(scratch);
    for file in &files {
        compile.arg(file);
    }
    let compiled = compile
        .output()
        .unwrap_or_else(|error| panic!("could not start javac: {error}"));
    if !compiled.status.success() {
        panic!("javac failed: {}", String::from_utf8_lossy(&compiled.stderr));
    }

    // The leg runs this subject with subprocess.run(timeout=20). An unbounded
    // wait would let a hung subject hang the twin instead of failing the check.
    let execution = run_subject(&java, scratch);
    if !execution.status.success() {
        let stderr = String::from_utf8_lossy(&execution.stderr);
        if let Some(name) = assertion_name(&stderr) {
            panic!("{name}");
        }
        panic!(
            "audio route checks failed: {}\n{}",
            stderr,
            String::from_utf8_lossy(&execution.stdout)
        );
    }
    let stdout = String::from_utf8(execution.stdout)
        .unwrap_or_else(|error| panic!("audio route check output was not UTF-8: {error}"));
    let passed: Vec<String> = stdout.lines().map(str::to_string).collect();
    if passed.len() != 23 {
        panic!("expected 23 audio route checks, got {}:\n{}", passed.len(), passed.join("\n"));
    }
    passed
}

fn run_subject(java: &Path, scratch: &Path) -> std::process::Output {
    let mut child = std::process::Command::new(java)
        .arg("-cp")
        .arg(scratch)
        .arg("com.local.client.AudioRouteChecks")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("could not start java: {error}"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .unwrap_or_else(|error| panic!("java wait: {error}"))
            }
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("audio route subject exceeded its 20 second bound");
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => panic!("java wait: {error}"),
        }
    }
}

fn assertion_name(stderr: &str) -> Option<&str> {
    let rest = stderr.split("AssertionError: ").nth(1)?;
    let name = rest.lines().next()?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn project_root() -> PathBuf {
    let manifest = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    if manifest.join("platform/com/local/client/Media.java").is_file() {
        return manifest;
    }
    manifest
        .parent()
        .unwrap_or_else(|| panic!("project root has no parent"))
        .to_path_buf()
}

fn extract_audio_focus(media: &str) -> &str {
    let start_mark = "    private boolean audioFocus(boolean communication){";
    let end_mark = "    private void show(";
    let start = media
        .find(start_mark)
        .unwrap_or_else(|| panic!("Media.java does not contain the audioFocus method"));
    let end = media[start..]
        .find(end_mark)
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("Media.java does not contain show() after audioFocus"));
    &media[start..end]
}

fn stubs() -> [(&'static str, &'static str); 9] {
    [
        (r#"android/content/Intent.java"#, r#"package android.content;public class Intent{private final String action;public Intent(String a){action=a;}public String getAction(){return action;}}"#),
        (r#"android/content/IntentFilter.java"#, r#"package android.content;public class IntentFilter{public final String action;public IntentFilter(String a){action=a;}}"#),
        (r#"android/content/BroadcastReceiver.java"#, r#"package android.content;public abstract class BroadcastReceiver{public abstract void onReceive(Context c,Intent i);}"#),
        (r#"android/content/Context.java"#, r#"package android.content;public class Context{public static final int RECEIVER_EXPORTED=2;public static final String AUDIO_SERVICE="audio";public BroadcastReceiver receiver;public IntentFilter filter;public int flags,registrations,retirements;public boolean denied,unregisterFails;public Object service;public Intent registerReceiver(BroadcastReceiver r,IntentFilter f){return registerReceiver(r,f,0);}public Intent registerReceiver(BroadcastReceiver r,IntentFilter f,int v){if(denied)throw new SecurityException("controlled denial");receiver=r;filter=f;flags=v;registrations++;return null;}public void unregisterReceiver(BroadcastReceiver r){if(unregisterFails)throw new IllegalArgumentException("controlled retired receiver");if(receiver!=r)throw new AssertionError("wrong receiver retired");receiver=null;retirements++;}public Object getSystemService(String n){return service;}}"#),
        (r#"android/os/Build.java"#, r#"package android.os;public class Build{public static class VERSION{public static int SDK_INT=35;}}"#),
        (r#"android/os/Handler.java"#, r#"package android.os;public class Handler{}"#),
        (r#"android/media/AudioAttributes.java"#, r#"package android.media;public class AudioAttributes{public static final int USAGE_MEDIA=1,USAGE_VOICE_COMMUNICATION=2,CONTENT_TYPE_SPEECH=1;public int usage,type;public static class Builder{private final AudioAttributes a=new AudioAttributes();public Builder setUsage(int v){a.usage=v;return this;}public Builder setContentType(int v){a.type=v;return this;}public AudioAttributes build(){return a;}}}"#),
        (r#"android/media/AudioFocusRequest.java"#, r#"package android.media;import android.os.Handler;public class AudioFocusRequest{public AudioManager.OnAudioFocusChangeListener listener;public boolean pauseOnDuck;public AudioAttributes attributes;public Handler handler;public static class Builder{private final AudioFocusRequest r=new AudioFocusRequest();public Builder(int v){}public Builder setAudioAttributes(AudioAttributes a){r.attributes=a;return this;}public Builder setWillPauseWhenDucked(boolean v){r.pauseOnDuck=v;return this;}public Builder setOnAudioFocusChangeListener(AudioManager.OnAudioFocusChangeListener l,Handler h){r.listener=l;r.handler=h;return this;}public AudioFocusRequest build(){return r;}}}"#),
        (r#"android/media/AudioManager.java"#, r#"package android.media;public class AudioManager{public static final String ACTION_AUDIO_BECOMING_NOISY="android.media.AUDIO_BECOMING_NOISY";public static final int AUDIOFOCUS_GAIN_TRANSIENT=2,AUDIOFOCUS_REQUEST_GRANTED=1,AUDIOFOCUS_LOSS=-1,AUDIOFOCUS_LOSS_TRANSIENT=-2,AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK=-3,AUDIOFOCUS_GAIN=1;public int result=AUDIOFOCUS_REQUEST_GRANTED;public AudioFocusRequest request;public int requestAudioFocus(AudioFocusRequest r){request=r;return result;}public interface OnAudioFocusChangeListener{void onAudioFocusChange(int v);}}"#)
    ]
}
