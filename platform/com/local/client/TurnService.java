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
import java.io.IOException;
import java.lang.ref.WeakReference;
import java.net.Socket;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.ThreadFactory;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicReference;
import org.json.JSONException;
import org.json.JSONObject;

/**
 * The owner of a model turn, and the platform half of the turn operation.
 *
 * The portable policy of a turn lives in Rust behind one typed operation,
 * op "turn" with the actions begin, extend, tick, expire, finish, publish and
 * rebind. The admission state machine, the cadence, the lease-expiry decision,
 * the reply text and its limit, the titles and the notification channel
 * defaults are decided there, not here. The Java side of that operation is
 * Core.call, the JNI entry point Java_com_local_client_Core_process. This file
 * adds no second call site into the core and decides no turn policy of its own.
 *
 * A turn has one window: the end-to-end bound of the request it carries, and
 * the core decides it. The promotion of the turn lasts exactly the time the
 * request has left, so the wake lock and the foreground service can neither
 * outlive the request nor end before it. The platform never computes that
 * time itself: it asks the core for the lease and holds the wake lock for
 * exactly as long as the core says. Nothing here renews a promotion, so there
 * is no path by which the protection of a turn could outlive its request.
 *
 * Two clocks meet in a turn, and they are not the same clock. The request runs
 * on the elapsed time since the turn started, and that is the progress a user
 * reads. The promotion runs on the elapsed time since its own lease was
 * decided. A decision about the promotion never rewinds the progress of the
 * request.
 *
 * The platform bridge is strictly what the core cannot do for itself:
 *  - the foreground dataSync promotion and the partial wake lock that keep a
 *    turn alive while the app is not on screen,
 *  - the notification channels and the notifications themselves,
 *  - the process-scoped threads, scheduler, resolver and sockets that carry
 *    the model transport, so that a turn outlives the Activity that started
 *    it,
 *  - the attachment of the view that renders the result, and the holding of a
 *    result that no view is there to receive.
 *
 * The live turn of this process is the pair of a request ticket and a holder
 * token that the core issued for it. Only that holder may extend the lease or
 * retire the identity, so a teardown of an older turn cannot strip the lease,
 * the notification or the identity of a newer live turn. The ticket alone is
 * not the identity: the holder is what makes a stale teardown distinguishable.
 */
public final class TurnService extends Service {
    static final int PROGRESS_ID=3;
    static final int REPLY_ID=4;
    /** The conversation a published reply belongs to, so a tap can open it. */
    static final String EXTRA_CHAT="rllm.conversation";
    private static final String PROGRESS_CHANNEL="turn";
    private static final String REPLY_CHANNEL="replies";
    private static final String EXTRA_TICKET="ticket";
    private static final String EXTRA_HOLDER="holder";
    private static final String EXTRA_LABEL="label";
    private static final String EXTRA_STARTED="started";
    private static final String EXTRA_WINDOW="window";
    private static final String EXTRA_CADENCE="cadence";

    /** The live turn of this process, and the only scope that may promote. */
    private static final class Live {
        final String ticket,holder,label;
        final int token;
        final long started;
        volatile long lease,cadence;
        volatile boolean aborted;
        Live(String ticket,String holder,String label,int token,long started,long lease,long cadence){
            this.ticket=ticket;this.holder=holder;this.label=label;this.token=token;this.started=started;this.lease=lease;this.cadence=cadence;
        }
    }

    /**
     * A protection loss that no view was there to hear. It is held in the
     * process scope, so the loss is announced by the view that returns instead
     * of passing in silence.
     */
    private static final class PendingLoss {
        final String notice;
        PendingLoss(String notice){this.notice=notice;}
    }
    private static final AtomicReference<PendingLoss> PENDING_LOSS=new AtomicReference<>(null);

