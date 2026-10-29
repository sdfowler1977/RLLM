package com.local.client;

import java.io.IOException;

/** Ephemeral monotonic ownership for one page-read redirect chain. */
final class Deadline {
    private String session="", ticket="";
    private long until;
    synchronized long begin(String root,String next,String parent,long now) throws IOException {
        if(root.isEmpty()||next.isEmpty()||root.length()>128||next.length()>128||parent.length()>128)
            throw new IOException("Invalid page-read owner");
        if(parent.isEmpty()) {
            if(!root.equals(next)||root.equals(session))throw new IOException("Duplicate page-read start");
            if(now<0||now>Long.MAX_VALUE-120000)throw new IOException("Invalid monotonic clock");
            session=root;ticket=next;until=now+120000;
        } else {
            if(!root.equals(session)||!parent.equals(ticket)||next.equals(ticket))
                throw new IOException("Page-read continuation owner changed or unavailable");
            remaining(until,now);ticket=next;
        }
        return until;
    }
    synchronized void clear(){session="";ticket="";until=0;}
    static int remaining(long until,long now) throws IOException {
        if(now<0||until<=now||until-now>120000)throw new IOException("Page-read deadline expired or unavailable");
        return (int)(until-now);
    }
}
