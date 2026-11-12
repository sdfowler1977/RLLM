"""Portable execution of production Android DNS/deadline bindings with controlled lookups."""
import json,os,pathlib,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1]
JAVA=r'''
package com.local.client;
import java.io.*;
import java.net.*;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;
final class ResolutionChecks {
 static final List<String> passed=new ArrayList<>();
 static void check(String name,boolean value){if(!value)throw new AssertionError(name);passed.add(name);}
 static long now(){return System.nanoTime()/1000000;}
 static Resolution.Budget budget(long until){return ()->Deadline.remaining(until,now());}
 static void await(CountDownLatch gate)throws IOException {try{while(!gate.await(20,TimeUnit.MILLISECONDS)){} }catch(InterruptedException interrupted){/* System DNS may retain its worker after caller cancellation. */await(gate);}}
 static InetSocketAddress eventually(Resolution resolver,String host,int port)throws Exception {
  long until=now()+2000;
  for(;;)try{return resolver.address(host,port,budget(until));}catch(IOException occupied){if(!occupied.getMessage().startsWith("Resolver is occupied")||now()>=until)throw occupied;Thread.sleep(5);}
 }
 static boolean refused(Resolution resolver,String host,Resolution.Budget budget,String text)throws Exception {
  for(;;)try{resolver.address(host,443,budget);return false;}catch(IOException failure){if(failure.getMessage().startsWith("Resolver is occupied")&&!text.equals("Resolver is occupied")){budget.remaining();Thread.sleep(5);continue;}return failure.getMessage().contains(text);}
 }
 public static void main(String[] args)throws Exception {
  ExecutorService callers=Executors.newSingleThreadExecutor();CountDownLatch release=new CountDownLatch(1),closedRelease=new CountDownLatch(1);
  try{
   AtomicInteger lookups=new AtomicInteger();Resolution numeric=new Resolution(host->{lookups.incrementAndGet();throw new UnknownHostException("Numeric path entered DNS");});
   check("numeric IPv4 retains exact address without DNS",Arrays.equals(numeric.address("127.0.0.1",8080,budget(now()+1000)).getAddress().getAddress(),new byte[]{127,0,0,1})&&lookups.get()==0);
   check("numeric IPv6 retains exact address without DNS",numeric.address("::1",8080,budget(now()+1000)).getAddress()instanceof Inet6Address&&lookups.get()==0);
   byte[] loop={127,0,0,1};Resolution normal=new Resolution(host->new InetAddress[]{InetAddress.getByAddress(loop)});
   try(ServerSocket server=new ServerSocket(0,1,InetAddress.getByAddress(loop));Socket client=new Socket()){
    client.connect(eventually(normal,"controlled.example",server.getLocalPort()),1000);
    try(Socket peer=server.accept()){peer.getOutputStream().write(new byte[]{82,76,76,77});peer.getOutputStream().flush();byte[] actual=new byte[4];new DataInputStream(client.getInputStream()).readFully(actual);check("resolved hostname connects to actual socket bytes",Arrays.equals(actual,new byte[]{82,76,76,77}));}
   }
   CountDownLatch entered=new CountDownLatch(1);AtomicInteger secondCalls=new AtomicInteger();Resolution blocked=new Resolution(host->{entered.countDown();await(release);return new InetAddress[]{InetAddress.getByAddress(loop)};});
   long started=now();Future<Boolean> expired=callers.submit(()->refused(blocked,"blocked.example",budget(started+250),"Network deadline"));if(!entered.await(1,TimeUnit.SECONDS))throw new AssertionError("Lookup did not enter");
   Resolution second=new Resolution(host->{secondCalls.incrementAndGet();return new InetAddress[]{InetAddress.getByAddress(loop)};});
   check("occupied resolver refuses an unrelated caller without queueing",refused(second,"second.example",budget(now()+1000),"Resolver is occupied")&&secondCalls.get()==0);
   check("deadline retires its caller while system lookup remains blocked",expired.get(1500,TimeUnit.MILLISECONDS)&&release.getCount()==1&&now()-started<1500);release.countDown();eventually(normal,"available.example",443);
   Resolution broken=new Resolution(host->{throw new UnknownHostException("ACTUAL_LOOKUP_FAILURE");});check("DNS failure retains the actual resolver diagnostic",refused(broken,"failed.example",budget(now()+1000),"ACTUAL_LOOKUP_FAILURE"));
   CountDownLatch closedEntered=new CountDownLatch(1);Resolution closing=new Resolution(host->{closedEntered.countDown();await(closedRelease);return new InetAddress[]{InetAddress.getByAddress(loop)};});
   Future<Boolean> closed=callers.submit(()->refused(closing,"closing.example",budget(now()+2000),"Resolver is closed"));if(!closedEntered.await(1,TimeUnit.SECONDS))throw new AssertionError("Lifecycle lookup did not enter");closing.close();check("activity or service closure retires its waiting lookup owner",closed.get(1000,TimeUnit.MILLISECONDS)&&closedRelease.getCount()==1);closedRelease.countDown();
   try(ServerSocket server=new ServerSocket(0,1,InetAddress.getByAddress(loop));Socket client=new Socket()){
    client.connect(eventually(normal,"fresh.example",server.getLocalPort()),1000);try(Socket peer=server.accept()){peer.getOutputStream().write(71);peer.getOutputStream().flush();check("released resolver slot serves a fresh actual socket after prior failures",client.getInputStream().read()==71);}
   }
   for(String name:passed)System.out.println(name);
  }finally{release.countDown();closedRelease.countDown();callers.shutdownNow();}
 }
}
'''
with tempfile.TemporaryDirectory(prefix='platform-resolution-',dir=PROJECT/'output')as tmp:
 work=pathlib.Path(tmp);source=work/'ResolutionChecks.java';source.write_text(JAVA);jdk=pathlib.Path(os.environ['CLIENT_JDK']);platform=PROJECT/'platform/com/local/client';subprocess.run([str(jdk/'bin/javac'),'--release','8','-d',str(work),str(platform/'Resolution.java'),str(platform/'Deadline.java'),str(source)],check=True,capture_output=True,text=True);run=subprocess.run([str(jdk/'bin/java'),'-cp',str(work),'com.local.client.ResolutionChecks'],check=True,capture_output=True,text=True,timeout=25);passed=run.stdout.splitlines();assert len(passed)==8,run.stdout
result=dict(passed=passed,scope='Owner portable Java execution of production Resolution/Deadline with controlled OS lookup outcomes, actual loopback sockets, occupied slot, monotonic deadline, lifecycle closure and recovery. Android platform call sites also compile against SDK35.');(PROJECT/'output/checks/platform-resolution.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))