    private static volatile Live live;
    private static volatile JSONObject remembered;
    /**
     * The transport tokens of this process. They are negative on purpose: the
     * token of a view and the token of a turn must never be the same number,
     * or a liveness check would answer for the wrong owner.
     */
    private static final AtomicInteger TOKENS=new AtomicInteger(1);
    private static final ThreadFactory OWNER_THREADS=new ThreadFactory(){
        private final AtomicInteger index=new AtomicInteger();
        public Thread newThread(Runnable body){Thread thread=new Thread(body,"rllm-turn-"+index.incrementAndGet());thread.setDaemon(true);return thread;}
    };
    /** The threads that carry a model turn. They belong to the process, not to a view. */
    private static final ExecutorService WORKERS=Executors.newFixedThreadPool(Math.max(2,Math.min(4,Runtime.getRuntime().availableProcessors())),OWNER_THREADS);
    /** The scheduler that carries a model turn's deadlines. */
    private static final ScheduledExecutorService DEADLINES=Executors.newSingleThreadScheduledExecutor(OWNER_THREADS);
    /** The resolver a model turn uses. It is never closed while the process lives. */
    private static final Resolution RESOLVER=new Resolution();
    private static final Map<Integer,Set<Socket>> SOCKETS=new ConcurrentHashMap<>();
    private static final Map<String,JSONObject> RESULTS=new ConcurrentHashMap<>();
    /** The callback each held result belongs to, so a returning view dispatches it as it was. */
    private static final Map<String,String> CALLBACKS=new ConcurrentHashMap<>();
    private static WeakReference<Activity> owner=new WeakReference<>(null);
    private static TurnService current;

    private final Handler handler=new Handler(Looper.getMainLooper());
    private PowerManager.WakeLock wake;
    private String ticket="";
    private String holder="";
    private String label="";
    private long started;
    private long lease;
    private long leaseSince;
    private long cadence=1000;
    private boolean active;
    private final Runnable ticker=new Runnable(){public void run(){
        if(!active)return;
        try{
            // The request's own clock runs from the start of the turn; the
            // promotion's clock runs from the decision of its lease. The core
            // reads both, so the progress a user sees never rewinds and the
            // protection never outlives the request.
            long now=SystemClock.elapsedRealtime();
            JSONObject tick=tick(now-started,now-leaseSince);
            if(tick.optBoolean("expired")){expire();return;}
            publish(tick);
            handler.postDelayed(this,Math.max(200,tick.optLong("cadence_ms",1000)));
        }catch(Exception unavailable){lost();}
    }};

    // --- The one typed operation. Every turn decision comes from here. ---
    private static JSONObject call(String action,JSONObject fields)throws Exception{
        JSONObject request=new JSONObject().put("op","turn").put("action",action);
        if(fields!=null){java.util.Iterator<String> keys=fields.keys();while(keys.hasNext()){String key=keys.next();request.put(key,fields.get(key));}}
        return (JSONObject)Core.call(request);
    }
    /** Admit a turn. The core decides queued, held or refused and issues the holder token. */
    static JSONObject begin(JSONObject state,String value,long window,long from,long elapsed)throws Exception{
        JSONObject fields=new JSONObject().put("state",state==null?new JSONObject():state).put("ticket",value==null?"":value).put("window_ms",window).put("started_ms",from).put("elapsed_ms",elapsed);
        String current=liveTicket();
        if(!current.isEmpty())fields.put("live_ticket",current);
        return call("begin",fields);
    }
    /**
     * The lease decision. Only the holder of the durable identity is extended,
     * and the lease is the time the request has left, so a promotion can
     * neither outlive the request it protects nor end before it.
     */
    static JSONObject renew(JSONObject state,String value,String holderToken,long window,long elapsed)throws Exception{
        return call("extend",new JSONObject().put("state",state==null?new JSONObject():state).put("ticket",value==null?"":value).put("holder",holderToken==null?"":holderToken).put("window_ms",window).put("elapsed_ms",elapsed));
    }
    /** Retire the durable identity, but only for the holder that owns it. */
    static JSONObject finish(JSONObject state,String value,String holderToken)throws Exception{
        return call("finish",new JSONObject().put("state",state==null?new JSONObject():state).put("ticket",value==null?"":value).put("holder",holderToken==null?"":holderToken));
    }
    /** Compose a reply, with its limit, its title and its channel. */
    static JSONObject publish(JSONObject state,String body,String chat,String value,boolean failed,boolean backgrounded,boolean allowed)throws Exception{
        return call("publish",new JSONObject().put("state",state==null?new JSONObject():state).put("body",body==null?"":body).put("chat",chat==null?"":chat).put("ticket",value==null?"":value).put("failed",failed).put("backgrounded",backgrounded).put("notifications_allowed",allowed));
    }
    /** Drain the announcement that is held in the persisted state for a later grant. */
    static JSONObject drain(JSONObject state,boolean allowed)throws Exception{
        return call("publish",new JSONObject().put("state",state==null?new JSONObject():state).put("drain",true).put("notifications_allowed",allowed));
    }
    /** Re-bind to the live turn of this process by its durable identity. */
    static JSONObject rebind(JSONObject state)throws Exception{
        JSONObject fields=new JSONObject().put("state",state==null?new JSONObject():state);
        String value=liveTicket(),holderToken=liveHolder();
        if(!value.isEmpty())fields.put("live_ticket",value);
        if(!holderToken.isEmpty())fields.put("live_holder",holderToken);
        return call("rebind",fields);
    }
    /**
     * The progress and the expiry of one tick. The request's elapsed time and
     * the promotion's elapsed time are two different measurements, and the
     * core reads both: the progress of the request and the expiry of the
     * promotion are decided from their own clocks.
     */
    private JSONObject tick(long elapsed,long leaseElapsed)throws Exception{return call("tick",new JSONObject().put("elapsed_ms",elapsed).put("lease_elapsed_ms",leaseElapsed).put("lease_ms",lease));}
    /** The notice a lost lease shows. It is the notice a refused start shows. */
    private static JSONObject expireNotice()throws Exception{return call("expire",new JSONObject());}

