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
import java.lang.ref.WeakReference;
import org.json.JSONObject;

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
 * The lease clamp and the elapsed-time text come from the core turn clock
 * through Core.call, the Java side of Java_com_local_client_Core_process.
 * The service still measures the elapsed instant itself.
 *
 * The ticket admitted by begin is the authority for a queued start. finish
 * retires that ticket even when no service instance exists yet, so a start
 * that arrives after the turn ended, or after a newer turn began, never
 * promotes and never takes the wake lock.
 */
public final class TurnService extends Service {
    static final int HELD=1, QUEUED=2, REFUSED=3;
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
    private static final class Ticket { final String id; Ticket(String id){this.id=id;} }
    private static volatile Ticket live;
    private static WeakReference<Activity> owner=new WeakReference<>(null);
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

    private static boolean openTicket(String value){Ticket held=live;return held!=null&&value!=null&&value.equals(held.id);}
    /** One clamp, owned by the core turn clock. Both the fresh start and an extension call this. */
    static long leaseOf(long window)throws Exception{JSONObject value=(JSONObject)Core.call(new JSONObject().put("op","turn_clock").put("window_ms",window));return value.getLong("lease_ms");}
    private static String elapsedText(long elapsed)throws Exception{JSONObject value=(JSONObject)Core.call(new JSONObject().put("op","turn_clock").put("elapsed_ms",elapsed));return value.getString("text");}
    private static void lost(){Activity activity=owner.get();if(activity!=null)activity.turnProtectionLost();}

    /**
     * Keep a started turn's process alive for as long as the turn may run.
     * HELD means the live instance took the new ticket. QUEUED means the
     * platform accepted a start. REFUSED means this process is unprotected.
     */
    static int begin(Activity activity,String ticket,long started,long lease,String label){
        if(activity==null||ticket==null||ticket.isEmpty())return REFUSED;
        owner=new WeakReference<>(activity);
        TurnService service=current;
        if(service!=null&&service.active){live=new Ticket(ticket);try{service.extend(ticket,started,lease,label);return HELD;}catch(Exception refused){if(openTicket(ticket))live=null;service.stop();return REFUSED;}}
        live=new Ticket(ticket);
        try{activity.startForegroundService(new Intent(activity,TurnService.class).putExtra(EXTRA_TICKET,ticket).putExtra(EXTRA_LABEL,label).putExtra(EXTRA_STARTED,started).putExtra(EXTRA_LEASE,lease));return QUEUED;}
        catch(RuntimeException refused){if(openTicket(ticket))live=null;return REFUSED;}
    }

    /**
     * The turn ended. Retire the admitted ticket first, so a start still
     * waiting on the main looper finds nothing to promote. Then release the
     * instance when one exists. The reply itself is published separately, so
     * a reply that the platform cannot show yet is never lost.
     */
    static void finish(){live=null;TurnService service=current;if(service!=null)service.stop();}

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

    private void extend(String value,long from,long window,String text)throws Exception{
        if(!openTicket(value))throw new IllegalStateException("Turn ticket is not admitted");
        long next=leaseOf(window);
        if(wake==null)throw new IllegalStateException("Turn wake lock is not held");
        try{wake.acquire(next);}catch(RuntimeException refused){throw refused;}
        ticket=value;label=text==null||text.isEmpty()?"Replying":text;started=from;lease=next;
        publish(SystemClock.elapsedRealtime()-started);
    }

    private Notification progress(long elapsed)throws Exception{
        NotificationManager manager=(NotificationManager)getSystemService(NOTIFICATION_SERVICE);
        manager.createNotificationChannel(new NotificationChannel(PROGRESS_CHANNEL,"Working",NotificationManager.IMPORTANCE_LOW));
        return new Notification.Builder(this,PROGRESS_CHANNEL).setSmallIcon(android.R.drawable.stat_notify_sync).setContentTitle("RLLM is working").setContentText(label+" · "+elapsedText(elapsed)).setOngoing(true).setOnlyAlertOnce(true).setCategory(Notification.CATEGORY_PROGRESS).build();
    }

    private void publish(long elapsed){try{((NotificationManager)getSystemService(NOTIFICATION_SERVICE)).notify(PROGRESS_ID,progress(elapsed));}catch(Exception unavailable){}}

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
        String text=intent.getStringExtra(EXTRA_LABEL);
        // stopSelf(id) retires this start only. A bare stopSelf() would drop a newer queued start and a live lease whose ticket is still admitted.
        if(!openTicket(value)){if(!(active&&openTicket(ticket)))stopSelf(id);return START_NOT_STICKY;}
        // No lease extra means the core value never arrived. Do not substitute a timeout.
        if(!intent.hasExtra(EXTRA_LEASE)){lost();if(openTicket(value))live=null;stop();return START_NOT_STICKY;}
        long window=intent.getLongExtra(EXTRA_LEASE,0L);
        if(active){try{extend(value,from,window,text);}catch(Exception refused){lost();if(openTicket(value))live=null;stop();}return START_NOT_STICKY;}
        try{
            ticket=value;label=text==null||text.isEmpty()?"Replying":text;started=from;
            lease=leaseOf(window);
            Notification notification=progress(0);
            if(Build.VERSION.SDK_INT>=29)startForeground(PROGRESS_ID,notification,ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC);
            else startForeground(PROGRESS_ID,notification);
            wake=((PowerManager)getSystemService(POWER_SERVICE)).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK,"rllm:turn");
            wake.setReferenceCounted(false);wake.acquire(lease);
            active=true;current=this;
            publish(0);
            handler.postDelayed(ticker,1000);
        }catch(Exception refused){lost();if(openTicket(value))live=null;stop();}
        return START_NOT_STICKY;
    }
    @Override public void onTaskRemoved(Intent root){stopSelf();}
    @Override public void onDestroy(){active=false;handler.removeCallbacksAndMessages(null);try{stopForeground(STOP_FOREGROUND_REMOVE);}catch(RuntimeException retired){}release();if(live!=null&&ticket.equals(live.id))live=null;if(current==this)current=null;super.onDestroy();}
}
