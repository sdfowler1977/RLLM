package com.local.client;

import android.app.usage.UsageEvents;
import android.app.usage.UsageEventsQuery;
import android.app.usage.UsageStatsManager;
import android.content.Context;
import android.os.SystemClock;
import java.io.IOException;
import java.util.Collections;
import org.json.JSONObject;

/** API 35 platform binding; callers must check the platform version first. */
final class Usage {
    interface Guard { void check() throws Exception; }
    private static final int LIMIT=10000;
    private static final long BUDGET_MS=2000;
    private Usage() {}

    static JSONObject read(Context context,JSONObject command,Guard guard,Guard access)throws Exception {
        final String selected=command.getString("package");
        if(!selected.matches("[A-Za-z0-9_.]{1,255}"))throw new IOException("Select a nonempty package for usage observation");
        final String beginText=command.getString("begin_ms"),endText=command.getString("end_ms");
        if(!beginText.matches("[0-9]{1,19}")||!endText.matches("[0-9]{1,19}"))throw new IOException("Invalid usage observation interval");
        final long begin=Long.parseLong(beginText),end=Long.parseLong(endText);
        if(begin<0||end<=begin||(end-begin!=86400000L&&end-begin!=604800000L))throw new IOException("Usage observation requires a fixed 24-hour or seven-day window");
        final int[] types={UsageEvents.Event.ACTIVITY_RESUMED,UsageEvents.Event.ACTIVITY_PAUSED,UsageEvents.Event.FOREGROUND_SERVICE_START,UsageEvents.Event.FOREGROUND_SERVICE_STOP};
        UsageEventsQuery query=new UsageEventsQuery.Builder(begin,end).setPackageNames(selected).setEventTypes(types).build();
        if(!query.getPackageNames().equals(Collections.singleton(selected)))throw new IOException("Android did not preserve the selected-package usage filter");
        int[] actual=query.getEventTypes();java.util.Arrays.sort(actual);int[] expected=types.clone();java.util.Arrays.sort(expected);
        if(!java.util.Arrays.equals(actual,expected)||query.getBeginTimeMillis()!=begin||query.getEndTimeMillis()!=end)throw new IOException("Android did not preserve the usage query boundaries");
        UsageStatsManager manager=(UsageStatsManager)context.getSystemService(Context.USAGE_STATS_SERVICE);
        if(manager==null)throw new IOException("Android usage service unavailable");
        access.check();
        UsageEvents events=manager.queryEvents(query);
        access.check();
        if(events==null)throw new IOException("Android usage history unavailable; no observation was adopted");
        long started=SystemClock.elapsedRealtime();int count=0,resumed=0,paused=0,serviceStarted=0,serviceStopped=0;
        UsageEvents.Event event=new UsageEvents.Event();
        while(events.hasNextEvent()){
            guard.check();
            if(count>=LIMIT||SystemClock.elapsedRealtime()-started>BUDGET_MS)throw new IOException("Usage observation exceeds 10000 records or two seconds of iteration; no partial counts adopted");
            if(!events.getNextEvent(event))throw new IOException("Android usage event cursor ended unexpectedly");
            count++;
            if(!selected.equals(event.getPackageName())||event.getTimeStamp()<begin||event.getTimeStamp()>=end)throw new IOException("Android returned an event outside the selected package or time window");
            switch(event.getEventType()){
                case UsageEvents.Event.ACTIVITY_RESUMED:resumed++;break;
                case UsageEvents.Event.ACTIVITY_PAUSED:paused++;break;
                case UsageEvents.Event.FOREGROUND_SERVICE_START:serviceStarted++;break;
                case UsageEvents.Event.FOREGROUND_SERVICE_STOP:serviceStopped++;break;
                default:throw new IOException("Android returned an unrequested usage event type");
            }
        }
        if(SystemClock.elapsedRealtime()-started>BUDGET_MS)throw new IOException("Usage observation exceeded two seconds of iteration; no partial counts adopted");
        access.check();
        return new JSONObject().put("package",selected).put("begin_ms",beginText).put("end_ms",endText).put("observed_ms",Long.toString(System.currentTimeMillis())).put("resumed",resumed).put("paused",paused).put("service_started",serviceStarted).put("service_stopped",serviceStopped);
    }
}
