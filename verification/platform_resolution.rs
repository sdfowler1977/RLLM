//! Native twin of the portable Java platform-resolution checks.
mod twin;

use twin::{obj, s, Value};

const SCOPE: &str = "Owner portable Java execution of production Resolution/Deadline with controlled OS lookup outcomes, actual loopback sockets, occupied slot, monotonic deadline, lifecycle closure and recovery. Android platform call sites also compile against SDK35.";
const REPORT: &str = "output/checks/platform-resolution.json";

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let root_text = root.to_str().unwrap_or_else(|| panic!("project root is not UTF-8")).to_string();
    let scratch = Scratch::create(&root);
    let passed = run(&root_text, scratch.path());
    twin::report_value(
        "platform_resolution",
        &obj(vec![
            ("passed".into(), Value::Arr(passed.iter().map(|name| s(name)).collect())),
            ("scope".into(), s(SCOPE)),
        ]),
        true,
    );
    println!("{} platform_resolution checks passed", passed.len());
    let written = std::fs::read_to_string(REPORT).unwrap_or_else(|error| panic!("{REPORT}: {error}"));
    if !written.contains(SCOPE) || !written.contains("numeric IPv4 retains exact address without DNS") {
        panic!("{REPORT} does not contain the expected report");
    }
}

fn run(root: &str, scratch: &str) -> Vec<String> {
    let mut passed = Vec::new();
    let jdk = std::env::var("CLIENT_JDK").unwrap_or_else(|error| panic!("CLIENT_JDK: {error}"));
    let source = std::path::Path::new(scratch).join("ResolutionChecks.java");
    std::fs::write(&source, java_source()).unwrap_or_else(|error| panic!("could not write {}: {error}", source.display()));
    let platform = std::path::Path::new(root).join("platform/com/local/client");
    let javac = std::path::Path::new(&jdk).join("bin/javac");
    let java = std::path::Path::new(&jdk).join("bin/java");
    let output = std::process::Command::new(&javac)
        .arg("--release")
        .arg("8")
        .arg("-d")
        .arg(scratch)
        .arg(platform.join("Resolution.java"))
        .arg(platform.join("Deadline.java"))
        .arg(&source)
        .output()
        .unwrap_or_else(|error| panic!("could not run {}: {error}", javac.display()));
    if !output.status.success() {
        panic!("javac failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    let output = std::process::Command::new(&java)
        .arg("-cp")
        .arg(scratch)
        .arg("com.local.client.ResolutionChecks")
        .output()
        .unwrap_or_else(|error| panic!("could not run {}: {error}", java.display()));
    if !output.status.success() {
        panic!("java failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let names: Vec<String> = stdout.lines().map(str::to_string).collect();
    if names.len() != 8 {
        panic!("expected 8 platform resolution checks, got {}", names.len());
    }
    for name in &names {
        passed.push(name.clone());
    }
    passed
}

fn java_source() -> &'static str {
    r#"package com.local.client;
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
 static void await(CountDownLatch gate)throws IOException {try{while(!gate.await(20,TimeUnit.MILLISECONDS)){} }catch(InterruptedException interrupted){await(gate);}}
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
"#
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn create(root: &std::path::Path) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path = root.join("output").join(format!("platform-resolution-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap_or_else(|error| panic!("could not create {}: {error}", path.display()));
        Self(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap_or_else(|| panic!("scratch path is not UTF-8"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}