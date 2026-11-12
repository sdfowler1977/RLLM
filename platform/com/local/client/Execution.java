package com.local.client;
import android.app.Service;
import android.content.Intent;
import android.os.*;
import org.json.JSONObject;

/** Isolated Android UID binding: no inherited application storage or network permissions. */
public final class Execution extends Service {
    private final java.util.concurrent.atomic.AtomicBoolean running=new java.util.concurrent.atomic.AtomicBoolean(false);
    private final Messenger messenger=new Messenger(new Handler(Looper.getMainLooper()){
        @Override public void handleMessage(Message msg){if(msg.what==2){cancel();return;}final Messenger reply=msg.replyTo;if(!running.compareAndSet(false,true)){Message response=Message.obtain(null,1);Bundle error=new Bundle();error.putString("error","An isolated program is already running");response.setData(error);try{reply.send(response);}catch(RemoteException ignored){}return;}final Bundle data=msg.getData();new Thread(()->{Bundle result=new Bundle();try{Object output=Core.call(new JSONObject().put("op","script").put("source",data.getString("source")).put("check",data.getBoolean("check")));result.putString("output",output.toString());}catch(Throwable e){result.putString("error",e.getMessage()==null?"Execution failed":e.getMessage());}running.set(false);Message response=Message.obtain(null,1);response.setData(result);try{reply.send(response);}catch(RemoteException ignored){cancel();}},"program").start();}
    });
    private void cancel(){try{Core.call(new JSONObject().put("op","cancel_tool"));}catch(Exception ignored){}}
    @Override public IBinder onBind(Intent intent){return messenger.getBinder();}
    @Override public void onDestroy(){cancel();super.onDestroy();}
}
