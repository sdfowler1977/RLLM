package com.local.client;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.PowerManager;
import android.os.SystemClock;

/**
 * Process lifetime for a model turn that has to outlive the Activity's focus.
 *
 * The Activity owns the turn's worker threads; this service owns the process
 * state those threads need. While a turn runs the service holds a foreground
 * dataSync promotion and a partial wake lock, so leaving the app - answering a
 * message, locking the screen - cannot let the platform reclaim the cached
 * process before the reply is committed to the encrypted history.
 *
 * The service starts no work of its own and cancels nothing. The Activity's
 * transport deadline, budget and cancellation paths stay the only authority
 * over the turn, and the Activity remains the only writer of product state.
 * The service never speaks to the product core: it reports elapsed time it
 * measures from the turn's own start instant, so the published progress is
 * exact while the app is in the background.
 */
public final class TurnService extends Service {
    static final int PROGRESS_ID=3;
    static final int REPLY_ID=4;
    /** The conversation a published reply belongs to, so a tap can open it. */
    static final String EXTRA_CHAT="rllm.conversation";
    private static final String PROGRESS_CHANNEL="turn";
    private static final String REPLY_CHANNEL="replies";
    private static final String EXTRA_TICKET="ticket";
    private static final String EXTRA_LABEL="label";
    private static final String EXTRA_STARTED="started";
    private static final String EXTRA_LEASE="lease";
    private static final long MIN_LEASE=30000L;
    private static final long MAX_LEASE=600000L;
    private static final long DEFAULT_LEASE=120000L;
    private static TurnService current;
    private final Handler handler=new Handler(Looper.getMainLooper());
    private PowerManager.WakeLock wake;
    private String ticket="";
    private String label="Replying";
    private long started;
    private long lease;
    private boolean active;
    private final Runnable ticker=new Runnable(){public void run(){
        if(!active)return;
        long elapsed=SystemClock.elapsedRealtime()-started;
        if(elapsed>=lease){stopSelf();return;}
        publish(elapsed);
        handler.postDelayed(this,1000);
    }};

    /** Keep a started turn's process alive for as long as the turn may run. */
    static void begin(Activity activity,String ticket,long started,long lease,String label){
        TurnService service=current;
        if(service!=null&&service.active){service.extend(ticket,started,lease,label);return;}
        try{activity.startForegroundService(new Intent(activity,TurnService.class).putExtra(EXTRA_TICKET,ticket).putExtra(EXTRA_LABEL,label).putExtra(EXTRA_STARTED,started).putExtra(EXTRA_LEASE,lease));}
        catch(RuntimeException refused){/* A background start can be refused when the app is not visible. The turn still runs in this process and its result is still committed. */}
    }

    /**
     * The turn ended. Release the process promotion. The reply itself is published
     * separately, so a reply that the platform cannot show yet is never lost.
     */
    static void finish(){
        TurnService service=current;
        if(service!=null)service.stop();
    }

    /** Whether the platform currently lets this app show a notification. */
    static boolean notificationsAllowed(Activity activity){
        if(Build.VERSION.SDK_INT<33)return true;
        try{return activity.checkSelfPermission("android.permission.POST_NOTIFICATIONS")==PackageManager.PERMISSION_GRANTED;}
        catch(RuntimeException unavailable){return false;}
    }

