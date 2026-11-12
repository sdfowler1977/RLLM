package com.local.client;

import java.io.IOException;
import java.net.*;
import java.util.concurrent.*;

/** One resolver slot, no queued names and no ownership of network operations. */
final class Resolution implements AutoCloseable {
    interface Budget { int remaining() throws IOException; }
    interface Lookup { InetAddress[] addresses(String host) throws IOException; }
    private final Lookup lookup;
    private static final ThreadPoolExecutor worker=new ThreadPoolExecutor(0,1,10L,TimeUnit.SECONDS,new SynchronousQueue<Runnable>());
    private volatile boolean closed;
    Resolution(){this(InetAddress::getAllByName);}
    Resolution(Lookup lookup){this.lookup=lookup;}
    InetSocketAddress address(String host,int port,Budget budget) throws IOException {
        budget.remaining();
        if(closed)throw new IOException("Resolver is closed");
        if(host==null||host.isEmpty()||host.length()>255||port<1||port>65535)throw new IOException("Invalid network destination");
        InetAddress numeric=numeric(host);
        if(numeric!=null){budget.remaining();if(closed)throw new IOException("Resolver is closed");return new InetSocketAddress(numeric,port);}
        Future<InetAddress[]> pending;
        try{pending=worker.submit(()->lookup.addresses(host));}
        catch(RejectedExecutionException occupied){throw new IOException("Resolver is occupied or closed; this request was not queued",occupied);}
        try{
            for(;;){
                if(closed)throw new IOException("Resolver is closed");
                int remaining=budget.remaining();
                try{
                    InetAddress[] values=pending.get(Math.min(remaining,50),TimeUnit.MILLISECONDS);
                    budget.remaining();
                    if(closed)throw new IOException("Resolver is closed");
                    if(values==null||values.length==0||values.length>64)throw new IOException("Resolver returned an unsupported address count");
                    for(InetAddress value:values)if(value==null)throw new IOException("Resolver returned a missing address");
                    return new InetSocketAddress(values[0],port);
                }catch(TimeoutException waiting){/* Recheck original deadline and owner, never refresh. */}
                catch(ExecutionException failed){Throwable cause=failed.getCause();if(cause instanceof IOException)throw (IOException)cause;throw new IOException("Resolution failed",cause);}
                catch(CancellationException cancelled){throw new IOException("Resolution was cancelled",cancelled);}
            }
        }catch(InterruptedException interrupted){Thread.currentThread().interrupt();throw new IOException("Resolution was interrupted",interrupted);}
        finally{pending.cancel(true);}
    }
    private static InetAddress numeric(String host) throws UnknownHostException {
        String[] pieces=host.split("\\.",-1);
        if(pieces.length==4){byte[] bytes=new byte[4];boolean valid=true;
            for(int i=0;i<4;i++){String piece=pieces[i];if(piece.isEmpty()||piece.length()>3){valid=false;break;}int value=0;for(int j=0;j<piece.length();j++){char c=piece.charAt(j);if(c<'0'||c>'9'){valid=false;break;}value=value*10+c-'0';}if(!valid||value>255){valid=false;break;}bytes[i]=(byte)value;}
            if(valid)return InetAddress.getByAddress(bytes);
        }
        // Only a numeric IPv6 spelling reaches this parser. Scoped interface
        // spellings use the bounded resolver slot like other system lookups.
        String literal=host.startsWith("[")&&host.endsWith("]")?host.substring(1,host.length()-1):host;
        if(literal.indexOf(':')>=0){boolean numeric=true;for(int i=0;i<literal.length();i++){char c=literal.charAt(i);if(!((c>='0'&&c<='9')||(c>='a'&&c<='f')||(c>='A'&&c<='F')||c==':'||c=='.')){numeric=false;break;}}
            if(numeric)return InetAddress.getByName(literal);
        }
        return null;
    }
    public void close(){closed=true;}
}
