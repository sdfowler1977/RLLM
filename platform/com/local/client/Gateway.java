package com.local.client;
import android.app.*;
import android.content.Intent;
import android.os.IBinder;
import org.json.JSONObject;
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.Set;
import java.util.concurrent.*;
import javax.net.ssl.*;
import android.util.Base64;

/** Android foreground-service, socket and TLS bindings for the Rust gateway protocol. */
public final class Gateway extends Service {
    static volatile boolean running=false;
    static volatile String status="Stopped";
    private volatile ServerSocket server;
    private volatile boolean starting=false,stopping=false;
    private final Set<Socket> sockets=ConcurrentHashMap.newKeySet();
    private final ThreadPoolExecutor pool=new ThreadPoolExecutor(2,4,30,TimeUnit.SECONDS,new ArrayBlockingQueue<Runnable>(8));
    private JSONObject config;
    private final ScheduledExecutorService deadlines=Executors.newSingleThreadScheduledExecutor();
    @Override public IBinder onBind(Intent intent){return null;}
    private void notice(String message){NotificationManager manager=(NotificationManager)getSystemService(NOTIFICATION_SERVICE);manager.createNotificationChannel(new NotificationChannel("gateway","Local gateway",NotificationManager.IMPORTANCE_LOW));Intent stop=new Intent(this,Gateway.class).setAction("stop");PendingIntent action=PendingIntent.getService(this,1,stop,PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE);Notification notification=new Notification.Builder(this,"gateway").setSmallIcon(android.R.drawable.stat_notify_sync).setContentTitle("RLLM gateway").setContentText(message).setOngoing(true).addAction(new Notification.Action.Builder(null,"Stop",action).build()).build();startForeground(1,notification);}
    @Override public int onStartCommand(Intent intent,int flags,int startId){if(intent==null||"stop".equals(intent.getAction())){stopSelf();return START_NOT_STICKY;}if(running||starting){status="Already listening; stop before changing configuration";return START_NOT_STICKY;}
        try{config=new JSONObject(intent.getStringExtra("config"));notice("Starting local gateway");starting=true;Thread listener=new Thread(()->{
            try{ServerSocket socket=new ServerSocket();server=socket;socket.setReuseAddress(true);socket.bind(new InetSocketAddress(config.getString("listen"),config.getInt("port")),8);running=true;starting=false;status="Listening on "+config.getString("listen")+":"+config.getInt("port");notice(status);
                while(!socket.isClosed()){Socket client=socket.accept();client.setSoTimeout(30000);sockets.add(client);try{pool.execute(()->handle(client));}catch(RejectedExecutionException e){try{client.getOutputStream().write("HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".getBytes(StandardCharsets.US_ASCII));}catch(IOException ignored){}finally{sockets.remove(client);try{client.close();}catch(IOException ignored){}}}}
            }catch(Exception e){if(!stopping)status="Gateway stopped: "+e.getClass().getSimpleName();}finally{running=false;stopSelf();}
        },"gateway-listener");listener.start();}catch(Exception e){status="Invalid gateway configuration";stopSelf();}return START_NOT_STICKY;
    }
    private byte[] read(InputStream in,int limit) throws IOException {ByteArrayOutputStream bytes=new ByteArrayOutputStream();byte[] buffer=new byte[8192];int n;while((n=in.read(buffer))!=-1){if(bytes.size()+n>limit)throw new IOException("Response limit exceeded");bytes.write(buffer,0,n);}return bytes.toByteArray();}
    private byte[] exchange(JSONObject wire) throws Exception {Socket raw=new Socket();sockets.add(raw);ScheduledFuture<?> deadline=deadlines.schedule(()->{try{raw.close();}catch(IOException ignored){}},120,TimeUnit.SECONDS);try{raw.connect(new InetSocketAddress(wire.getString("host"),wire.getInt("port")),15000);raw.setSoTimeout(120000);Socket connection=raw;if(wire.getBoolean("tls")){SSLSocket tls=(SSLSocket)((SSLSocketFactory)SSLSocketFactory.getDefault()).createSocket(raw,wire.getString("host"),wire.getInt("port"),true);SSLParameters p=tls.getSSLParameters();p.setEndpointIdentificationAlgorithm("HTTPS");tls.setSSLParameters(p);tls.setSoTimeout(120000);tls.startHandshake();connection=tls;}try(Socket managed=connection){managed.getOutputStream().write(wire.getString("wire").getBytes(StandardCharsets.UTF_8));managed.getOutputStream().flush();return read(managed.getInputStream(),Math.min(4*1024*1024,config.optInt("limit",1024*1024)));}}finally{deadline.cancel(false);sockets.remove(raw);raw.close();}}
    private void handle(Socket client){ScheduledFuture<?> deadline=deadlines.schedule(()->{try{client.close();}catch(IOException ignored){}},150,TimeUnit.SECONDS);try{Socket owned=client;InputStream in=owned.getInputStream();ByteArrayOutputStream header=new ByteArrayOutputStream();int tail=0;while(tail!=0x0d0a0d0a){int b=in.read();if(b<0)throw new IOException("Incomplete headers");header.write(b);tail=(tail<<8)|b;if(header.size()>16384)throw new IOException("Header limit exceeded");}String rawHeader=new String(header.toByteArray(),StandardCharsets.US_ASCII);int size=((Number)Core.call(new JSONObject().put("op","gateway_size").put("header",rawHeader))).intValue();byte[] bytes=new byte[size];new DataInputStream(in).readFully(bytes);JSONObject route=(JSONObject)Core.call(new JSONObject().put("op","gateway_incoming").put("config",config).put("header",rawHeader).put("body",StandardCharsets.UTF_8.newDecoder().onMalformedInput(java.nio.charset.CodingErrorAction.REPORT).decode(java.nio.ByteBuffer.wrap(bytes)).toString()));String output;
            if(route.has("reply"))output=route.getString("reply");else{byte[] response=exchange(route.getJSONObject("upstream"));output=Core.call(new JSONObject().put("op","gateway_outgoing").put("protocol",route.getString("protocol")).put("model",route.getString("model")).put("data",Base64.encodeToString(response,Base64.NO_WRAP))).toString();}
            owned.getOutputStream().write(output.getBytes(StandardCharsets.UTF_8));owned.getOutputStream().flush();
        }catch(Exception e){try{String output=Core.call(new JSONObject().put("op","gateway_failure").put("message","Gateway request failed. Check server configuration and request format.")).toString();client.getOutputStream().write(output.getBytes(StandardCharsets.UTF_8));}catch(Exception ignored){/* Peer may already be disconnected. */}}finally{deadline.cancel(false);sockets.remove(client);try{client.close();}catch(IOException ignored){}}
    }
    @Override public void onDestroy(){stopping=true;starting=false;running=false;status="Stopped";try{if(server!=null)server.close();}catch(IOException ignored){}for(Socket s:sockets)try{s.close();}catch(IOException ignored){}deadlines.shutdownNow();pool.shutdownNow();stopForeground(true);super.onDestroy();}
}
