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
    private AlertDialog preview;
    private boolean closed,paused=true;
    private String context="";
    void context(String value){if(!context.equals(value)){context=value;stopPlayback();}}
    private volatile int previewVersion,captureVersion;
    private volatile String loadingTicket="";
    private long captureStarted;
    private boolean pendingRecord;
    private final java.util.Set<android.os.CancellationSignal> signals=java.util.concurrent.ConcurrentHashMap.newKeySet();
    private final java.util.Set<ParcelFileDescriptor> openFiles=java.util.concurrent.ConcurrentHashMap.newKeySet();
    private final Runnable maximum=()->finishRecording();
    Media(Activity activity,ExecutorService workers,Callback callback){this.activity=activity;this.callback=callback;}
    private void deliver(String action,String owner,JSONObject values){if(closed)return;try{values.put("media_ticket",owner);callback.result(action,values);}catch(JSONException e){notice(e.getMessage());}}
    private void queue(Runnable work){try{workers.execute(work);}catch(java.util.concurrent.RejectedExecutionException error){String owner=loadingTicket;loadingTicket="";if(!owner.isEmpty())deliver("media_input_failure",owner,new JSONObject());notice("Media workers are occupied; other app functions remain available");}}
    private void notice(String error){android.widget.Toast.makeText(activity,"Media: "+error,android.widget.Toast.LENGTH_LONG).show();}
    boolean handle(JSONObject e)throws Exception{
        switch(e.optString("kind")){
            case "media_pick":
                stopRecording(false);captureVersion++;captureStarted=android.os.SystemClock.elapsedRealtime();ticket=e.getString("media_ticket");
                try{activity.startActivityForResult(new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").addCategory(Intent.CATEGORY_OPENABLE),53);}catch(Exception error){String owner=ticket;ticket="";deliver("media_input_failure",owner,new JSONObject().put("error",error.getMessage()));}return true;
            case "media_record":
                stopPlayback();stopRecording(false);capturePath=e.getString("capture_path");captureLimit=e.getLong("capture_limit");captureVersion++;captureStarted=android.os.SystemClock.elapsedRealtime();ticket=e.getString("media_ticket");
                if(activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO)!=PackageManager.PERMISSION_GRANTED){activity.requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO},54);}else beginRecording();return true;
            case "media_record_stop":if(e.optString("media_ticket").equals(ticket))finishRecording();return true;
            case "media_preview":if(paused){notice("File received; open the app to preview it");return true;}stopPlayback();final int version=previewVersion;queue(()->{try{Core.call(new JSONObject().put("op","media_verify").put("root",e.getString("root")).put("root_identity",e.getString("root_identity")).put("record",e.getJSONObject("record")));handler.post(()->{if(!closed&&!paused&&version==previewVersion&&activity.mediaCurrent(e))try{show(e);}catch(Exception error){notice(error.getMessage());}});}catch(Exception error){handler.post(()->{if(!closed&&version==previewVersion)notice(error.getMessage());});}});return true;
            case "media_playback_stop":stopPlayback();return true;
            default:return false;
        }
    }
    void permission(int request,int[] grants){if(request!=54)return;if(grants.length>0&&grants[0]==PackageManager.PERMISSION_GRANTED&&!ticket.isEmpty())beginRecording();else{String owner=ticket;ticket="";deliver("media_input_failure",owner,new JSONObject());}}
    private void beginRecording(){
        if(closed||ticket.isEmpty())return;
        if(android.os.SystemClock.elapsedRealtime()-captureStarted>60000){String owner=ticket;ticket="";pendingRecord=false;deliver("media_input_failure",owner,new JSONObject());return;}
        if(paused){pendingRecord=true;return;}pendingRecord=false;
        try{
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
                final String name=selected==null?"file.bin":selected;final byte[] bytes;
                try(ParcelFileDescriptor fd=activity.getContentResolver().openFileDescriptor(uri,"r",signal)){if(fd==null)throw new IOException("Input unavailable");opened.set(fd);openFiles.add(fd);try{bytes=read(new ParcelFileDescriptor.AutoCloseInputStream(fd));}finally{opened.compareAndSet(fd,null);openFiles.remove(fd);}}
                handler.post(()->{if(version!=captureVersion||closed)return;loadingTicket="";try{activity.mediaSourceAccess(uri);deliver("media_input_result",owner,new JSONObject().put("name",name).put("data",Base64.encodeToString(bytes,Base64.NO_WRAP)));}catch(Exception error){deliver("media_input_failure",owner,new JSONObject());notice(error.getMessage());}});
            }catch(Exception error){handler.post(()->{if(version==captureVersion){loadingTicket="";deliver("media_input_failure",owner,new JSONObject());}});}finally{handler.removeCallbacks(timeout);signals.remove(signal);}
        });return true;
    }
    private static byte[] read(InputStream source)throws IOException{
        if(source==null)throw new IOException("Input unavailable");try(InputStream in=source;ByteArrayOutputStream out=new ByteArrayOutputStream()){byte[] chunk=new byte[8192];int count;while((count=in.read(chunk))!=-1){if(out.size()+count>2*1024*1024)throw new IOException("Input exceeds 2 MiB");out.write(chunk,0,count);}return out.toByteArray();}
    }
    private boolean audioFocus(){
        audio=(AudioManager)activity.getSystemService(android.content.Context.AUDIO_SERVICE);
        focus=new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT).setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build()).setOnAudioFocusChangeListener(change->{if(change==AudioManager.AUDIOFOCUS_LOSS||change==AudioManager.AUDIOFOCUS_LOSS_TRANSIENT){stopPlayback();}}).build();
        return audio.requestAudioFocus(focus)==AudioManager.AUDIOFOCUS_REQUEST_GRANTED;
    }
    private void show(JSONObject e)throws Exception{
        stopPlayback();String path=e.getString("path"),kind=e.getString("media_kind");File file=new File(path);
        if(!file.isFile()||file.length()>256*1024*1024)throw new IOException("Media file unavailable or exceeds preview limit");
        if(kind.equals("audio")){
            if(!audioFocus())throw new IOException("Audio focus unavailable");
            player=new MediaPlayer();player.setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build());player.setDataSource(path);player.setOnPreparedListener(p->p.start());player.setOnCompletionListener(p->stopPlayback());player.setOnErrorListener((p,what,extra)->{stopPlayback();notice("Audio decoder could not play this file");return true;});player.prepareAsync();
            preview=new AlertDialog.Builder(activity).setTitle("Audio playback").setMessage("Playing returned audio").setPositiveButton("Stop",(d,w)->stopPlayback()).create();preview.setOnDismissListener(d->stopPlayback());preview.show();
        }else if(kind.equals("video")){
            video=new VideoView(activity);MediaController controls=new MediaController(activity);controls.setAnchorView(video);video.setMediaController(controls);video.setVideoPath(path);video.setOnPreparedListener(p->video.start());video.setOnErrorListener((p,what,extra)->{notice("Video decoder could not play this file");stopPlayback();return true;});preview=new AlertDialog.Builder(activity).setTitle("Video playback").setView(video).setPositiveButton("Close",(d,w)->stopPlayback()).create();preview.setOnDismissListener(d->stopPlayback());preview.show();
        }else if(kind.equals("image")){
            BitmapFactory.Options options=new BitmapFactory.Options();options.inJustDecodeBounds=true;BitmapFactory.decodeFile(path,options);int sample=1;while(Math.max(options.outWidth,options.outHeight)/sample>1600)sample*=2;options.inJustDecodeBounds=false;options.inSampleSize=sample;Bitmap bitmap=BitmapFactory.decodeFile(path,options);if(bitmap==null)throw new IOException("Image decoder could not open this file");ImageView image=new ImageView(activity);image.setAdjustViewBounds(true);image.setImageBitmap(bitmap);preview=new AlertDialog.Builder(activity).setTitle("Image").setView(image).setPositiveButton("Close",null).create();preview.setOnDismissListener(d->{image.setImageDrawable(null);bitmap.recycle();});preview.show();
        }else if(kind.equals("document")){
            ParcelFileDescriptor descriptor=ParcelFileDescriptor.open(file,ParcelFileDescriptor.MODE_READ_ONLY);PdfRenderer openedPdf;try{openedPdf=new PdfRenderer(descriptor);}catch(Exception error){descriptor.close();throw error;}final PdfRenderer pdf=openedPdf;ImageView image=new ImageView(activity);image.setAdjustViewBounds(true);final int[] page={0};final Bitmap[] shown={null};
            Runnable render=()->{try(PdfRenderer.Page p=pdf.openPage(page[0])){float scale=Math.min(1f,1600f/Math.max(p.getWidth(),p.getHeight()));Bitmap bitmap=Bitmap.createBitmap(Math.max(1,(int)(p.getWidth()*scale)),Math.max(1,(int)(p.getHeight()*scale)),Bitmap.Config.ARGB_8888);bitmap.eraseColor(android.graphics.Color.WHITE);p.render(bitmap,null,null,PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY);image.setImageBitmap(bitmap);if(shown[0]!=null)shown[0].recycle();shown[0]=bitmap;}catch(Exception error){notice(error.getMessage());}};
            if(pdf.getPageCount()==0){pdf.close();descriptor.close();throw new IOException("PDF has no pages");}render.run();preview=new AlertDialog.Builder(activity).setTitle("PDF pages").setView(image).setPositiveButton("Next",null).setNeutralButton("Previous",null).setNegativeButton("Close",null).create();preview.setOnDismissListener(d->{image.setImageDrawable(null);if(shown[0]!=null)shown[0].recycle();pdf.close();try{descriptor.close();}catch(IOException ignored){}});preview.show();preview.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(v->{if(page[0]+1<pdf.getPageCount()){page[0]++;render.run();}});preview.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener(v->{if(page[0]>0){page[0]--;render.run();}});
        }else throw new IOException("This artifact supports export; a decoder is not configured");
    }
    void stopPlayback(){previewVersion++;
        if(player!=null){player.release();player=null;}if(video!=null){video.stopPlayback();video=null;}if(audio!=null&&focus!=null)audio.abandonAudioFocusRequest(focus);focus=null;
        AlertDialog old=preview;preview=null;if(old!=null&&old.isShowing())old.dismiss();
    }
    private void cancelLoading(){captureVersion++;for(android.os.CancellationSignal signal:signals)signal.cancel();for(ParcelFileDescriptor fd:openFiles)try{fd.close();}catch(IOException ignored){}String owner=loadingTicket;loadingTicket="";if(!owner.isEmpty())deliver("media_input_failure",owner,new JSONObject());}
    void cancel(){pendingRecord=false;cancelLoading();String owner=ticket;stopRecording(false);ticket="";if(!owner.isEmpty())deliver("media_input_cancel",owner,new JSONObject());stopPlayback();}
    void resume(){paused=false;if(pendingRecord&&!ticket.isEmpty()&&activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO)==PackageManager.PERMISSION_GRANTED)beginRecording();}
    void pause(){paused=true;stopPlayback();if(!loadingTicket.isEmpty())cancelLoading();if(recorder!=null){String owner=ticket;stopRecording(false);ticket="";deliver("media_input_failure",owner,new JSONObject());}}
    void close(){closed=true;cancelLoading();workers.shutdownNow();stopRecording(false);stopPlayback();ticket="";handler.removeCallbacksAndMessages(null);}
}
