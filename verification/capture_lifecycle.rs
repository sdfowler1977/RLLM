mod twin;

use twin::{n, obj, report_value, s, Value};

const SCOPE: &str = "Owner execution of exact production Media capture critical-section and preview-pause methods against controlled native-driver/UI state and deterministic start/stop interleaving. No physical microphone/driver/lifecycle acceptance is claimed.";

fn main() {
    let passed = run();
    let checks = Value::Arr(passed.iter().map(|name| s(name)).collect());
    let body = obj(vec![
        ("passed".into(), n(&passed.len().to_string())),
        ("checks".into(), checks),
        ("scope".into(), s(SCOPE)),
    ]);
    report_value("capture_lifecycle", &body, true);
    println!("{} capture_lifecycle checks passed", passed.len());
}

fn run() -> Vec<String> {
    let project = project_root();
    let media = std::fs::read_to_string(project.join("platform/com/local/client/Media.java"))
        .unwrap_or_else(|error| panic!("could not read Media.java: {error}"));
    let methods = extract_methods(&media);
    let scratch = twin::create_scratch(&project, "capture-lifecycle");
    let result = std::panic::catch_unwind(|| execute_java(&scratch, &methods));
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

fn execute_java(scratch: &std::path::Path, methods: &str) -> Vec<String> {
    let audio_record = r#"package android.media;import java.util.concurrent.*;public class AudioRecord{public static final int RECORDSTATE_RECORDING=3;public volatile boolean active,gated,failStop,noStart;public int starts,stops;public CountDownLatch entered=new CountDownLatch(1),release=new CountDownLatch(1);public void startRecording(){starts++;entered.countDown();if(gated)try{if(!release.await(2,TimeUnit.SECONDS))throw new RuntimeException("driver wait expired");}catch(InterruptedException e){throw new RuntimeException(e);}active=!noStart;}public int getRecordingState(){return active?3:0;}public void stop(){stops++;active=false;if(failStop)throw new IllegalStateException();}}"#;
    let source = java_source(methods);
    let audio_path = scratch.join("android/media/AudioRecord.java");
    let java_path = scratch.join("com/local/client/CaptureChecks.java");
    std::fs::create_dir_all(audio_path.parent().unwrap()).unwrap_or_else(|error| panic!("could not create audio source directory: {error}"));
    std::fs::create_dir_all(java_path.parent().unwrap()).unwrap_or_else(|error| panic!("could not create capture source directory: {error}"));
    std::fs::write(&audio_path, audio_record).unwrap_or_else(|error| panic!("could not write AudioRecord.java: {error}"));
    std::fs::write(&java_path, source).unwrap_or_else(|error| panic!("could not write CaptureChecks.java: {error}"));

    let jdk = std::env::var_os("CLIENT_JDK").unwrap_or_else(|| panic!("CLIENT_JDK is not set"));
    let javac = std::path::Path::new(&jdk).join("bin/javac");
    let java = std::path::Path::new(&jdk).join("bin/java");
    let compile = std::process::Command::new(javac)
        .arg("--release")
        .arg("8")
        .arg("-d")
        .arg(scratch)
        .arg(&audio_path)
        .arg(&java_path)
        .output()
        .unwrap_or_else(|error| panic!("could not start javac: {error}"));
    if !compile.status.success() {
        panic!("javac failed: {}", String::from_utf8_lossy(&compile.stderr));
    }
    let execution = {
        let mut child = std::process::Command::new(java)
            .arg("-cp")
            .arg(scratch)
            .arg("com.local.client.CaptureChecks")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("could not start java: {error}"));
        // The leg runs this subject with subprocess.run(timeout=15); an unbounded
        // wait would let a hung subject hang the twin instead of failing the check.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break child.wait_with_output().unwrap_or_else(|error| panic!("java wait: {error}")),
                Ok(None) => {
                    if std::time::Instant::now() > deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        panic!("java capture subject exceeded its 15 second bound");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(error) => panic!("java wait: {error}"),
            }
        }
    };
    if !execution.status.success() {
        panic!("capture checks failed: {}", String::from_utf8_lossy(&execution.stderr));
    }
    let stdout = String::from_utf8(execution.stdout).unwrap_or_else(|error| panic!("capture check output was not UTF-8: {error}"));
    let passed = stdout.lines().map(str::to_string).collect::<Vec<_>>();
    if passed.len() != 15 {
        panic!("expected 15 capture checks, got {}", passed.len());
    }
    passed
}

fn project_root() -> std::path::PathBuf {
    let manifest = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    if manifest.join("platform/com/local/client/Media.java").is_file() {
        return manifest;
    }
    manifest
        .parent()
        .unwrap_or_else(|| panic!("project root has no parent"))
        .to_path_buf()
}