    // --- The process scope of a turn. ---
    /**
     * The next transport token of this process. It is negative, so it can never
     * be the generation counter of a view.
     */
    static int nextToken(){return -TOKENS.incrementAndGet();}
    static ExecutorService workers(){return WORKERS;}
    static ScheduledExecutorService deadlines(){return DEADLINES;}
    static Resolution resolver(){return RESOLVER;}
    static String liveTicket(){Live held=live;return held==null?"":held.ticket;}
    static String liveHolder(){Live held=live;return held==null?"":held.holder;}
    static int liveToken(){Live held=live;return held==null?0:held.token;}
    static long liveStarted(){Live held=live;return held==null?0L:held.started;}
    /** Whether a transport token still belongs to a turn that runs in this process. */
    static boolean alive(int token){Live held=live;return held!=null&&held.token==token&&!held.aborted;}
    /** The state the owner last committed. It carries the durable identity of the live turn. */
    static void remember(JSONObject state){remembered=state;}
    static void track(int token,Socket socket){SOCKETS.computeIfAbsent(token,key->ConcurrentHashMap.newKeySet()).add(socket);}
    static void untrack(int token,Socket socket){Set<Socket> held=SOCKETS.get(token);if(held!=null)held.remove(socket);}
    /** The user cancelled. The live turn's transport really stops. */
    static void abort(){Live held=live;if(held==null)return;held.aborted=true;Set<Socket> sockets=SOCKETS.get(held.token);if(sockets!=null)for(Socket socket:sockets)try{socket.close();}catch(IOException ignored){}}
    /** The view that renders the result, if there is one. */
    static void attach(Activity activity){if(activity!=null)owner=new WeakReference<>(activity);}
    /** The view is gone. The turn keeps running without it. */
    static void detach(Activity activity){Activity held=owner.get();if(held==activity)owner=new WeakReference<>(null);}
    /** Hand a result to the attached view, or hold it for the view that returns. */
    static void deliver(int token,String callback,JSONObject payload){
        Live held=live;if(held==null||held.token!=token||held.aborted||payload==null)return;
        Activity view=owner.get();
        if(view!=null&&!view.isClosed()){view.turnResult(callback,payload);return;}
        RESULTS.put(held.ticket,payload);
        CALLBACKS.put(held.ticket,callback==null?"network_result":callback);
    }
    /**
     * The result a returning view adopts, once it has re-bound by its identity.
     * The callback travels with the payload, so the view dispatches the event
     * the turn really produced rather than a guessed one.
     */
    static JSONObject take(String value){
        if(value==null||value.isEmpty())return null;
        JSONObject payload=RESULTS.remove(value);
        if(payload==null)return null;
        String callback=CALLBACKS.remove(value);
        try{return new JSONObject().put("callback",callback==null?"network_result":callback).put("payload",payload);}
        catch(JSONException missing){return null;}
    }