    /** Publish a committed reply so it reaches a user who is not looking at the app. */
    static void post(Activity activity,String title,String body,String conversation){
        if(body==null||body.trim().isEmpty())return;
        try{
            NotificationManager manager=(NotificationManager)activity.getSystemService(NOTIFICATION_SERVICE);
            manager.createNotificationChannel(new NotificationChannel(REPLY_CHANNEL,"Replies",NotificationManager.IMPORTANCE_DEFAULT));
            Intent open=new Intent(activity,Activity.class).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);
            if(conversation!=null&&!conversation.isEmpty())open.putExtra(EXTRA_CHAT,conversation);
            PendingIntent tap=PendingIntent.getActivity(activity,REPLY_ID,open,PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE);
            String text=body.trim();
            if(text.length()>600)text=text.substring(0,600)+"…";
            Notification notification=new Notification.Builder(activity,REPLY_CHANNEL).setSmallIcon(android.R.drawable.stat_notify_chat).setContentTitle(title).setContentText(text).setStyle(new Notification.BigTextStyle().bigText(text)).setContentIntent(tap).setAutoCancel(true).setCategory(Notification.CATEGORY_MESSAGE).build();
            manager.notify(REPLY_ID,notification);
        }catch(RuntimeException unavailable){/* A refused notification never blocks the committed reply. */}
    }

    private void extend(String value,long from,long window,String text){
        ticket=value;label=text==null||text.isEmpty()?"Replying":text;
        started=from;lease=Math.max(MIN_LEASE,Math.min(MAX_LEASE,window));
        if(wake!=null)try{if(wake.isHeld())wake.release();wake.acquire(lease);}catch(RuntimeException refused){}
        publish(SystemClock.elapsedRealtime()-started);
    }

    private static String seconds(long elapsed){
        long total=Math.max(0L,elapsed/1000L);
        if(total<60L)return total+"s";
        return (total/60L)+"m "+(total%60L)+"s";
    }

    private Notification progress(long elapsed){
        NotificationManager manager=(NotificationManager)getSystemService(NOTIFICATION_SERVICE);
        manager.createNotificationChannel(new NotificationChannel(PROGRESS_CHANNEL,"Working",NotificationManager.IMPORTANCE_LOW));
        return new Notification.Builder(this,PROGRESS_CHANNEL).setSmallIcon(android.R.drawable.stat_notify_sync).setContentTitle("RLLM is working").setContentText(label+" · "+seconds(elapsed)).setOngoing(true).setOnlyAlertOnce(true).setCategory(Notification.CATEGORY_PROGRESS).build();
    }

    private void publish(long elapsed){
        try{((NotificationManager)getSystemService(NOTIFICATION_SERVICE)).notify(PROGRESS_ID,progress(elapsed));}catch(RuntimeException unavailable){}
    }

    private void stop(){
        active=false;handler.removeCallbacksAndMessages(null);
        try{stopForeground(STOP_FOREGROUND_REMOVE);}catch(RuntimeException retired){}
        release();
        if(current==this)current=null;
        stopSelf();
    }

    private void release(){
        if(wake!=null){try{if(wake.isHeld())wake.release();}catch(RuntimeException retired){}wake=null;}
    }

    @Override public IBinder onBind(Intent intent){return null;}
    @Override public int onStartCommand(Intent intent,int flags,int id){
        if(intent==null){stopSelf();return START_NOT_STICKY;}
        String value=intent.getStringExtra(EXTRA_TICKET);
        if(value==null||value.isEmpty()){stopSelf();return START_NOT_STICKY;}
        long from=intent.getLongExtra(EXTRA_STARTED,SystemClock.elapsedRealtime());
        long window=intent.getLongExtra(EXTRA_LEASE,DEFAULT_LEASE);
        String text=intent.getStringExtra(EXTRA_LABEL);
        if(active){extend(value,from,window,text);return START_NOT_STICKY;}
        ticket=value;label=text==null||text.isEmpty()?"Replying":text;started=from;lease=Math.max(MIN_LEASE,Math.min(MAX_LEASE,window));
        current=this;
        try{
            Notification notification=progress(0);
            if(Build.VERSION.SDK_INT>=29)startForeground(PROGRESS_ID,notification,ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC);
            else startForeground(PROGRESS_ID,notification);
            wake=((PowerManager)getSystemService(POWER_SERVICE)).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK,"rllm:turn");
            wake.setReferenceCounted(false);wake.acquire(lease);
            active=true;current=this;
            publish(0);
            handler.postDelayed(ticker,1000);
        }catch(RuntimeException refused){stopSelf();}
        return START_NOT_STICKY;
    }
    @Override public void onTaskRemoved(Intent root){stopSelf();}
    @Override public void onDestroy(){active=false;handler.removeCallbacksAndMessages(null);try{stopForeground(STOP_FOREGROUND_REMOVE);}catch(RuntimeException retired){}release();if(current==this)current=null;super.onDestroy();}
}
