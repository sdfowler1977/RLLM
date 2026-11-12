"""Execute the production capture start/stop critical section with driver interleaving."""
import json,os,pathlib,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1]
media=(PROJECT/'platform/com/local/client/Media.java').read_text()
start=media.index('    private boolean startCapture(');end=media.index('    private void stopListening()',start);methods=media[start:end]
start=media.index('    private void hideVoicePreview()');end=media.index('    private boolean startCapture(',start);methods+=media[start:end]
start=media.index('    void stopVoice()');end=media.index('    private boolean backgroundPlayback',start);methods+=media[start:end]
RECORD='''package android.media;import java.util.concurrent.*;public class AudioRecord{public static final int RECORDSTATE_RECORDING=3;public volatile boolean active,gated,failStop,noStart;public int starts,stops;public CountDownLatch entered=new CountDownLatch(1),release=new CountDownLatch(1);public void startRecording(){starts++;entered.countDown();if(gated)try{if(!release.await(2,TimeUnit.SECONDS))throw new RuntimeException("driver wait expired");}catch(InterruptedException e){throw new RuntimeException(e);}active=!noStart;}public int getRecordingState(){return active?3:0;}public void stop(){stops++;active=false;if(failStop)throw new IllegalStateException();}}'''
JAVA=r'''
package com.local.client;import java.io.*;import java.util.*;import java.util.concurrent.*;import java.util.concurrent.atomic.*;import android.media.AudioRecord;
public class CaptureChecks{
 private boolean closed;private volatile int listenVersion;private volatile AudioRecord microphone;private final Object microphoneLock=new Object();private Object playbackOwner;private AlertDialog preview;private int previewStops,microphoneStops;private void stopListening(){microphoneStops++;}private void stopPlayback(){previewStops++;preview=null;}private static class AlertDialog{Runnable listener;boolean dismissed;void setOnDismissListener(Runnable r){listener=r;}void dismiss(){dismissed=true;if(listener!=null)listener.run();}}
 METHODS
 static List<String> passed=new ArrayList<>();static void check(String n,boolean v){if(!v)throw new AssertionError(n);passed.add(n);}
 public static void main(String[] args)throws Exception{
  CaptureChecks c=new CaptureChecks();AudioRecord r=new AudioRecord();c.microphone=r;
  check("current capture reaches the actual native start binding",c.startCapture(r,0)&&r.active&&r.starts==1);
  c.stopCapture();check("stop returns after native capture is stopped and owner retired",!r.active&&c.listenVersion==1&&r.stops==1);
  check("retired capture cannot call native start",!c.startCapture(r,0)&&r.starts==1);
  c.closed=true;check("closed host cannot start a current-generation capture",!c.startCapture(r,1)&&r.starts==1);c.closed=false;
  AudioRecord missing=new AudioRecord();missing.noStart=true;boolean refused=false;try{c.startCapture(missing,1);}catch(IOException expected){refused=true;}check("native failure never reports recording success",refused&&!missing.active);
  AudioRecord racing=new AudioRecord();racing.gated=true;c.microphone=racing;AtomicReference<Throwable> failure=new AtomicReference<>();Thread starter=new Thread(()->{try{c.startCapture(racing,1);}catch(Throwable e){failure.set(e);}});starter.start();check("controlled driver starts the concurrent path",racing.entered.await(2,TimeUnit.SECONDS));
  CountDownLatch attempting=new CountDownLatch(1),stopped=new CountDownLatch(1);Thread stopper=new Thread(()->{attempting.countDown();c.stopCapture();stopped.countDown();});stopper.start();attempting.await();check("stop cannot return ahead of a native start in progress",!stopped.await(40,TimeUnit.MILLISECONDS));racing.release.countDown();starter.join(2000);stopper.join(2000);
  check("overlapping start and stop leave no running microphone",!starter.isAlive()&&!stopper.isAlive()&&failure.get()==null&&!racing.active&&c.listenVersion==2&&racing.stops==1);
  check("late callback cannot rearm the retired racing capture",!c.startCapture(racing,1)&&racing.starts==1);
  racing.failStop=true;c.stopCapture();check("already-retired driver still advances cancellation ownership",c.listenVersion==3&&!racing.active);
  CaptureChecks previews=new CaptureChecks();previews.preview=new AlertDialog();previews.hideVoicePreview();check("background pause retires every ordinary preview through its existing stop path",previews.previewStops==1&&previews.preview==null);previews.playbackOwner=new Object();previews.preview=new AlertDialog();AlertDialog voice=previews.preview;voice.listener=()->{throw new AssertionError("speech was dismissed");};previews.hideVoicePreview();check("owned speech hides its dialog without stopping playback",voice.dismissed&&previews.preview==null&&previews.previewStops==1);previews.hideVoicePreview();check("repeated background pause retains owned speech",previews.previewStops==1);
  CaptureChecks scoped=new CaptureChecks();scoped.preview=new AlertDialog();scoped.stopVoice();check("service stop preserves an ordinary preview",scoped.microphoneStops==1&&scoped.previewStops==0&&scoped.preview!=null);scoped.playbackOwner=new Object();scoped.stopVoice();check("service stop retires only its owned speech playback",scoped.microphoneStops==2&&scoped.previewStops==1);
  for(String name:passed)System.out.println(name);
 }
}
'''.replace(' METHODS',methods)
with tempfile.TemporaryDirectory(prefix='capture-lifecycle-',dir=PROJECT/'output')as directory:
    root=pathlib.Path(directory);a=root/'android/media/AudioRecord.java';b=root/'com/local/client/CaptureChecks.java'
    for p,s in [(a,RECORD),(b,JAVA)]:p.parent.mkdir(parents=True,exist_ok=True);p.write_text(s)
    jdk=pathlib.Path(os.environ['CLIENT_JDK']);compiled=subprocess.run([str(jdk/'bin/javac'),'--release','8','-d',str(root),str(a),str(b)],capture_output=True,text=True)
    if compiled.returncode:raise RuntimeError(compiled.stderr)
    run=subprocess.run([str(jdk/'bin/java'),'-cp',str(root),'com.local.client.CaptureChecks'],capture_output=True,text=True,timeout=15)
    if run.returncode:raise RuntimeError(run.stderr)
    checks=run.stdout.splitlines();assert len(checks)==15
record=dict(passed=len(checks),checks=checks,scope='Owner execution of exact production Media capture critical-section and preview-pause methods against controlled native-driver/UI state and deterministic start/stop interleaving. No physical microphone/driver/lifecycle acceptance is claimed.')
(PROJECT/'output/checks/capture_lifecycle.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record,indent=2))
