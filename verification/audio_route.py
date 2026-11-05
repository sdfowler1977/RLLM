"""Production receiver/focus bindings under controlled Android callback delivery."""
import json,os,pathlib,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1]
STUBS={
'android/content/Intent.java':'''package android.content;public class Intent{private final String action;public Intent(String a){action=a;}public String getAction(){return action;}}''',
'android/content/IntentFilter.java':'''package android.content;public class IntentFilter{public final String action;public IntentFilter(String a){action=a;}}''',
'android/content/BroadcastReceiver.java':'''package android.content;public abstract class BroadcastReceiver{public abstract void onReceive(Context c,Intent i);}''',
'android/content/Context.java':'''package android.content;public class Context{public static final int RECEIVER_EXPORTED=2;public static final String AUDIO_SERVICE="audio";public BroadcastReceiver receiver;public IntentFilter filter;public int flags,registrations,retirements;public boolean denied,unregisterFails;public Object service;public Intent registerReceiver(BroadcastReceiver r,IntentFilter f){return registerReceiver(r,f,0);}public Intent registerReceiver(BroadcastReceiver r,IntentFilter f,int v){if(denied)throw new SecurityException("controlled denial");receiver=r;filter=f;flags=v;registrations++;return null;}public void unregisterReceiver(BroadcastReceiver r){if(unregisterFails)throw new IllegalArgumentException("controlled retired receiver");if(receiver!=r)throw new AssertionError("wrong receiver retired");receiver=null;retirements++;}public Object getSystemService(String n){return service;}}''',
'android/os/Build.java':'''package android.os;public class Build{public static class VERSION{public static int SDK_INT=35;}}''',
'android/os/Handler.java':'''package android.os;public class Handler{}''',
'android/media/AudioAttributes.java':'''package android.media;public class AudioAttributes{public static final int USAGE_MEDIA=1,USAGE_VOICE_COMMUNICATION=2,CONTENT_TYPE_SPEECH=1;public int usage,type;public static class Builder{private final AudioAttributes a=new AudioAttributes();public Builder setUsage(int v){a.usage=v;return this;}public Builder setContentType(int v){a.type=v;return this;}public AudioAttributes build(){return a;}}}''',
'android/media/AudioFocusRequest.java':'''package android.media;import android.os.Handler;public class AudioFocusRequest{public AudioManager.OnAudioFocusChangeListener listener;public boolean pauseOnDuck;public AudioAttributes attributes;public Handler handler;public static class Builder{private final AudioFocusRequest r=new AudioFocusRequest();public Builder(int v){}public Builder setAudioAttributes(AudioAttributes a){r.attributes=a;return this;}public Builder setWillPauseWhenDucked(boolean v){r.pauseOnDuck=v;return this;}public Builder setOnAudioFocusChangeListener(AudioManager.OnAudioFocusChangeListener l,Handler h){r.listener=l;r.handler=h;return this;}public AudioFocusRequest build(){return r;}}}''',
'android/media/AudioManager.java':'''package android.media;public class AudioManager{public static final String ACTION_AUDIO_BECOMING_NOISY="android.media.AUDIO_BECOMING_NOISY";public static final int AUDIOFOCUS_GAIN_TRANSIENT=2,AUDIOFOCUS_REQUEST_GRANTED=1,AUDIOFOCUS_LOSS=-1,AUDIOFOCUS_LOSS_TRANSIENT=-2,AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK=-3,AUDIOFOCUS_GAIN=1;public int result=AUDIOFOCUS_REQUEST_GRANTED;public AudioFocusRequest request;public int requestAudioFocus(AudioFocusRequest r){request=r;return result;}public interface OnAudioFocusChangeListener{void onAudioFocusChange(int v);}}'''}
JAVA=r'''
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
'''
platform=PROJECT/'platform/com/local/client';media=(platform/'Media.java').read_text();start=media.index('    private boolean audioFocus(boolean communication){');end=media.index('    private void show(',start);source=JAVA.replace(' AUDIO_FOCUS_METHOD',media[start:end])
with tempfile.TemporaryDirectory(prefix='audio-route-',dir=PROJECT/'output')as temp:
    root=pathlib.Path(temp);files=[]
    for relative,body in STUBS.items():p=root/relative;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(body);files.append(str(p))
    p=root/'com/local/client/AudioRouteChecks.java';p.parent.mkdir(parents=True,exist_ok=True);p.write_text(source);files.extend([str(platform/'AudioRoute.java'),str(p)]);jdk=pathlib.Path(os.environ['CLIENT_JDK']);subprocess.run([str(jdk/'bin/javac'),'--release','8','-d',str(root),*files],check=True,capture_output=True,text=True);run=subprocess.run([str(jdk/'bin/java'),'-cp',str(root),'com.local.client.AudioRouteChecks'],check=True,capture_output=True,text=True,timeout=20);passed=run.stdout.splitlines();assert len(passed)==23,run.stdout
report=dict(passed=len(passed),checks=passed,scope='Owner execution of actual production AudioRoute and extracted unchanged Media audioFocus method against controlled Android callback/registration/focus adapters. Actual Android35SDK compilation is separate. No physical wired/Bluetooth/call/audio-focus/lifecycle acceptance is claimed.');(PROJECT/'output/checks').mkdir(parents=True,exist_ok=True);(PROJECT/'output/checks/audio_route.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