    /**
     * Realize the promotion of an admitted turn. The core already decided the
     * admission, so this only makes the process scope real: the live turn, and
     * the promotion that keeps it alive while the app is not on screen. It
     * answers whether the platform could promote the turn, never whether the
     * turn may run: that decision belongs to the core alone.
     */
    static boolean promote(Activity activity,String value,String holderToken,int token,long from,long window,long leaseMs,long cadenceMs,String text){
        if(activity==null||value==null||value.isEmpty()||holderToken==null||holderToken.isEmpty())return false;
        Live scope=new Live(value,holderToken,text,token,from,leaseMs,cadenceMs);
        live=scope;
        attach(activity);
        TurnService service=current;
        if(service!=null&&service.active){
            try{service.extend(value,holderToken,from,window,cadenceMs,text);return true;}
            catch(Exception refused){if(live==scope)live=null;service.stop();return false;}
        }
        try{
            activity.startForegroundService(new Intent(activity,TurnService.class).putExtra(EXTRA_TICKET,value).putExtra(EXTRA_HOLDER,holderToken).putExtra(EXTRA_LABEL,text).putExtra(EXTRA_STARTED,from).putExtra(EXTRA_WINDOW,window).putExtra(EXTRA_CADENCE,cadenceMs));
            return true;
        }catch(RuntimeException refused){if(live==scope)live=null;return false;}
    }
    /**
     * The turn ended. The live scope is released only for the holder that owns
     * it, so a stale teardown leaves a newer live turn alone.
     */
    static void release(String value,String holderToken){
        Live held=live;
        if(held==null)return;
        if(value==null||holderToken==null||!value.equals(held.ticket)||!holderToken.equals(held.holder))return;
        live=null;
        TurnService service=current;
        if(service!=null)service.stop();
    }

    /** Whether the platform currently lets this app show a notification. */
    static boolean notificationsAllowed(Activity activity){
        if(Build.VERSION.SDK_INT<33)return true;
        try{return activity.checkSelfPermission("android.permission.POST_NOTIFICATIONS")==PackageManager.PERMISSION_GRANTED;}
        catch(RuntimeException unavailable){return false;}
    }

