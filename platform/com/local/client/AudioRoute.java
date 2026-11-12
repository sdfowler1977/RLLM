package com.local.client;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.media.AudioManager;
import android.os.Build;

/** Activity-scoped binding for the protected system audio-route broadcast. */
final class AudioRoute implements AutoCloseable {
    private final Context context;
    private final Runnable changed;
    private boolean registered,closed;
    private BroadcastReceiver receiver;
    AudioRoute(Context context,Runnable changed){this.context=context;this.changed=changed;}
    boolean watch(){
        if(closed)return false;if(registered)return true;
        BroadcastReceiver current=new BroadcastReceiver(){
            @Override public void onReceive(Context source,Intent intent){
                if(receiver==this&&registered&&!closed&&intent!=null&&AudioManager.ACTION_AUDIO_BECOMING_NOISY.equals(intent.getAction()))changed.run();
            }
        };
        receiver=current;
        try{
            IntentFilter filter=new IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY);
            // This protected system action may originate from the Bluetooth stack.
            // Its only effect is stopping current audio, never starting work.
            if(Build.VERSION.SDK_INT>=33)context.registerReceiver(current,filter,Context.RECEIVER_EXPORTED);
            else context.registerReceiver(current,filter);
            registered=true;return true;
        }catch(RuntimeException unavailable){receiver=null;return false;}
    }
    void stop(){if(!registered)return;registered=false;BroadcastReceiver old=receiver;receiver=null;try{context.unregisterReceiver(old);}catch(RuntimeException alreadyGone){/* Retired callbacks remain inert. */}}
    @Override public void close(){closed=true;stop();}
}
