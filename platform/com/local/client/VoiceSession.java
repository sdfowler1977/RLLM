package com.local.client;

import android.app.*;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ServiceInfo;
import android.os.*;
import java.lang.ref.WeakReference;
import org.json.JSONObject;

/** Platform lifetime and notification for a reviewed, process-owned voice session. */
public final class VoiceSession extends Service {
    private static WeakReference<Activity> owner=new WeakReference<>(null);
    private static VoiceSession current;
    private static String requested="";
    private static long deadline;
    private final Handler handler=new Handler(Looper.getMainLooper());
    private String session="";
    private boolean active;
    private String lastStatus="";
    private PowerManager.WakeLock wake;
    private final Runnable expiry=()->stopSelf();
    private final Runnable health=new Runnable(){public void run(){
        try{if(!active||owner.get()==null||SystemClock.elapsedRealtime()>=deadline||!notificationsVisible(VoiceSession.this)
                ||checkSelfPermission("android.permission.RECORD_AUDIO")!=PackageManager.PERMISSION_GRANTED){stopSelf();return;}
            handler.postDelayed(this,Math.min(2000,Math.max(1,deadline-SystemClock.elapsedRealtime())));
        }catch(RuntimeException unavailable){stopSelf();}
    }};

    static boolean owns(Activity activity,String value){return current!=null&&current.active&&owner.get()==activity&&sessionMatches(value)&&SystemClock.elapsedRealtime()<deadline;}
    private static boolean sessionMatches(String value){return value!=null&&!value.isEmpty()&&value.equals(requested);}
    private static boolean notificationsVisible(android.content.Context context){
        NotificationManager manager=(NotificationManager)context.getSystemService(NOTIFICATION_SERVICE);
        NotificationChannel channel=manager.getNotificationChannel("voice");
        return manager.areNotificationsEnabled()&&(channel==null||channel.getImportance()!=NotificationManager.IMPORTANCE_NONE);
    }
    static boolean pending(Activity activity,String value){return owner.get()==activity&&sessionMatches(value)&&SystemClock.elapsedRealtime()<deadline;}
    static void start(Activity activity,JSONObject effect)throws Exception{
        String value=effect.getString("voice_session");
        if(owns(activity,value)){activity.voiceSessionReady(value);return;}
        if(pending(activity,value))return;
        if(!activity.voiceForeground())throw new java.io.IOException("Open the app before starting background voice");
        if(activity.checkSelfPermission("android.permission.RECORD_AUDIO")!=PackageManager.PERMISSION_GRANTED)throw new java.io.IOException("Microphone permission is required for background voice");
        NotificationManager notifications=(NotificationManager)activity.getSystemService(NOTIFICATION_SERVICE);
        if(!notificationsVisible(activity)||(Build.VERSION.SDK_INT>=33&&activity.checkSelfPermission("android.permission.POST_NOTIFICATIONS")!=PackageManager.PERMISSION_GRANTED))throw new java.io.IOException("App and voice notifications are required for background voice; ordinary foreground voice remains available");
        long lease=effect.getLong("voice_lease_ms");
        if(value.isEmpty()||lease<=0||lease>600000)throw new java.io.IOException("Voice session expired");
        if(current!=null)throw new java.io.IOException("Finish the current background voice session first");
        owner=new WeakReference<>(activity);requested=value;deadline=SystemClock.elapsedRealtime()+lease;
        try{activity.startForegroundService(new Intent(activity,VoiceSession.class).putExtra("session",value));}
        catch(RuntimeException failure){owner.clear();requested="";deadline=0;throw failure;}
    }
    static void release(Activity activity){if(owner.get()!=activity)return;VoiceSession service=current;owner.clear();requested="";deadline=0;if(service!=null)service.stopSelf();else activity.stopService(new Intent(activity,VoiceSession.class));}
    static void reconcile(Activity activity,JSONObject state,String status){
        if(owner.get()!=activity)return;
        String value=state.optString("voice_consent");JSONObject settings=state.optJSONObject("voice_settings");
        if(!state.optBoolean("voice_open")||settings==null||!settings.optBoolean("background")||!sessionMatches(value)||SystemClock.elapsedRealtime()>=deadline){release(activity);return;}
        if(current!=null&&current.active)current.notification(status);
    }
    private Notification notice(String status){
        NotificationManager manager=(NotificationManager)getSystemService(NOTIFICATION_SERVICE);
        manager.createNotificationChannel(new NotificationChannel("voice","Voice conversation",NotificationManager.IMPORTANCE_LOW));
        Intent stop=new Intent(this,VoiceSession.class).setAction("stop").setData(android.net.Uri.parse("rllm-voice:/"+session)).putExtra("session",session);
        PendingIntent action=PendingIntent.getService(this,2,stop,PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE);
        PendingIntent open=PendingIntent.getActivity(this,2,new Intent(this,Activity.class).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE);
        return new Notification.Builder(this,"voice").setSmallIcon(android.R.drawable.ic_btn_speak_now).setContentTitle("Voice conversation").setContentText(status).setContentIntent(open).setCategory(Notification.CATEGORY_SERVICE).setOngoing(true).setOnlyAlertOnce(true).addAction(new Notification.Action.Builder(null,"Stop voice",action).build()).build();
    }
    private void notification(String status){try{if(!notificationsVisible(this)){stopSelf();return;}if(status.equals(lastStatus))return;lastStatus=status;((NotificationManager)getSystemService(NOTIFICATION_SERVICE)).notify(2,notice(status));}catch(RuntimeException failure){stopSelf();}}
    @Override public IBinder onBind(Intent intent){return null;}
    @Override public int onStartCommand(Intent intent,int flags,int id){
        if(intent==null){stopSelf();return START_NOT_STICKY;}
        String value=intent.getStringExtra("session");
        if("stop".equals(intent.getAction())){if(active&&session.equals(value))stopSelf();else if(!active)stopSelf();return START_NOT_STICKY;}
        Activity activity=owner.get();
        if(active){if(activity==null)stopSelf();else if(session.equals(value))activity.voiceSessionReady(value);return START_NOT_STICKY;}
        if(activity==null||!activity.voiceForeground()||!sessionMatches(value)||SystemClock.elapsedRealtime()>=deadline){
            if(sessionMatches(value)){owner.clear();requested="";deadline=0;if(activity!=null)activity.voiceSessionStopped(value);}
            stopSelf();return START_NOT_STICKY;
        }
        session=value;current=this;
        try{
            Notification n=notice("Starting microphone");
            if(!notificationsVisible(this))throw new SecurityException("Voice notification is unavailable");
            if(Build.VERSION.SDK_INT>=30)startForeground(2,n,ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE|ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
            else if(Build.VERSION.SDK_INT>=29)startForeground(2,n,ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
            else startForeground(2,n);
            long remaining=Math.max(1,deadline-SystemClock.elapsedRealtime());
            wake=((PowerManager)getSystemService(POWER_SERVICE)).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK,"voice:session");wake.setReferenceCounted(false);wake.acquire(remaining);
            active=true;handler.postDelayed(expiry,remaining);handler.postDelayed(health,Math.min(2000,remaining));activity.voiceSessionReady(session);
        }catch(RuntimeException failure){stopSelf();}
        return START_NOT_STICKY;
    }
    @Override public void onTaskRemoved(Intent root){stopSelf();}
    @Override public void onDestroy(){
        handler.removeCallbacksAndMessages(null);Activity activity=owner.get();String ended=session;
        if(current==this){current=null;owner.clear();requested="";deadline=0;}
        active=false;try{stopForeground(STOP_FOREGROUND_REMOVE);}catch(RuntimeException retired){}
        if(wake!=null){try{if(wake.isHeld())wake.release();}catch(RuntimeException retired){}wake=null;}
        if(activity!=null&&!ended.isEmpty())activity.voiceSessionStopped(ended);
        super.onDestroy();
    }
}