    /** Publish a committed reply so it reaches a user who is not looking at the app. The text is already composed. */
    static void post(Activity activity,String channel,String channelName,String title,String body,String conversation){
        if(activity==null||body==null||body.trim().isEmpty())return;
        try{
            NotificationManager manager=(NotificationManager)activity.getSystemService(NOTIFICATION_SERVICE);
            String id=channel==null||channel.isEmpty()?REPLY_CHANNEL:channel;
            manager.createNotificationChannel(new NotificationChannel(id,channelName==null||channelName.isEmpty()?"Replies":channelName,NotificationManager.IMPORTANCE_DEFAULT));
            Intent open=new Intent(activity,Activity.class).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);
            if(conversation!=null&&!conversation.isEmpty())open.putExtra(EXTRA_CHAT,conversation);
            PendingIntent tap=PendingIntent.getActivity(activity,REPLY_ID,open,PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE);
            String text=body.trim();
            Notification notification=new Notification.Builder(activity,id).setSmallIcon(android.R.drawable.stat_notify_chat).setContentTitle(title).setContentText(text).setStyle(new Notification.BigTextStyle().bigText(text)).setContentIntent(tap).setAutoCancel(true).setCategory(Notification.CATEGORY_MESSAGE).build();
            manager.notify(REPLY_ID,notification);
        }catch(RuntimeException unavailable){/* A refused notification never blocks the committed reply. */}
    }

    private static boolean openTicket(String value,String holderToken){Live held=live;return held!=null&&value!=null&&value.equals(held.ticket)&&holderToken!=null&&holderToken.equals(held.holder);}
    private static void lost(){lost("");}
    /**
     * The protection was lost. The notice is the one the refused start shows.
     * A loss that no view was there to hear is held in the process scope, so
     * the view that returns announces it instead of the loss passing in
     * silence.
     */
    private static void lost(String notice){
        Activity activity=owner.get();
        if(activity!=null&&!activity.isClosed()){activity.turnProtectionLost(notice);return;}
        PENDING_LOSS.set(new PendingLoss(notice==null?"":notice));
    }
    /** The held protection loss, or null when no loss is waiting for a view. */
    static String takePendingLoss(){PendingLoss held=PENDING_LOSS.getAndSet(null);return held==null?null:held.notice;}

    /**
     * Re-target this instance at the turn that holds the lease scope now. Only
     * the holder of the live identity may extend it, so a stale start cannot
     * keep the wake lock of a newer live turn. The lease is the core's
     * decision and never a number computed here: it is the time the request
     * has left, so the wake lock ends with the request.
     */
    private void extend(String value,String holderToken,long from,long window,long cadenceMs,String text)throws Exception{
        if(!openTicket(value,holderToken))throw new IllegalStateException("Turn ticket is not admitted");
        if(wake==null)throw new IllegalStateException("Turn wake lock is not held");
        long now=SystemClock.elapsedRealtime();
        long next=leaseFor(value,holderToken,window,now-from);
        try{wake.acquire(next);}catch(RuntimeException refused){throw refused;}
        ticket=value;holder=holderToken;label=text==null?"":text;started=from;lease=next;leaseSince=now;cadence=cadenceMs>0?cadenceMs:1000;
    }
    /**
     * The lease the core decides for this turn. The core measures the elapsed
     * time of the request and answers with the time the request has left, so
     * the promotion ends exactly with the request and never after it. A turn
     * whose identity this process no longer holds is refused the lease, and a
     * turn with no time left is refused as well.
     */
    private static long leaseFor(String value,String holderToken,long window,long elapsed)throws Exception{
        JSONObject decided=renew(remembered,value,holderToken,window,elapsed);
        if(!decided.optBoolean("extended"))throw new IllegalStateException("Turn identity is no longer held");
        long next=decided.optLong("lease_ms",0L);
        if(next<=0)throw new IllegalStateException("Turn has no time left");
        return next;
    }
    private Notification progress(JSONObject tick)throws Exception{
        NotificationManager manager=(NotificationManager)getSystemService(NOTIFICATION_SERVICE);
        String id=tick.optString("channel",PROGRESS_CHANNEL);
        manager.createNotificationChannel(new NotificationChannel(id,tick.optString("channel_name","Working"),NotificationManager.IMPORTANCE_LOW));
        return new Notification.Builder(this,id).setSmallIcon(android.R.drawable.stat_notify_sync).setContentTitle(tick.optString("title","RLLM is working")).setContentText(tick.optString("progress_text")).setOngoing(true).setOnlyAlertOnce(true).setCategory(Notification.CATEGORY_PROGRESS).build();
    }
    private void publish(JSONObject tick){try{((NotificationManager)getSystemService(NOTIFICATION_SERVICE)).notify(PROGRESS_ID,progress(tick));}catch(Exception unavailable){}}
    /** The lease ran out. The turn still runs; only the process protection is lost, and the user is told. */
    private void expire(){
        String notice="";
        try{notice=expireNotice().optString("notice");}catch(Exception unavailable){}
        lost(notice);
        stop();
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
        String holderToken=intent.getStringExtra(EXTRA_HOLDER);
        if(value==null||value.isEmpty()||holderToken==null||holderToken.isEmpty()){stopSelf();return START_NOT_STICKY;}
        long from=intent.getLongExtra(EXTRA_STARTED,SystemClock.elapsedRealtime());
        String text=intent.getStringExtra(EXTRA_LABEL);
        long cadenceMs=intent.getLongExtra(EXTRA_CADENCE,1000L);
        // stopSelf(id) retires this start only. A bare stopSelf() would drop a newer queued start and a live lease whose ticket is still admitted.
        if(!openTicket(value,holderToken)){stopSelf(id);return START_NOT_STICKY;}
        // No window extra means the core value never arrived. Do not substitute a timeout.
        if(!intent.hasExtra(EXTRA_WINDOW)){lost();stop();return START_NOT_STICKY;}
        long window=intent.getLongExtra(EXTRA_WINDOW,0L);
        if(window<=0){lost();stop();return START_NOT_STICKY;}
        if(active){try{extend(value,holderToken,from,window,cadenceMs,text);}catch(Exception refused){lost();}return START_NOT_STICKY;}
        try{
            ticket=value;holder=holderToken;label=text==null?"":text;started=from;cadence=cadenceMs>0?cadenceMs:1000;
            // The lease is the core's decision and it is the time the request
            // has left, so the wake lock ends exactly with the request.
            long now=SystemClock.elapsedRealtime();
            lease=leaseFor(value,holderToken,window,now-from);leaseSince=now;
            Notification notification=progress(tick(now-started,0));
            if(Build.VERSION.SDK_INT>=29)startForeground(PROGRESS_ID,notification,ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC);
            else startForeground(PROGRESS_ID,notification);
            wake=((PowerManager)getSystemService(POWER_SERVICE)).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK,"rllm:turn");
            wake.setReferenceCounted(false);wake.acquire(lease);
            active=true;current=this;
            publish(tick(now-started,0));
            handler.postDelayed(ticker,Math.max(200,cadence));
        }catch(Exception refused){lost();stop();}
        return START_NOT_STICKY;
    }
    @Override public void onTaskRemoved(Intent root){stopSelf();}
    /**
     * The instance is gone. The live turn is not: its identity stays durable, so
     * a view that returns re-binds to it instead of starting a second turn.
     */
    @Override public void onDestroy(){active=false;handler.removeCallbacksAndMessages(null);try{stopForeground(STOP_FOREGROUND_REMOVE);}catch(RuntimeException retired){}release();if(current==this)current=null;super.onDestroy();}
}
