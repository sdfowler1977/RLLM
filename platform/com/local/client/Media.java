package com.local.client;

import android.Manifest;
import android.app.AlertDialog;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.pdf.PdfRenderer;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;
import android.media.MediaPlayer;
import android.media.MediaRecorder;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import android.database.Cursor;
import android.util.Base64;
import android.widget.ImageView;
import android.widget.MediaController;
import android.widget.VideoView;
import java.io.*;
import java.util.concurrent.ExecutorService;
import org.json.*;

/** Platform capture/decoder bindings. Rust owns consent, routes and artifacts. */
final class Media {
    interface Callback { void result(String action, JSONObject input); }
    private final Activity activity;
    private final ExecutorService workers=new java.util.concurrent.ThreadPoolExecutor(2,2,0L,java.util.concurrent.TimeUnit.SECONDS,new java.util.concurrent.ArrayBlockingQueue<Runnable>(4));
    private final Callback callback;
    private final AudioRoute route;
    private String voiceSession="";
    private final Handler handler=new Handler(Looper.getMainLooper());
    private String ticket="";
    private MediaRecorder recorder;
    private File recording;
    private String capturePath="";
    private long captureLimit=2*1024*1024;
    private MediaPlayer player;
    private AudioFocusRequest focus;
    private AudioManager audio;
    private VideoView video;
    private SceneView sceneView;
    private AlertDialog preview;
    private boolean closed,paused=true;
    private String context="";
    private volatile android.media.AudioRecord microphone;
    private volatile int listenVersion;
    private String audioTicket="",audioSession="",audioPlayback="";
    private volatile boolean watching;
    private boolean communicationOwned;
    private volatile boolean interruptionReady;
    private int previousAudioMode;
    private boolean pendingListen;
    private long listenRequested;
    boolean permissionPending(){return pendingListen;}
    private JSONObject playbackOwner;
    private final Runnable playbackDeadline=()->{if(playbackOwner!=null){notice("Speech playback timed out; microphone paused");stopPlayback();}};
    private final ExecutorService microphoneWorker=new java.util.concurrent.ThreadPoolExecutor(1,1,0L,java.util.concurrent.TimeUnit.SECONDS,new java.util.concurrent.ArrayBlockingQueue<Runnable>(1));
    void context(String value){if(!context.equals(value)){context=value;playbackOwner=null;stopPlayback();}}
    void state(JSONObject value){
        voiceSession=value.optBoolean("voice_open")?value.optString("voice_consent"):"";
        JSONObject expected=value.optJSONObject("voice_audio_expected"),watch=value.optJSONObject("voice_watch_expected");String phase=value.optString("voice_status");
        boolean normal=expected!=null&&audioTicket.equals(expected.optString("ticket"))&&(phase.equals("listening")||phase.equals("microphone_pending"));
        boolean observed=watching&&watch!=null&&audioTicket.equals(watch.optString("ticket"))&&audioPlayback.equals(watch.optString("playback"))&&phase.equals("speaking");
        if(watching&&normal)watching=false;
        if(!audioTicket.isEmpty()&&(!value.optBoolean("voice_open")||!audioSession.equals(value.optString("voice_consent"))||!(normal||observed)))stopListening();
        if(playbackOwner!=null&&(!playbackOwner.optString("voice_session").equals(value.optString("voice_consent"))||!(phase.equals("playback_pending")||phase.equals("speaking")))){playbackOwner=null;stopPlayback();}
        retireRoute();
    }
    private void retireRoute(){if(voiceSession.isEmpty()&&audioTicket.isEmpty()&&player==null&&video==null&&recorder==null)route.stop();}
    private void audioRouteLost(){
        if(closed||paused)return;String session=voiceSession;boolean active=!session.isEmpty()||!audioTicket.isEmpty()||player!=null||video!=null||recorder!=null;if(!active)return;
        stopListening();stopPlayback();if(recorder!=null){String owner=ticket;stopRecording(false);ticket="";deliver("media_input_failure",owner,new JSONObject());}
        if(!session.isEmpty())try{voiceEvent("voice_route_lost",new JSONObject().put("voice_session",session));}catch(JSONException impossible){notice(impossible.getMessage());}
        retireRoute();notice("Audio interrupted; choose voice or playback again when ready");
    }
    private volatile int previewVersion,captureVersion;
    private volatile String loadingTicket="";
    private long captureStarted;
    private boolean pendingRecord;
    private final java.util.Set<android.os.CancellationSignal> signals=java.util.concurrent.ConcurrentHashMap.newKeySet();
    private final java.util.Set<ParcelFileDescriptor> openFiles=java.util.concurrent.ConcurrentHashMap.newKeySet();
    private final Runnable maximum=()->finishRecording();
    Media(Activity activity,ExecutorService workers,Callback callback){this.activity=activity;this.callback=callback;this.route=new AudioRoute(activity,()->audioRouteLost());}
    private void deliver(String action,String owner,JSONObject values){if(closed)return;try{values.put("media_ticket",owner);callback.result(action,values);}catch(JSONException e){notice(e.getMessage());}}
    private void queue(Runnable work){try{workers.execute(work);}catch(java.util.concurrent.RejectedExecutionException error){String owner=loadingTicket;loadingTicket="";if(!owner.isEmpty())deliver("media_input_failure",owner,new JSONObject());notice("Media workers are occupied; other app functions remain available");}}
    private void notice(String error){android.widget.Toast.makeText(activity,"Media: "+error,android.widget.Toast.LENGTH_LONG).show();}
    boolean handle(JSONObject e)throws Exception{
        switch(e.optString("kind")){
            case "voice_listen":
                stopListening();stopPlayback();audioTicket=e.getString("voice_audio_ticket");audioSession=e.getString("voice_session");listenRequested=android.os.SystemClock.elapsedRealtime();
                if(activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO)!=PackageManager.PERMISSION_GRANTED){pendingListen=true;activity.requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO},55);}else beginListening();return true;
            case "voice_watch":
                stopListening();audioTicket=e.getString("voice_audio_ticket");audioSession=e.getString("voice_session");audioPlayback=e.getString("voice_playback_ticket");watching=true;listenRequested=android.os.SystemClock.elapsedRealtime();
                if(paused||!activity.mediaCurrent(e)||activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO)!=PackageManager.PERMISSION_GRANTED){JSONObject owner=audioOwner();stopListening();voiceEvent("voice_watch_unavailable",owner);}else beginListening();return true;
            case "voice_interrupt_listen":
                if(e.optString("voice_audio_ticket").equals(audioTicket)&&e.optString("voice_session").equals(audioSession)){watching=false;stopPlayback();}return true;
            case "media_pick":
                stopRecording(false);captureVersion++;captureStarted=android.os.SystemClock.elapsedRealtime();ticket=e.getString("media_ticket");
                try{activity.startActivityForResult(new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").addCategory(Intent.CATEGORY_OPENABLE),53);}catch(Exception error){String owner=ticket;ticket="";deliver("media_input_failure",owner,new JSONObject().put("error",error.getMessage()));}return true;
            case "media_record":
                stopPlayback();stopRecording(false);capturePath=e.getString("capture_path");captureLimit=e.getLong("capture_limit");captureVersion++;captureStarted=android.os.SystemClock.elapsedRealtime();ticket=e.getString("media_ticket");
                if(activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO)!=PackageManager.PERMISSION_GRANTED){activity.requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO},54);}else beginRecording();return true;
            case "media_record_stop":if(e.optString("media_ticket").equals(ticket))finishRecording();return true;
            case "media_preview":if(paused){playbackEvent(e,"voice_playback_result",false);notice("File received; open the app to preview it");return true;}stopPlayback();final int version=previewVersion;queue(()->{try{Core.call(new JSONObject().put("op","media_verify").put("root",e.getString("root")).put("root_identity",e.getString("root_identity")).put("record",e.getJSONObject("record")));handler.post(()->{if(!closed&&!paused&&version==previewVersion&&activity.mediaCurrent(e))try{show(e);}catch(Exception error){stopPlayback();playbackEvent(e,"voice_playback_result",false);notice(error.getMessage());}});}catch(Exception error){handler.post(()->{if(!closed&&version==previewVersion){playbackEvent(e,"voice_playback_result",false);notice(error.getMessage());}});}});return true;
            case "media_playback_stop":stopPlayback();return true;
            default:return false;
        }
    }
    void permission(int request,int[] grants){if(request==55){if(!pendingListen||audioTicket.isEmpty())return;if(grants.length>0&&grants[0]==PackageManager.PERMISSION_GRANTED){pendingListen=true;if(!paused)beginListening();}else{JSONObject owner=audioOwner();stopListening();voiceEvent("voice_audio_failure",owner);}return;}if(request!=54)return;if(grants.length>0&&grants[0]==PackageManager.PERMISSION_GRANTED&&!ticket.isEmpty())beginRecording();else{String owner=ticket;ticket="";deliver("media_input_failure",owner,new JSONObject());}}
    private JSONObject audioOwner(){JSONObject out=new JSONObject();try{out.put("voice_session",audioSession).put("voice_audio_ticket",audioTicket).put("voice_playback_ticket",audioPlayback);}catch(JSONException impossible){throw new IllegalStateException(impossible);}return out;}
    private void voiceEvent(String action,JSONObject values){if(!closed)callback.result(action,values);}
    private void beginListening(){
        if(closed||audioTicket.isEmpty())return;
        if(android.os.SystemClock.elapsedRealtime()-listenRequested>60000){JSONObject owner=audioOwner();boolean watch=watching;stopListening();voiceEvent(watch?"voice_watch_unavailable":"voice_audio_failure",owner);return;}
        if(paused){if(watching){JSONObject owner=audioOwner();stopListening();voiceEvent("voice_watch_unavailable",owner);}else pendingListen=true;return;}
        pendingListen=false;final int version=listenVersion;final JSONObject owner=audioOwner();final boolean wasWatch=watching;
        if(!route.watch()){stopListening();voiceEvent(wasWatch?"voice_watch_unavailable":"voice_audio_failure",owner);return;}
        try{microphoneWorker.execute(()->{android.media.AudioRecord input=null;android.media.audiofx.AcousticEchoCanceler echo=null;JSONObject result=null;String action="voice_audio_failure";
            try{
                if(closed||version!=listenVersion)return;Core.call(new JSONObject(owner.toString()).put("op","voice_audio_check"));
                int minimum=android.media.AudioRecord.getMinBufferSize(16000,android.media.AudioFormat.CHANNEL_IN_MONO,android.media.AudioFormat.ENCODING_PCM_16BIT);if(minimum<=0||minimum>65536)throw new IOException("Microphone format unavailable");
                input=new android.media.AudioRecord(wasWatch?MediaRecorder.AudioSource.VOICE_COMMUNICATION:MediaRecorder.AudioSource.VOICE_RECOGNITION,16000,android.media.AudioFormat.CHANNEL_IN_MONO,android.media.AudioFormat.ENCODING_PCM_16BIT,Math.max(minimum,2560));
                if(input.getState()!=android.media.AudioRecord.STATE_INITIALIZED)throw new IOException("Microphone initialization failed");microphone=input;
                if(wasWatch){
                    if(!interruptionReady||audio==null||audio.getMode()!=AudioManager.MODE_IN_COMMUNICATION||!android.media.audiofx.AcousticEchoCanceler.isAvailable())throw new IOException("Echo control unavailable");
                    echo=android.media.audiofx.AcousticEchoCanceler.create(input.getAudioSessionId());if(echo==null)throw new IOException("Echo control unavailable");
                    if(!echo.getEnabled()&&(!echo.hasControl()||echo.setEnabled(true)!=android.media.audiofx.AudioEffect.SUCCESS))throw new IOException("Echo control could not be enabled");
                    if(!echo.getEnabled())throw new IOException("Echo control is inactive");
                }
                if(closed||version!=listenVersion)return;input.startRecording();if(input.getRecordingState()!=android.media.AudioRecord.RECORDSTATE_RECORDING)throw new IOException("Microphone did not start");
                handler.post(()->{if(!closed&&!paused&&version==listenVersion)try{voiceEvent(wasWatch?"voice_watch_start":"voice_audio_start",new JSONObject(owner.toString()).put("echo_active",wasWatch));}catch(JSONException failure){notice(failure.getMessage());}});
                byte[] frame=new byte[640];
                while(!closed&&version==listenVersion){
                    int n=input.read(frame,0,frame.length);if(closed||version!=listenVersion)return;if(n<=0||n%2!=0)throw new IOException("Microphone read failed");
                    if(watching&&(!interruptionReady||echo==null||!echo.getEnabled()||audio==null||audio.getMode()!=AudioManager.MODE_IN_COMMUNICATION))throw new IOException("Echo control stopped");
                    JSONObject response=(JSONObject)Core.call(new JSONObject(owner.toString()).put("op","voice_audio_feed").put("data",Base64.encodeToString(java.util.Arrays.copyOf(frame,n),Base64.NO_WRAP)));
                    if(response.has("onset_receipt")){final JSONObject onset=new JSONObject(owner.toString()).put("onset_receipt",response.getString("onset_receipt"));handler.post(()->{if(!closed&&!paused&&version==listenVersion)voiceEvent("voice_watch_onset",onset);});}
                    if(response.getBoolean("ready")){result=new JSONObject(owner.toString()).put("receipt",response.getString("receipt"));action="voice_audio_result";break;}
                }
            }catch(Exception error){result=owner;}
            finally{
                if(input!=null){try{input.stop();}catch(RuntimeException ignored){}try{input.release();}catch(RuntimeException error){result=owner;action="voice_audio_failure";}if(microphone==input)microphone=null;}
                if(echo!=null)try{echo.release();}catch(RuntimeException error){result=owner;action="voice_audio_failure";}
            }
            final JSONObject delivered=result;final String outcome=action;
            if(delivered!=null)handler.post(()->{if(!closed&&!paused&&version==listenVersion){String event=wasWatch&&watching&&outcome.equals("voice_audio_failure")?"voice_watch_unavailable":outcome;audioTicket="";audioSession="";audioPlayback="";watching=false;voiceEvent(event,delivered);}});
        });}catch(java.util.concurrent.RejectedExecutionException busy){JSONObject failureOwner=audioOwner();boolean watch=watching;stopListening();voiceEvent(watch?"voice_watch_unavailable":"voice_audio_failure",failureOwner);}
    }
    private void stopListening(){listenVersion++;pendingListen=false;watching=false;audioTicket="";audioSession="";audioPlayback="";android.media.AudioRecord input=microphone;if(input!=null)try{input.stop();}catch(RuntimeException ignored){}}
    private void playbackEvent(JSONObject owner,String action,boolean completed){if(owner==null||owner.optString("voice_playback_ticket").isEmpty()||closed)return;try{voiceEvent(action,new JSONObject().put("voice_session",owner.getString("voice_session")).put("voice_playback_ticket",owner.getString("voice_playback_ticket")).put("completed",completed));}catch(JSONException error){notice(error.getMessage());}}
    private void beginRecording(){
        if(closed||ticket.isEmpty())return;
        if(android.os.SystemClock.elapsedRealtime()-captureStarted>60000){String owner=ticket;ticket="";pendingRecord=false;deliver("media_input_failure",owner,new JSONObject());return;}
        if(paused){pendingRecord=true;return;}pendingRecord=false;
        try{
            if(!route.watch())throw new IOException("Audio route monitoring unavailable");
            recording=new File(capturePath);if(!recording.isFile()||recording.length()!=0||captureLimit<1||captureLimit>2*1024*1024)throw new IOException("Capture destination changed or exceeds its storage bound");
            recorder=new MediaRecorder();recorder.setAudioSource(MediaRecorder.AudioSource.MIC);recorder.setOutputFormat(MediaRecorder.OutputFormat.MPEG_4);recorder.setAudioEncoder(MediaRecorder.AudioEncoder.AAC);recorder.setAudioChannels(1);recorder.setAudioSamplingRate(16000);recorder.setAudioEncodingBitRate(64000);recorder.setMaxDuration(60000);recorder.setMaxFileSize(captureLimit);recorder.setOutputFile(recording.getAbsolutePath());
            recorder.setOnInfoListener((r,what,extra)->{if(what==MediaRecorder.MEDIA_RECORDER_INFO_MAX_DURATION_REACHED||what==MediaRecorder.MEDIA_RECORDER_INFO_MAX_FILESIZE_REACHED)finishRecording();});
            recorder.setOnErrorListener((r,what,extra)->{String owner=ticket;stopRecording(false);ticket="";deliver("media_input_failure",owner,new JSONObject());});
            recorder.prepare();recorder.start();handler.postDelayed(maximum,60000);
            android.widget.Toast.makeText(activity,"Recording audio. Choose Finish recording to stop.",android.widget.Toast.LENGTH_LONG).show();
        }catch(Exception error){String owner=ticket;stopRecording(false);ticket="";deliver("media_input_failure",owner,new JSONObject());notice(error.getMessage());}
    }
    private void finishRecording(){
        if(recorder==null)return;String owner=ticket;File file=recording;
        boolean valid=stopRecording(true);ticket="";
        if(!valid){deliver("media_input_failure",owner,new JSONObject());return;}
        final int version=captureVersion;loadingTicket=owner;
        queue(()->{try{byte[] bytes=read(new FileInputStream(file));handler.post(()->{if(version!=captureVersion||closed)return;loadingTicket="";try{deliver("media_input_result",owner,new JSONObject().put("name","recording.m4a").put("data",Base64.encodeToString(bytes,Base64.NO_WRAP)));}catch(JSONException error){notice(error.getMessage());}});}catch(Exception error){handler.post(()->{if(version==captureVersion){loadingTicket="";deliver("media_input_failure",owner,new JSONObject());}});}});
    }
    private boolean stopRecording(boolean finish){
        handler.removeCallbacks(maximum);boolean valid=false;
        if(recorder!=null){try{recorder.stop();valid=finish;}catch(RuntimeException ignored){valid=false;}finally{MediaRecorder old=recorder;recorder=null;try{old.reset();}catch(RuntimeException ignored){}finally{try{old.release();}catch(RuntimeException ignored){}}}}
        // Capture files remain local; Rust only adopts a successful owned result.
        recording=null;return valid;
    }
    boolean result(int request,int status,Intent data){
        if(request!=53)return false;String owner=ticket;ticket="";
        if(status!=android.app.Activity.RESULT_OK||data==null||data.getData()==null){deliver("media_input_cancel",owner,new JSONObject());return true;}
        Uri uri=data.getData();if(!"content".equals(uri.getScheme())||android.os.SystemClock.elapsedRealtime()-captureStarted>120000){deliver("media_input_failure",owner,new JSONObject());return true;}
        final int version=captureVersion;loadingTicket=owner;
        queue(()->{
            android.os.CancellationSignal signal=new android.os.CancellationSignal();signals.add(signal);final java.util.concurrent.atomic.AtomicReference<ParcelFileDescriptor> opened=new java.util.concurrent.atomic.AtomicReference<>();Runnable timeout=()->{signal.cancel();ParcelFileDescriptor fd=opened.get();if(fd!=null)try{fd.close();}catch(IOException ignored){}};handler.postDelayed(timeout,30000);
            try{
                if(version!=captureVersion)throw new IOException("Cancelled");activity.mediaSourceAccess(uri);String selected="file.bin";
                try(Cursor cursor=activity.getContentResolver().query(uri,new String[]{OpenableColumns.DISPLAY_NAME},null,null,null,signal)){if(cursor!=null&&cursor.moveToFirst())selected=cursor.getString(0);}
                final String name=selected==null?"file.bin":selected;final String receipt;
                JSONObject stream=(JSONObject)Core.call(new JSONObject().put("op","media_input_open").put("media_ticket",owner).put("name",name));String streamTicket=stream.getString("ticket");long limit=stream.getLong("limit"),count=0;
                try(ParcelFileDescriptor fd=activity.getContentResolver().openFileDescriptor(uri,"r",signal)){if(fd==null)throw new IOException("Input unavailable");opened.set(fd);openFiles.add(fd);try(InputStream input=new ParcelFileDescriptor.AutoCloseInputStream(fd)){byte[] buffer=new byte[8192];int n;while((n=input.read(buffer))!=-1){if(version!=captureVersion||closed||signal.isCanceled())throw new IOException("Cancelled");if(n==0)throw new IOException("Input made no progress");count+=n;if(count>limit)throw new IOException("Input exceeds available storage or 64 MiB");Core.call(new JSONObject().put("op","media_input_feed").put("media_ticket",owner).put("ticket",streamTicket).put("data",Base64.encodeToString(java.util.Arrays.copyOf(buffer,n),Base64.NO_WRAP)));}}finally{opened.compareAndSet(fd,null);openFiles.remove(fd);}}
                if(version!=captureVersion||closed||signal.isCanceled())throw new IOException("Cancelled");receipt=(String)Core.call(new JSONObject().put("op","media_input_finish").put("media_ticket",owner).put("ticket",streamTicket));
                handler.post(()->{try{if(version!=captureVersion||closed)return;loadingTicket="";activity.mediaSourceAccess(uri);deliver("media_input_result",owner,new JSONObject().put("media_receipt",receipt));}catch(Exception error){deliver("media_input_failure",owner,new JSONObject());notice(error.getMessage());}finally{cancelInput(owner);}});
            }catch(Exception error){cancelInput(owner);handler.post(()->{if(version==captureVersion){loadingTicket="";deliver("media_input_failure",owner,new JSONObject());}});}finally{handler.removeCallbacks(timeout);signals.remove(signal);}
        });return true;
    }
    private static void cancelInput(String owner){try{Core.call(new JSONObject().put("op","media_input_cancel").put("media_ticket",owner));}catch(Exception ignored){}}
    private static byte[] read(InputStream source)throws IOException{
        if(source==null)throw new IOException("Input unavailable");try(InputStream in=source;ByteArrayOutputStream out=new ByteArrayOutputStream()){byte[] chunk=new byte[8192];int count;while((count=in.read(chunk))!=-1){if(out.size()+count>2*1024*1024)throw new IOException("Input exceeds 2 MiB");out.write(chunk,0,count);}return out.toByteArray();}
    }
    private boolean communicationMode(){
        audio=(AudioManager)activity.getSystemService(android.content.Context.AUDIO_SERVICE);
        try{previousAudioMode=audio.getMode();if(previousAudioMode!=AudioManager.MODE_NORMAL)return false;audio.setMode(AudioManager.MODE_IN_COMMUNICATION);communicationOwned=audio.getMode()==AudioManager.MODE_IN_COMMUNICATION;return communicationOwned;}catch(RuntimeException unavailable){return false;}
    }
    private void restoreCommunicationMode(){if(communicationOwned&&audio!=null)try{if(audio.getMode()==AudioManager.MODE_IN_COMMUNICATION)audio.setMode(previousAudioMode);}catch(RuntimeException error){notice("Audio mode could not be restored");}finally{communicationOwned=false;}}
    private boolean audioFocus(boolean communication){
        audio=(AudioManager)activity.getSystemService(android.content.Context.AUDIO_SERVICE);
        final int focusVersion=previewVersion;focus=new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT).setAudioAttributes(new AudioAttributes.Builder().setUsage(communication?AudioAttributes.USAGE_VOICE_COMMUNICATION:AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build()).setWillPauseWhenDucked(true).setOnAudioFocusChangeListener(change->{if(focusVersion==previewVersion&&(change==AudioManager.AUDIOFOCUS_LOSS||change==AudioManager.AUDIOFOCUS_LOSS_TRANSIENT||change==AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK)){audioRouteLost();}},handler).build();
        return audio.requestAudioFocus(focus)==AudioManager.AUDIOFOCUS_REQUEST_GRANTED;
    }
    private void show(JSONObject e)throws Exception{
        stopPlayback();String path=e.getString("path"),kind=e.getString("media_kind");File file=new File(path);
        if(!file.isFile()||file.length()>256*1024*1024)throw new IOException("Media file unavailable or exceeds preview limit");
        if(kind.equals("audio")){
            if(!route.watch())throw new IOException("Audio route monitoring unavailable");
            boolean communication=e.optBoolean("voice_interruption")&&communicationMode();interruptionReady=communication;if(!audioFocus(communication)){interruptionReady=false;restoreCommunicationMode();throw new IOException("Audio focus unavailable");}
            playbackOwner=e.optString("voice_playback_ticket").isEmpty()?null:e;player=new MediaPlayer();player.setAudioAttributes(new AudioAttributes.Builder().setUsage(communication?AudioAttributes.USAGE_VOICE_COMMUNICATION:AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build());player.setDataSource(path);player.setOnPreparedListener(p->{if(player!=p)return;if(paused||closed||!activity.mediaCurrent(e)){stopPlayback();return;}try{p.start();handler.removeCallbacks(playbackDeadline);if(playbackOwner!=null)handler.postDelayed(playbackDeadline,600000);playbackEvent(e,"voice_playback_start",false);}catch(RuntimeException error){stopPlayback();notice(error.getMessage());}});player.setOnCompletionListener(p->{if(player!=p)return;JSONObject owner=playbackOwner;playbackOwner=null;stopPlayback();playbackEvent(owner,"voice_playback_result",true);});player.setOnErrorListener((p,what,extra)->{if(player!=p)return true;stopPlayback();notice("Audio decoder could not play this file");return true;});if(playbackOwner!=null)handler.postDelayed(playbackDeadline,30000);player.prepareAsync();
            AlertDialog.Builder controls=new AlertDialog.Builder(activity).setTitle("Audio playback").setMessage("Playing returned audio").setPositiveButton("Stop",(d,w)->{if(preview==d)stopPlayback();});if(playbackOwner!=null)controls.setNeutralButton("Speak now",(d,w)->{if(preview==d&&playbackOwner==e)playbackEvent(e,"voice_interrupt",false);});preview=controls.create();preview.setOnDismissListener(d->{if(preview==d)stopPlayback();});preview.show();
        }else if(kind.equals("video")){
            if(!route.watch())throw new IOException("Audio route monitoring unavailable");
            final VideoView shownVideo=new VideoView(activity);video=shownVideo;MediaController controls=new MediaController(activity);controls.setAnchorView(shownVideo);shownVideo.setMediaController(controls);shownVideo.setVideoPath(path);shownVideo.setOnPreparedListener(p->{if(video!=shownVideo)return;if(paused||closed||!activity.mediaCurrent(e)){stopPlayback();return;}shownVideo.start();});shownVideo.setOnErrorListener((p,what,extra)->{if(video!=shownVideo)return true;notice("Video decoder could not play this file");stopPlayback();return true;});preview=new AlertDialog.Builder(activity).setTitle("Video playback").setView(shownVideo).setPositiveButton("Close",(d,w)->{if(preview==d)stopPlayback();}).create();preview.setOnDismissListener(d->{if(preview==d)stopPlayback();});preview.show();
        }else if(kind.equals("image")){
            BitmapFactory.Options options=new BitmapFactory.Options();options.inJustDecodeBounds=true;BitmapFactory.decodeFile(path,options);int sample=1;while(Math.max(options.outWidth,options.outHeight)/sample>1600)sample*=2;options.inJustDecodeBounds=false;options.inSampleSize=sample;Bitmap bitmap=BitmapFactory.decodeFile(path,options);if(bitmap==null)throw new IOException("Image decoder could not open this file");ImageView image=new ImageView(activity);image.setAdjustViewBounds(true);image.setImageBitmap(bitmap);preview=new AlertDialog.Builder(activity).setTitle("Image").setView(image).setPositiveButton("Close",null).create();preview.setOnDismissListener(d->{image.setImageDrawable(null);bitmap.recycle();});preview.show();
        }else if(kind.equals("scene")){
            final SceneView shownScene=new SceneView(activity,e);sceneView=shownScene;android.widget.ScrollView sceneScroll=new android.widget.ScrollView(activity);sceneScroll.addView(shownScene);preview=new AlertDialog.Builder(activity).setTitle("3D geometry preview").setView(sceneScroll).setPositiveButton("Close",(d,w)->{if(preview==d)stopPlayback();}).create();preview.setOnDismissListener(d->{shownScene.close();if(sceneView==shownScene)sceneView=null;if(preview==d)preview=null;});preview.show();
        }else if(kind.equals("document")){
            ParcelFileDescriptor descriptor=ParcelFileDescriptor.open(file,ParcelFileDescriptor.MODE_READ_ONLY);PdfRenderer openedPdf;try{openedPdf=new PdfRenderer(descriptor);}catch(Exception error){descriptor.close();throw error;}final PdfRenderer pdf=openedPdf;ImageView image=new ImageView(activity);image.setAdjustViewBounds(true);final int[] page={0};final Bitmap[] shown={null};
            Runnable render=()->{try(PdfRenderer.Page p=pdf.openPage(page[0])){float scale=Math.min(1f,1600f/Math.max(p.getWidth(),p.getHeight()));Bitmap bitmap=Bitmap.createBitmap(Math.max(1,(int)(p.getWidth()*scale)),Math.max(1,(int)(p.getHeight()*scale)),Bitmap.Config.ARGB_8888);bitmap.eraseColor(android.graphics.Color.WHITE);p.render(bitmap,null,null,PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY);image.setImageBitmap(bitmap);if(shown[0]!=null)shown[0].recycle();shown[0]=bitmap;}catch(Exception error){notice(error.getMessage());}};
            if(pdf.getPageCount()==0){pdf.close();descriptor.close();throw new IOException("PDF has no pages");}render.run();preview=new AlertDialog.Builder(activity).setTitle("PDF pages").setView(image).setPositiveButton("Next",null).setNeutralButton("Previous",null).setNegativeButton("Close",null).create();preview.setOnDismissListener(d->{image.setImageDrawable(null);if(shown[0]!=null)shown[0].recycle();pdf.close();try{descriptor.close();}catch(IOException ignored){}});preview.show();preview.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(v->{if(page[0]+1<pdf.getPageCount()){page[0]++;render.run();}});preview.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener(v->{if(page[0]>0){page[0]--;render.run();}});
        }else throw new IOException("This artifact supports export; a decoder is not configured");
    }
    void stopPlayback(){if(watching)stopListening();interruptionReady=false;previewVersion++;handler.removeCallbacks(playbackDeadline);
        JSONObject owner=playbackOwner;playbackOwner=null;
        if(sceneView!=null){sceneView.close();sceneView=null;}
        if(player!=null){player.release();player=null;}if(video!=null){video.stopPlayback();video=null;}if(audio!=null&&focus!=null)audio.abandonAudioFocusRequest(focus);focus=null;restoreCommunicationMode();
        AlertDialog old=preview;preview=null;if(old!=null&&old.isShowing())old.dismiss();
        playbackEvent(owner,"voice_playback_result",false);retireRoute();
    }
    private void cancelLoading(){captureVersion++;for(android.os.CancellationSignal signal:signals)signal.cancel();for(ParcelFileDescriptor fd:openFiles)try{fd.close();}catch(IOException ignored){}String owner=loadingTicket;loadingTicket="";if(!owner.isEmpty())deliver("media_input_failure",owner,new JSONObject());}
    void cancel(){stopListening();pendingRecord=false;cancelLoading();String owner=ticket;stopRecording(false);ticket="";if(!owner.isEmpty())deliver("media_input_cancel",owner,new JSONObject());stopPlayback();}
    void resume(){paused=false;if(pendingListen&&!audioTicket.isEmpty()&&activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO)==PackageManager.PERMISSION_GRANTED)beginListening();if(pendingRecord&&!ticket.isEmpty()&&activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO)==PackageManager.PERMISSION_GRANTED)beginRecording();}
    void pause(){paused=true;route.stop();if(!pendingListen)stopListening();stopPlayback();if(!loadingTicket.isEmpty())cancelLoading();if(recorder!=null){String owner=ticket;stopRecording(false);ticket="";deliver("media_input_failure",owner,new JSONObject());}}
    void close(){closed=true;route.close();stopListening();microphoneWorker.shutdownNow();cancelLoading();workers.shutdownNow();stopRecording(false);stopPlayback();ticket="";handler.removeCallbacksAndMessages(null);}
}