fn extract_methods(media: &str) -> String {
    let capture = between(
        media,
        "    private boolean startCapture(",
        "    private void stopListening()",
    );
    let preview = between(
        media,
        "    private void hideVoicePreview()",
        "    private boolean startCapture(",
    );
    let voice = between(
        media,
        "    void stopVoice()",
        "    private boolean backgroundPlayback",
    );
    format!("{capture}{preview}{voice}")
}

fn between(text: &str, start: &str, end: &str) -> String {
    let start = text
        .find(start)
        .unwrap_or_else(|| panic!("Media.java does not contain {start:?}"));
    let end = text
        .get(start..)
        .and_then(|tail| tail.find(end))
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("Media.java does not contain {end:?} after {start:?}"));
    text[start..end].to_string()
}

fn java_source(methods: &str) -> String {
    format!(
        r#"package com.local.client;import java.io.*;import java.util.*;import java.util.concurrent.*;import java.util.concurrent.atomic.*;import android.media.AudioRecord;
public class CaptureChecks{{
 private boolean closed;private volatile int listenVersion;private volatile AudioRecord microphone;private final Object microphoneLock=new Object();private Object playbackOwner;private AlertDialog preview;private int previewStops,microphoneStops;private void stopListening(){{microphoneStops++;}}private void stopPlayback(){{previewStops++;preview=null;}}private static class AlertDialog{{Runnable listener;boolean dismissed;void setOnDismissListener(Runnable r){{listener=r;}}void dismiss(){{dismissed=true;if(listener!=null)listener.run();}}}}
 {methods}
 static List<String> passed=new ArrayList<>();static void check(String n,boolean v){{if(!v)throw new AssertionError(n);passed.add(n);}}
 public static void main(String[] args)throws Exception{{
  CaptureChecks c=new CaptureChecks();AudioRecord r=new AudioRecord();c.microphone=r;
  check("current capture reaches the actual native start binding",c.startCapture(r,0)&&r.active&&r.starts==1);
  c.stopCapture();check("stop returns after native capture is stopped and owner retired",!r.active&&c.listenVersion==1&&r.stops==1);
  check("retired capture cannot call native start",!c.startCapture(r,0)&&r.starts==1);
  c.closed=true;check("closed host cannot start a current-generation capture",!c.startCapture(r,1)&&r.starts==1);c.closed=false;
  AudioRecord missing=new AudioRecord();missing.noStart=true;boolean refused=false;try{{c.startCapture(missing,1);}}catch(IOException expected){{refused=true;}}check("native failure never reports recording success",refused&&!missing.active);
  AudioRecord racing=new AudioRecord();racing.gated=true;c.microphone=racing;AtomicReference<Throwable> failure=new AtomicReference<>();Thread starter=new Thread(()->{{try{{c.startCapture(racing,1);}}catch(Throwable e){{failure.set(e);}}}});starter.start();check("controlled driver starts the concurrent path",racing.entered.await(2,TimeUnit.SECONDS));
  CountDownLatch attempting=new CountDownLatch(1),stopped=new CountDownLatch(1);Thread stopper=new Thread(()->{{attempting.countDown();c.stopCapture();stopped.countDown();}});stopper.start();attempting.await();check("stop cannot return ahead of a native start in progress",!stopped.await(40,TimeUnit.MILLISECONDS));racing.release.countDown();starter.join(2000);stopper.join(2000);
  check("overlapping start and stop leave no running microphone",!starter.isAlive()&&!stopper.isAlive()&&failure.get()==null&&!racing.active&&c.listenVersion==2&&racing.stops==1);
  check("late callback cannot rearm the retired racing capture",!c.startCapture(racing,1)&&racing.starts==1);
  racing.failStop=true;c.stopCapture();check("already-retired driver still advances cancellation ownership",c.listenVersion==3&&!racing.active);
  CaptureChecks previews=new CaptureChecks();previews.preview=new AlertDialog();previews.hideVoicePreview();check("background pause retires every ordinary preview through its existing stop path",previews.previewStops==1&&previews.preview==null);previews.playbackOwner=new Object();previews.preview=new AlertDialog();AlertDialog voice=previews.preview;voice.listener=()->{{throw new AssertionError("speech was dismissed");}};previews.hideVoicePreview();check("owned speech hides its dialog without stopping playback",voice.dismissed&&previews.preview==null&&previews.previewStops==1);previews.hideVoicePreview();check("repeated background pause retains owned speech",previews.previewStops==1);
  CaptureChecks scoped=new CaptureChecks();scoped.preview=new AlertDialog();scoped.stopVoice();check("service stop preserves an ordinary preview",scoped.microphoneStops==1&&scoped.previewStops==0&&scoped.preview!=null);scoped.playbackOwner=new Object();scoped.stopVoice();check("service stop retires only its owned speech playback",scoped.microphoneStops==2&&scoped.previewStops==1);
  for(String name:passed)System.out.println(name);
 }}
}}"#
    )
}