package com.local.client;

import android.content.ContentResolver;
import android.content.Context;
import android.content.Intent;
import android.content.UriPermission;
import android.database.Cursor;
import android.net.Uri;
import android.os.Build;
import android.os.CancellationSignal;
import android.os.ParcelFileDescriptor;
import android.os.Environment;
import android.os.storage.StorageManager;
import android.os.storage.StorageVolume;
import android.provider.DocumentsContract;
import org.json.JSONArray;
import org.json.JSONObject;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.ScheduledFuture;
import java.util.concurrent.TimeUnit;

/** Android document grants and source-policy binding; no provider account is required. */
final class DocumentAccess {
    private DocumentAccess() {}

    static boolean valid(Uri uri) {
        if(uri==null||!"content".equals(uri.getScheme())||uri.toString().length()>8192||uri.getFragment()!=null)return false;
        String authority=uri.getAuthority(),path=uri.getPath();
        if(authority==null||!authority.matches("[A-Za-z0-9._-]+")||path==null||path.length()<2)return false;
        String raw=uri.toString();for(int i=0;i<raw.length();i++)if(Character.isISOControl(raw.charAt(i)))return false;
        return raw.startsWith("content://"+authority+"/");
    }

    static boolean provider(Uri uri) {
        String authority=uri==null?null:uri.getAuthority();
        // These Android authorities expose local device files. Every other content
        // source uses the provider policy, including providers unknown to this app.
        return !"com.android.externalstorage.documents".equals(authority)
            &&!"com.android.providers.downloads.documents".equals(authority)
            &&!"com.android.providers.media.documents".equals(authority)
            &&!"media".equals(authority);
    }

    static boolean localStorageAuthority(String authority) {
        return "com.android.externalstorage.documents".equals(authority)
            ||"com.android.providers.downloads.documents".equals(authority)
            ||"com.android.providers.media.documents".equals(authority);
    }

    /** Device-file trees stay on the platform's own storage providers. */
    static void requireLocalTree(Uri uri)throws IOException {
        if(!valid(uri)||!DocumentsContract.isTreeUri(uri))throw new IOException("Choose a permitted device folder first");
        if(!localStorageAuthority(uri.getAuthority())){
            String authority=uri.getAuthority()==null?"unknown":uri.getAuthority();
            throw new IOException("Folder provider "+authority+" is not built-in device storage. Choose a folder from the device's local storage.");
        }
    }

    static final int MAX_PATH_CHARS=4096,MAX_PATH_PARTS=256,MAX_ENTRY_CHARS=1024,MAX_LIST_BYTES=1024*1024;
    static final int MAX_MOVE_DEPTH=64,MAX_MOVE_ENTRIES=100000,MAX_MOVE_NAME_BYTES=32*1024*1024,MAX_NAME_BYTES=255;

    static int pathParts(CharSequence path){
        int parts=1;
        for(int i=0;i<path.length();i++)if(path.charAt(i)=='/')parts++;
        return parts;
    }

    static void boundPath(String path)throws IOException {
        if(path.length()>MAX_PATH_CHARS)throw new IOException("Device path exceeds 4096 characters");
        if(pathParts(path)>MAX_PATH_PARTS)throw new IOException("Device path exceeds 256 components");
    }

    static void addListed(JSONArray rows,String name,boolean directory,int rowLimit,String rowError,int[] used)throws Exception {
        if(name==null||name.isEmpty()||name.indexOf('/')>=0||name.indexOf(0)>=0)throw new IOException("Invalid device directory entry");
        for(int i=0;i<name.length();i++)if(Character.isISOControl(name.charAt(i)))throw new IOException("Invalid device directory entry");
        if(name.length()>MAX_ENTRY_CHARS)throw new IOException("Device folder entry name exceeds 1024 characters");
        byte[] encoded=name.getBytes(StandardCharsets.UTF_8);
        if(encoded.length>MAX_ENTRY_CHARS)throw new IOException("Device folder entry name exceeds 1024 bytes");
        if(rows.length()>=rowLimit)throw new IOException(rowError);
        JSONObject row=new JSONObject().put("name",name).put("directory",directory);
        int addition=row.toString().getBytes(StandardCharsets.UTF_8).length+1;
        if(used[0]>MAX_LIST_BYTES-addition)throw new IOException("Device folder listing exceeds 1 MiB");
        rows.put(row);
        used[0]+=addition;
    }

    static void check(Uri uri,boolean phone,boolean providers,boolean selected)throws IOException {
        if(!valid(uri))throw new IOException("Select a valid document from the device file locations");
        if((selected||provider(uri))&&!providers)throw new IOException("Document provider access is off in Setup");
        if(!provider(uri)&&!phone)throw new IOException("Device storage access is off in Setup");
    }

    interface Check { void run() throws IOException; }

    /** Rollback and discard finish after the caller has already cancelled the forward move. */
    private static final Check ROLLBACK=()->{};

    static boolean directGranted(Context context) {
        if(Build.VERSION.SDK_INT>=30)return Environment.isExternalStorageManager();
        boolean permissions=context.checkSelfPermission(android.Manifest.permission.READ_EXTERNAL_STORAGE)==android.content.pm.PackageManager.PERMISSION_GRANTED
            &&context.checkSelfPermission(android.Manifest.permission.WRITE_EXTERNAL_STORAGE)==android.content.pm.PackageManager.PERMISSION_GRANTED;
        // Android 10 applies scoped storage unless legacy storage is actually in effect.
        if(Build.VERSION.SDK_INT==29)return Environment.isExternalStorageLegacy()&&permissions;
        return permissions;
    }

    static File primary() { return Environment.getExternalStorageDirectory(); }

    static List<File> volumes(Context context)throws IOException {
        List<File> roots=new ArrayList<>();
        File primary=primary().getCanonicalFile();
        roots.add(primary);
        if(Build.VERSION.SDK_INT>=30) {
            StorageManager manager=context.getSystemService(StorageManager.class);
            if(manager==null)return roots;
            for(StorageVolume volume:manager.getStorageVolumes()) {
                File directory=volume.getDirectory();
                if(directory!=null&&!roots.contains(directory.getCanonicalFile()))roots.add(directory.getCanonicalFile());
            }
        }else {
            File[] apps=context.getExternalFilesDirs(null);
            if(apps==null)return roots;
            for(File app:apps) {
                if(app==null)continue;
                File parent=app.getParentFile();
                File data=parent==null?null:parent.getParentFile();
                File android=data==null?null:data.getParentFile();
                if(android==null||!"Android".equals(android.getName())||android.getParentFile()==null)continue;
                File directory=android.getParentFile().getCanonicalFile();
                if(!roots.contains(directory))roots.add(directory);
            }
        }
        return roots;
    }

    static boolean contains(File folder,File path) {
        String prefix=folder.getPath();
        return path.equals(folder)||path.getPath().startsWith(prefix.endsWith("/")?prefix:prefix+"/");
    }

    // O_DIRECTORY is 0200000 on Android. This compile stub does not declare the constant.
    private static final int OPEN_DIRECTORY=0200000;

    static final class Relocation {
        final String kind;
        final File placed;
        final File retained;
        Relocation(String kind,File placed,File retained){this.kind=kind;this.placed=placed;this.retained=retained;}
    }

    // Each directory is opened with O_DIRECTORY|O_NOFOLLOW. The next component is
    // opened through /proc/self/fd/<fd>/<name>, which is this runtime's openat.
    private static final class HeldDirectory implements Closeable {
        private final ParcelFileDescriptor parcel;
        private boolean closed;
        private HeldDirectory(FileDescriptor opened)throws IOException {
            try{parcel=ParcelFileDescriptor.dup(opened);}
            catch(IOException problem){
                try{android.system.Os.close(opened);}catch(android.system.ErrnoException ignored){}
                throw problem;
            }
            try{android.system.Os.close(opened);}
            catch(android.system.ErrnoException problem){
                try{parcel.close();}catch(IOException ignored){}
                throw new IOException(problem.getMessage(),problem);
            }
        }
        int number(){return parcel.getFd();}
        FileDescriptor descriptor(){return parcel.getFileDescriptor();}
        String childPath(String name){return "/proc/self/fd/"+number()+"/"+name;}
        public void close()throws IOException {if(closed)return;closed=true;parcel.close();}
        private void quiet(){try{close();}catch(IOException ignored){}}
    }

    private static void leafName(String name)throws IOException {
        if(name.isEmpty()||name.equals(".")||name.equals("..")||name.indexOf('/')>=0||name.indexOf(0)>=0)
            throw new IOException("Enter a path inside the selected device folder");
        for(int i=0;i<name.length();i++)if(Character.isISOControl(name.charAt(i)))
            throw new IOException("Enter a path inside the selected device folder");
    }

    private static HeldDirectory openDirectory(File directory)throws IOException {
        String absolute=directory.getAbsolutePath();
        // The walked path includes the volume prefix already counted against the user-path cap.
        if(absolute.length()>8192)throw new IOException("Device path exceeds 8192 characters");
        if(pathParts(absolute)>512)throw new IOException("Device path exceeds 512 components");
        if(!absolute.startsWith("/"))throw new IOException("Enter an absolute device path");
        FileDescriptor root;
        try{
            root=android.system.Os.open("/",android.system.OsConstants.O_RDONLY|OPEN_DIRECTORY
                |android.system.OsConstants.O_NOFOLLOW|android.system.OsConstants.O_CLOEXEC,0);
        }catch(android.system.ErrnoException problem){
            throw new IOException("Device path is unavailable: /. "+problem.getMessage(),problem);
        }
        HeldDirectory current=new HeldDirectory(root);
        try{
            StringBuilder walked=new StringBuilder();
            for(String part:absolute.split("/")){
                if(part.isEmpty())continue;
                leafName(part);
                walked.append('/').append(part);
                FileDescriptor next;
                try{
                    next=android.system.Os.open(current.childPath(part),android.system.OsConstants.O_RDONLY
                        |OPEN_DIRECTORY|android.system.OsConstants.O_NOFOLLOW|android.system.OsConstants.O_CLOEXEC,0);
                }catch(android.system.ErrnoException problem){
                    if(problem.errno==android.system.OsConstants.ELOOP)throw new IOException("Device path is a symbolic link: "+walked);
                    if(problem.errno==android.system.OsConstants.ENOTDIR)throw new IOException("Device path is not a directory: "+walked);
                    throw new IOException("Device path is unavailable: "+walked+". "+problem.getMessage(),problem);
                }
                HeldDirectory child=new HeldDirectory(next);
                try{
                    android.system.StructStat stat=android.system.Os.fstat(child.descriptor());
                    if((stat.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFDIR)
                        throw new IOException("Device path is not a directory: "+walked);
                }catch(android.system.ErrnoException problem){
                    child.quiet();
                    throw new IOException("Device path is unavailable: "+walked+". "+problem.getMessage(),problem);
                }catch(IOException problem){
                    child.quiet();
                    throw problem;
                }
                HeldDirectory previous=current;
                current=child;
                previous.quiet();
            }
            HeldDirectory opened=current;
            current=null;
            return opened;
        }finally{if(current!=null)current.quiet();}
    }

    private static FileDescriptor openRelative(HeldDirectory directory,String name,int flags,int mode)throws IOException,android.system.ErrnoException {
        leafName(name);
        return android.system.Os.open(directory.childPath(name),flags|android.system.OsConstants.O_NOFOLLOW|android.system.OsConstants.O_CLOEXEC,mode);
    }

    private static void bind(File file)throws IOException {
        File parent=file.getParentFile();
        if(parent==null){openDirectory(file).close();return;}
        leafName(file.getName());
        HeldDirectory directory=openDirectory(parent);
        try{
            try{
                android.system.StructStat stat=android.system.Os.lstat(directory.childPath(file.getName()));
                if((stat.st_mode&android.system.OsConstants.S_IFMT)==android.system.OsConstants.S_IFLNK)
                    throw new IOException("Device path is a symbolic link: "+file);
            }catch(android.system.ErrnoException problem){
                if(problem.errno!=android.system.OsConstants.ENOENT)
                    throw new IOException("Device path is unavailable: "+file+". "+problem.getMessage(),problem);
            }
        }finally{directory.quiet();}
    }

    private static File lexicalVolume(Context context,File path)throws IOException {
        File best=null;
        for(File volume:volumes(context))
            if(contains(volume,path)&&(best==null||volume.getPath().length()>best.getPath().length()))best=volume;
        return best;
    }

    static File absolute(String path)throws IOException {
        boundPath(path);
        if(!path.startsWith("/")||path.indexOf(0)>=0)throw new IOException("Enter an absolute device path");
        for(int i=0;i<path.length();i++)if(Character.isISOControl(path.charAt(i)))throw new IOException("Enter an absolute device path");
        for(String part:path.split("/"))if(part.equals(".."))throw new IOException("Parent traversal is unsupported");
        return new File(path).toPath().normalize().toFile();
    }

    static File sharedVolume(Context context,File path)throws IOException {
        File canonical=path.getCanonicalFile();
        for(File volume:volumes(context))if(contains(volume,canonical))return volume;
        return null;
    }

    /** User path under a shared volume. Every component is opened with O_NOFOLLOW; a symbolic link is refused. */
    static File sharedFile(Context context,String path)throws IOException {
        File requested=path.isEmpty()?primary():absolute(path);
        if(lexicalVolume(context,requested)==null)throw new IOException("Choose a path on a shared device volume");
        bind(requested);
        return requested;
    }

    static String reviewLeaf(String name,long stamp,int counter){
        String suffix=counter==0?"-"+stamp:"-"+stamp+"-"+counter;
        byte[] suffixBytes=suffix.getBytes(StandardCharsets.UTF_8);
        int budget=MAX_NAME_BYTES-suffixBytes.length;
        if(budget<1)budget=1;
        String extension="";
        int dot=name.lastIndexOf('.');
        if(dot>0&&dot<name.length()-1){
            String candidate=name.substring(dot);
            int extensionBytes=candidate.getBytes(StandardCharsets.UTF_8).length;
            if(extensionBytes<=32&&extensionBytes<budget)extension=candidate;
        }
        int extensionBytes=extension.getBytes(StandardCharsets.UTF_8).length;
        String stem=extension.isEmpty()?name:name.substring(0,name.length()-extension.length());
        String trimmed=trimUtf8(stem,budget-extensionBytes);
        if(trimmed.isEmpty()||trimmed.equals(".")||trimmed.equals(".."))trimmed=trimUtf8("file",budget-extensionBytes);
        String leaf=trimmed+extension+suffix;
        if(leaf.getBytes(StandardCharsets.UTF_8).length>MAX_NAME_BYTES)leaf=trimUtf8(trimmed,budget)+suffix;
        return leaf;
    }

    private static String trimUtf8(String value,int maxBytes){
        if(value==null||maxBytes<=0)return "";
        byte[] bytes=value.getBytes(StandardCharsets.UTF_8);
        if(bytes.length<=maxBytes)return value;
        int end=maxBytes;
        while(end>0&&(bytes[end]&0xC0)==0x80)end--;
        return end<=0?"":new String(bytes,0,end,StandardCharsets.UTF_8);
    }

    static File within(File folder,String path)throws IOException {
        boundPath(path);
        if(path.startsWith("/")||path.indexOf(0)>=0)throw new IOException("Enter a path inside the selected device folder");
        for(int i=0;i<path.length();i++)if(Character.isISOControl(path.charAt(i)))throw new IOException("Enter a path inside the selected device folder");
        for(String part:path.split("/"))if(part.equals(".."))throw new IOException("Path leaves the selected device folder");
        File original=path.isEmpty()?folder:new File(folder,path).toPath().normalize().toFile();
        if(!contains(folder,original))throw new IOException("Path leaves the selected device folder");
        if(path.isEmpty()){openDirectory(folder).close();return folder;}
        bind(original);
        return original;
    }

    static File directTree(Context context,String tree)throws IOException {
        if(!directGranted(context)||tree.isEmpty())return null;
        if(tree.startsWith("/")) {
            File folder=absolute(tree);
            if(lexicalVolume(context,folder)==null)throw new IOException("Selected folder is outside shared device storage");
            openDirectory(folder).close();
            return folder;
        }
        Uri uri=Uri.parse(tree);
        if(!"com.android.externalstorage.documents".equals(uri.getAuthority())||!DocumentsContract.isTreeUri(uri))return null;
        String id=DocumentsContract.getTreeDocumentId(uri);
        int split=id.indexOf(':');
        if(split<=0)return null;
        String key=id.substring(0,split);
        if(key.equalsIgnoreCase("primary"))return within(primary(),id.substring(split+1));
        for(File volume:volumes(context))if(volume.getName().equalsIgnoreCase(key))
            return within(volume,id.substring(split+1));
        return null;
    }

    static JSONObject metadata(File file)throws Exception {
        File parent=file.getParentFile();
        if(parent==null){
            try{return described(file.getPath(),android.system.Os.lstat(file.getPath()));}
            catch(android.system.ErrnoException problem){throw new IOException("Device path is unavailable: "+file+". "+problem.getMessage(),problem);}
        }
        leafName(file.getName());
        HeldDirectory directory=openDirectory(parent);
        try{
            try{return described(file.getName(),android.system.Os.lstat(directory.childPath(file.getName())));}
            catch(android.system.ErrnoException problem){
                if(problem.errno==android.system.OsConstants.ELOOP)throw new IOException("Device path is a symbolic link: "+file);
                throw new IOException("Device path is unavailable: "+file+". "+problem.getMessage(),problem);
            }
        }finally{directory.quiet();}
    }

    private static JSONObject described(String name,android.system.StructStat stat)throws Exception {
        int format=stat.st_mode&android.system.OsConstants.S_IFMT;
        String kind=format==android.system.OsConstants.S_IFDIR?"directory":format==android.system.OsConstants.S_IFREG?"file":format==android.system.OsConstants.S_IFLNK?"link":"other";
        return new JSONObject().put("name",name).put("kind",kind).put("bytes",stat.st_size).put("modified",stat.st_mtime);
    }

    static void createDirectory(File file)throws IOException {
        File parent=file.getParentFile();
        if(parent==null)throw new IOException("Choose a folder inside a device folder");
        leafName(file.getName());
        HeldDirectory directory=openDirectory(parent);
        try{
            try{android.system.Os.mkdir(directory.childPath(file.getName()),0700);}
            catch(android.system.ErrnoException problem){
                if(problem.errno==android.system.OsConstants.EEXIST)throw new IOException("Device destination already exists");
                if(problem.errno==android.system.OsConstants.ELOOP)throw new IOException("Device path is a symbolic link: "+file);
                throw new IOException("Device folder could not be created: "+file+". "+problem.getMessage(),problem);
            }
            android.system.StructStat stat=android.system.Os.lstat(directory.childPath(file.getName()));
            if((stat.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFDIR)
                throw new IOException("Device destination is not a directory: "+file);
        }catch(android.system.ErrnoException problem){
            throw new IOException("Device folder could not be created: "+file+". "+problem.getMessage(),problem);
        }finally{directory.quiet();}
    }

    static boolean exists(File file)throws IOException {
        File parent=file.getParentFile();
        if(parent==null)return false;
        leafName(file.getName());
        HeldDirectory directory=openDirectory(parent);
        try{
            try{android.system.Os.lstat(directory.childPath(file.getName()));return true;}
            catch(android.system.ErrnoException problem){
                if(problem.errno==android.system.OsConstants.ENOENT)return false;
                throw new IOException("Device path is unavailable: "+file+". "+problem.getMessage(),problem);
            }
        }finally{directory.quiet();}
    }

    private static FileDescriptor openRegular(File file,int flags)throws IOException {
        File parent=file.getParentFile();
        if(parent==null)throw new IOException("Choose a file inside a device folder");
        leafName(file.getName());
        HeldDirectory directory=openDirectory(parent);
        FileDescriptor fd=null;
        try{
            fd=openRelative(directory,file.getName(),flags,0);
            android.system.StructStat stat=android.system.Os.fstat(fd);
            if((stat.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFREG){
                android.system.Os.close(fd);fd=null;
                throw new IOException("Device path is not a regular file: "+file);
            }
            FileDescriptor opened=fd;fd=null;return opened;
        }catch(android.system.ErrnoException problem){
            if(fd!=null)try{android.system.Os.close(fd);}catch(android.system.ErrnoException ignored){}
            if(problem.errno==android.system.OsConstants.ELOOP)throw new IOException("Device path is a symbolic link: "+file);
            throw new IOException("Device file is unavailable: "+file+". "+problem.getMessage(),problem);
        }finally{directory.quiet();}
    }

    static FileDescriptor openReadWrite(File file)throws IOException {
        return openRegular(file,android.system.OsConstants.O_RDWR);
    }

    static InputStream regularInput(File file)throws IOException {
        return new DeviceFileInput(openRegular(file,android.system.OsConstants.O_RDONLY|android.system.OsConstants.O_NONBLOCK));
    }

    // The replacement is a new file in the same directory. The original name is exchanged
    // only after that file is synced. A failed write removes the new file and leaves the original.
    static void replace(File file,InputStream input,Check check)throws IOException {
        File parent=file.getParentFile();
        if(parent==null)throw new IOException("Choose a file inside a device folder");
        String name=file.getName();
        leafName(name);
        check.run();
        HeldDirectory directory=openDirectory(parent);
        String sibling=null;
        long siblingDev=0,siblingIno=0;
        boolean haveSibling=false,published=false;
        Exception failure=null;
        IOException cleanup=null;
        try{
            android.system.StructStat existing;
            try{existing=android.system.Os.lstat(directory.childPath(name));}
            catch(android.system.ErrnoException problem){throw new IOException("Device file is unavailable: "+file+". "+problem.getMessage(),problem);}
            int format=existing.st_mode&android.system.OsConstants.S_IFMT;
            if(format==android.system.OsConstants.S_IFLNK)throw new IOException("Device path is a symbolic link: "+file);
            if(format!=android.system.OsConstants.S_IFREG)throw new IOException("Device path is not a regular file: "+file);
            FileDescriptor fd=null;
            for(int attempt=0;attempt<8&&sibling==null;attempt++){
                String candidate="w"+android.system.Os.getpid()+"-"+System.nanoTime();
                if(candidate.equals(name)||candidate.getBytes(StandardCharsets.UTF_8).length>MAX_NAME_BYTES)continue;
                try{fd=openRelative(directory,candidate,android.system.OsConstants.O_WRONLY|android.system.OsConstants.O_CREAT|android.system.OsConstants.O_EXCL,0600);sibling=candidate;}
                catch(android.system.ErrnoException problem){
                    if(problem.errno==android.system.OsConstants.EEXIST||problem.errno==android.system.OsConstants.ELOOP)continue;
                    throw new IOException("Device file could not be written beside the original. "+problem.getMessage(),problem);
                }
            }
            if(fd==null||sibling==null)throw new IOException("Device file could not be written beside the original");
            try{
                android.system.StructStat created=android.system.Os.fstat(fd);
                siblingDev=created.st_dev;siblingIno=created.st_ino;haveSibling=true;
                try{android.system.Os.fchmod(fd,existing.st_mode&0777);}catch(android.system.ErrnoException ignored){}
                try(OutputStream out=new DeviceFileOutput(fd)){
                    fd=null;
                    transfer(input,out,check);
                }
            }finally{if(fd!=null)try{android.system.Os.close(fd);}catch(android.system.ErrnoException ignored){}}
            android.system.StructStat linked=android.system.Os.lstat(directory.childPath(sibling));
            if(linked.st_dev!=siblingDev||linked.st_ino!=siblingIno||(linked.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFREG)
                throw new IOException("Device file could not be written beside the original");
            android.system.StructStat current=android.system.Os.lstat(directory.childPath(name));
            int currentFormat=current.st_mode&android.system.OsConstants.S_IFMT;
            if(currentFormat==android.system.OsConstants.S_IFLNK)throw new IOException("Device path is a symbolic link: "+file);
            if(currentFormat!=android.system.OsConstants.S_IFREG)throw new IOException("Device path is not a regular file: "+file);
            if(current.st_dev!=existing.st_dev||current.st_ino!=existing.st_ino)throw new IOException("Device destination changed during the write; it was not overwritten");
            android.system.Os.rename(directory.childPath(sibling),directory.childPath(name));
            sibling=null;published=true;
        }catch(Exception problem){failure=problem;}
        finally{
            if(!published&&sibling!=null){
                try{
                    if(!haveSibling){
                        android.system.StructStat leftover=android.system.Os.lstat(directory.childPath(sibling));
                        if((leftover.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFREG)
                            throw new IOException("Device file could not be written beside the original");
                        siblingDev=leftover.st_dev;siblingIno=leftover.st_ino;
                    }
                    releaseMatched(directory,sibling,siblingDev,siblingIno);
                }catch(Exception problem){cleanup=problem instanceof IOException?(IOException)problem:new IOException(problem.getMessage(),problem);}
            }
            directory.quiet();
        }
        if(failure==null){if(cleanup!=null)throw cleanup;return;}
        if(!haveSibling){
            if(cleanup!=null)failure.addSuppressed(cleanup);
            if(failure instanceof IOException)throw (IOException)failure;
            if(failure instanceof RuntimeException)throw (RuntimeException)failure;
            throw new IOException(failure.getMessage(),failure);
        }
        if(failure instanceof RuntimeException){if(cleanup!=null)failure.addSuppressed(cleanup);throw (RuntimeException)failure;}
        IOException problem=failure instanceof IOException?(IOException)failure:new IOException(failure.getMessage(),failure);
        if(replaceRefusal(problem)){if(cleanup!=null)problem.addSuppressed(cleanup);throw problem;}
        IOException reported=new IOException(leftUnchanged(problem)+(cleanup==null?"":" A temporary file may remain."),problem);
        if(cleanup!=null)reported.addSuppressed(cleanup);
        throw reported;
    }

    private static boolean replaceRefusal(IOException problem){
        String message=problem.getMessage();
        return message!=null&&(message.startsWith("Device path is a symbolic link")||message.startsWith("Device path is not a regular file")||message.startsWith("Device file is unavailable")||message.startsWith("Choose a file inside a device folder"));
    }

    private static String leftUnchanged(IOException problem){
        String message=problem.getMessage();
        if(message==null||message.isEmpty())message="Device write could not finish";
        if(message.contains("left unchanged")||message.contains("may remain"))return message;
        return message+" The original was left unchanged.";
    }

    private static final class DeviceFileInput extends InputStream {
        private FileDescriptor fd;
        DeviceFileInput(FileDescriptor fd){this.fd=fd;}
        public int read()throws IOException {byte[] one=new byte[1];int n=read(one,0,1);return n<0?-1:one[0]&255;}
        public int read(byte[] buffer,int offset,int length)throws IOException {
            if(fd==null)throw new IOException("Device file is closed");
            if(buffer==null)throw new NullPointerException();
            if(offset<0||length<0||offset>buffer.length||length>buffer.length-offset)throw new IndexOutOfBoundsException();
            if(length==0)return 0;
            try{int n=android.system.Os.read(fd,buffer,offset,length);return n==0?-1:n;}
            catch(android.system.ErrnoException|InterruptedIOException problem){throw new IOException(problem.getMessage(),problem);}
        }
        public void close()throws IOException {
            FileDescriptor current=fd;fd=null;
            if(current!=null)try{android.system.Os.close(current);}catch(android.system.ErrnoException problem){throw new IOException(problem.getMessage(),problem);}
        }
    }

    private static final class DeviceFileOutput extends OutputStream {
        private FileDescriptor fd;
        DeviceFileOutput(FileDescriptor fd){this.fd=fd;}
        public void write(int value)throws IOException {write(new byte[]{(byte)value},0,1);}
        public void write(byte[] buffer,int offset,int length)throws IOException {
            if(fd==null)throw new IOException("Device file is closed");
            if(buffer==null)throw new NullPointerException();
            if(offset<0||length<0||offset>buffer.length||length>buffer.length-offset)throw new IndexOutOfBoundsException();
            int at=offset,end=offset+length;
            while(at<end){
                try{
                    int n=android.system.Os.write(fd,buffer,at,end-at);
                    if(n<=0)throw new IOException("Device write stopped before all bytes were written");
                    at+=n;
                }catch(android.system.ErrnoException|InterruptedIOException problem){throw new IOException(problem.getMessage(),problem);}
            }
        }
        public void close()throws IOException {
            FileDescriptor current=fd;if(current==null)return;fd=null;
            android.system.ErrnoException sync=null;
            try{android.system.Os.fsync(current);}catch(android.system.ErrnoException problem){sync=problem;}
            try{android.system.Os.close(current);}catch(android.system.ErrnoException problem){if(sync==null)sync=problem;}
            if(sync!=null)throw new IOException(sync.getMessage(),sync);
        }
    }

    private static final class MoveWalk {
        private final Check check;
        private int entries,bytes;
        MoveWalk(Check check){this.check=check;}
        void entry(String name)throws IOException {
            check.run();
            if(name!=null){
                int size=name.getBytes(StandardCharsets.UTF_8).length;
                if(name.length()>MAX_ENTRY_CHARS||size>MAX_ENTRY_CHARS)throw new IOException("Device folder entry name exceeds 1024 characters");
                if(bytes>MAX_MOVE_NAME_BYTES-size)throw new IOException("Device directory move exceeds the name budget");
                bytes+=size;
            }
            if(++entries>MAX_MOVE_ENTRIES)throw new IOException("Device directory move exceeds 100000 entries");
        }
        void depth(int depth)throws IOException {
            if(depth>MAX_MOVE_DEPTH)throw new IOException("Device directory move exceeds 64 nested directories");
        }
    }

    private static final class CopiedFile {
        final long sourceDev,sourceIno,destDev,destIno;
        CopiedFile(long sourceDev,long sourceIno,long destDev,long destIno){
            this.sourceDev=sourceDev;this.sourceIno=sourceIno;this.destDev=destDev;this.destIno=destIno;
        }
    }

    private static String normalizedDevicePath(File file)throws IOException {
        String absolute=file.getAbsolutePath();
        ArrayList<String> parts=new ArrayList<>();
        for(String part:absolute.split("/",-1)){
            if(part.isEmpty()||part.equals("."))continue;
            if(part.equals("..")){
                if(parts.isEmpty())throw new IOException("Device path is unavailable: "+file);
                parts.remove(parts.size()-1);
                continue;
            }
            parts.add(part);
        }
        if(parts.isEmpty())return "/";
        StringBuilder path=new StringBuilder();
        for(String part:parts)path.append('/').append(part);
        return path.toString();
    }

    private static boolean pathInside(String source,String destination){
        if(source==null||destination==null||source.isEmpty()||destination.isEmpty())return false;
        if(destination.equals(source))return true;
        String prefix=source.endsWith("/")?source:source+"/";
        return destination.startsWith(prefix);
    }

    private static void refuseCopyDestination(File source,File destination)throws IOException {
        if(pathInside(normalizedDevicePath(source),normalizedDevicePath(destination)))throw new IOException("Choose a destination outside the source");
        String sourceCanonical,destinationCanonical;
        try{sourceCanonical=source.getCanonicalPath();destinationCanonical=destination.getCanonicalPath();}
        catch(IOException problem){throw new IOException("Device copy was refused because the destination could not be confirmed outside the source",problem);}
        if(pathInside(sourceCanonical,destinationCanonical))throw new IOException("Choose a destination outside the source");
    }

    static void copyTree(File source,File destination,Check check)throws IOException {
        check.run();
        refuseCopyDestination(source,destination);
        File sourceParent=source.getParentFile(),destinationParent=destination.getParentFile();
        if(sourceParent==null||destinationParent==null)throw new IOException("Choose a file or subfolder");
        leafName(source.getName());leafName(destination.getName());
        HeldDirectory sourceDirectory=openDirectory(sourceParent);
        try{
            HeldDirectory destinationDirectory=openDirectory(destinationParent);
            try{
                android.system.StructStat stat=lstatRelative(sourceDirectory,source);
                int format=stat.st_mode&android.system.OsConstants.S_IFMT;
                if(format==android.system.OsConstants.S_IFLNK)throw new IOException("Device path is a symbolic link: "+source);
                MoveWalk walk=new MoveWalk(check);
                if(format==android.system.OsConstants.S_IFDIR){
                    try{copyDirectory(sourceDirectory,source.getName(),destinationDirectory,destination.getName(),walk,1);}
                    catch(StackOverflowError overflow){throw new IOException("Device directory copy exceeds 64 nested directories",overflow);}
                    return;
                }
                if(format!=android.system.OsConstants.S_IFREG)throw new IOException("Device path is not a regular file: "+source);
                CopiedFile copied=copyExclusive(sourceDirectory,source.getName(),source.getPath(),destinationDirectory,destination.getName(),check);
                try{verifyClaimed(sourceDirectory,source.getName(),destinationDirectory,destination.getName(),copied,check);}
                catch(IOException problem){
                    discardIfMatch(destinationDirectory,destination.getName(),copied.destDev,copied.destIno);
                    throw problem;
                }
            }finally{destinationDirectory.quiet();}
        }finally{sourceDirectory.quiet();}
    }

    private static void copyDirectory(HeldDirectory sourceDirectory,String sourceName,HeldDirectory destinationDirectory,String destinationName,MoveWalk walk,int depth)throws IOException {
        walk.depth(depth);
        String destPath=destinationDirectory.childPath(destinationName);
        try{android.system.Os.mkdir(destPath,0700);}
        catch(android.system.ErrnoException problem){
            if(problem.errno==android.system.OsConstants.EEXIST)throw new IOException("Device destination already exists");
            throw new IOException("Device copy could not finish: "+destinationName+". "+problem.getMessage(),problem);
        }
        HeldDirectory from=null,to=null;
        try{
            from=holdDirectory(openRelative(sourceDirectory,sourceName,android.system.OsConstants.O_RDONLY|OPEN_DIRECTORY,0));
            to=holdDirectory(openRelative(destinationDirectory,destinationName,android.system.OsConstants.O_RDONLY|OPEN_DIRECTORY,0));
            android.system.StructStat opened=android.system.Os.fstat(from.descriptor());
            if((opened.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFDIR)
                throw new IOException("Device path is not a directory: "+sourceName);
            walk.entry(null);
            try(java.nio.file.DirectoryStream<java.nio.file.Path> entries=java.nio.file.Files.newDirectoryStream(java.nio.file.Paths.get("/proc/self/fd/"+from.number()))){
                for(java.nio.file.Path entry:entries){
                    String child=entry.getFileName().toString();
                    if(child.equals(".")||child.equals(".."))continue;
                    walk.entry(child);
                    copyChild(from,to,child,walk,depth);
                }
            }
        }catch(android.system.ErrnoException problem){
            throw new IOException("Device copy could not finish: "+destinationName+". An incomplete destination may remain. "+problem.getMessage(),problem);
        }catch(IOException problem){
            if(problem.getMessage()!=null&&problem.getMessage().contains("may remain"))throw problem;
            throw new IOException("Device copy could not finish: "+destinationName+". An incomplete destination may remain. "+problem.getMessage(),problem);
        }finally{if(from!=null)from.quiet();if(to!=null)to.quiet();}
    }

    private static void copyChild(HeldDirectory source,HeldDirectory destination,String name,MoveWalk walk,int depth)throws IOException {
        android.system.StructStat stat;
        try{stat=android.system.Os.lstat(source.childPath(name));}
        catch(android.system.ErrnoException problem){throw new IOException("Device path is unavailable: "+name+". "+problem.getMessage(),problem);}
        int format=stat.st_mode&android.system.OsConstants.S_IFMT;
        if(format==android.system.OsConstants.S_IFDIR){copyDirectory(source,name,destination,name,walk,depth+1);return;}
        if(format==android.system.OsConstants.S_IFLNK)throw new IOException("Device path is a symbolic link: "+name);
        if(format!=android.system.OsConstants.S_IFREG)throw new IOException("Device path is not a regular file: "+name);
        CopiedFile copied=copyExclusive(source,name,name,destination,name,walk.check);
        try{verifyClaimed(source,name,destination,name,copied,walk.check);}
        catch(IOException problem){discardIfMatch(destination,name,copied.destDev,copied.destIno);throw problem;}
    }

    static Relocation moveExclusive(File source,File destination,Check check)throws IOException {
        check.run();
        if(source.equals(destination))throw new IOException("Choose a destination outside the source");
        File sourceParent=source.getParentFile(),destinationParent=destination.getParentFile();
        if(sourceParent==null||destinationParent==null)throw new IOException("Choose a file or subfolder");
        leafName(source.getName());leafName(destination.getName());
        HeldDirectory sourceDirectory=openDirectory(sourceParent);
        try{
            HeldDirectory destinationDirectory=openDirectory(destinationParent);
            try{
                android.system.StructStat sourceStat=lstatRelative(sourceDirectory,source);
                int format=sourceStat.st_mode&android.system.OsConstants.S_IFMT;
                if(format==android.system.OsConstants.S_IFLNK)throw new IOException("Device path is a symbolic link: "+source);
                android.system.StructStat parentStat=android.system.Os.fstat(destinationDirectory.descriptor());
                boolean sameVolume=sourceStat.st_dev==parentStat.st_dev;
                if(format==android.system.OsConstants.S_IFDIR){
                    if(!sameVolume)throw new IOException("Directory copy across volumes is not available; nothing was moved");
                    String from=sourceDirectory.childPath(source.getName()),to=destinationDirectory.childPath(destination.getName());
                    if(renameNoReplace(from,to)==0){
                        android.system.StructStat destStat=android.system.Os.lstat(to);
                        if((destStat.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFDIR)
                            throw new IOException("Device destination is not a directory: "+destination);
                        try{
                            android.system.Os.lstat(from);
                            throw new IOException("Device rename left the original directory at "+source);
                        }catch(android.system.ErrnoException gone){
                            if(gone.errno!=android.system.OsConstants.ENOENT)
                                throw new IOException("Device path is unavailable: "+source+". "+gone.getMessage(),gone);
                        }
                        return new Relocation("moved by rename",destination,null);
                    }
                    try{claimDirectory(sourceDirectory,source,destinationDirectory,destination,check);}
                    catch(StackOverflowError overflow){throw new IOException("Device directory move exceeds 64 nested directories",overflow);}
                    return new Relocation("moved by directory claim",destination,null);
                }
                if(format!=android.system.OsConstants.S_IFREG)throw new IOException("Device path is not a regular file: "+source);
                if(sameVolume){
                    String kind=relocateFile(sourceDirectory,source.getName(),source.getPath(),destinationDirectory,destination.getName(),check);
                    return new Relocation(kind,destination,null);
                }
                CopiedFile copied=copyExclusive(sourceDirectory,source.getName(),source.getPath(),destinationDirectory,destination.getName(),check);
                try{verifyClaimed(sourceDirectory,source.getName(),destinationDirectory,destination.getName(),copied,check);}
                catch(IOException problem){
                    discardIfMatch(destinationDirectory,destination.getName(),copied.destDev,copied.destIno);
                    throw problem;
                }
            }catch(android.system.ErrnoException problem){
                throw new IOException("Device file is unavailable: "+source+". "+problem.getMessage(),problem);
            }finally{destinationDirectory.quiet();}
        }finally{sourceDirectory.quiet();}
        try{return new Relocation("copied across volumes",destination,retainSource(source,check));}
        catch(IOException problem){
            throw new IOException("Copied across volumes to "+destination+" but the original remains at "+source+". "+problem.getMessage(),problem);
        }
    }

    private static android.system.StructStat lstatRelative(HeldDirectory directory,File file)throws IOException {
        try{return android.system.Os.lstat(directory.childPath(file.getName()));}
        catch(android.system.ErrnoException problem){
            throw new IOException("Device file is unavailable: "+file+". "+problem.getMessage(),problem);
        }
    }

    // Os.rename replaces an existing name. renameat2(RENAME_NOREPLACE) is one syscall that
    // refuses an existing name. Where that call is unavailable, link refuses the name, and a
    // regular file is copied into an O_EXCL name instead. Neither fallback is a single rename.
    private static final int AT_FDCWD=-100,RENAME_NOREPLACE=1;

    private static int renameNoReplace(String from,String to)throws IOException {
        try{invokeRename(android.system.Os.class,null,from,to);return 0;}
        catch(java.lang.reflect.InvocationTargetException problem){return translateRename(problem,to);}
        catch(ReflectiveOperationException|NoSuchMethodError|SecurityException unsupported){}
        try{
            Class<?> libcore=Class.forName("libcore.io.Libcore");
            Object os=libcore.getField("os").get(null);
            invokeRename(os.getClass(),os,from,to);
            return 0;
        }catch(java.lang.reflect.InvocationTargetException problem){return translateRename(problem,to);}
        catch(ReflectiveOperationException|NoSuchMethodError|SecurityException unsupported){return 1;}
    }

    private static void invokeRename(Class<?> type,Object target,String from,String to)throws ReflectiveOperationException {
        java.lang.reflect.Method method=type.getMethod("renameat2",int.class,String.class,int.class,String.class,int.class);
        method.invoke(target,AT_FDCWD,from,AT_FDCWD,to,RENAME_NOREPLACE);
    }

    private static int translateRename(java.lang.reflect.InvocationTargetException problem,String to)throws IOException {
        Throwable cause=problem.getCause();
        if(!(cause instanceof android.system.ErrnoException))return 1;
        android.system.ErrnoException err=(android.system.ErrnoException)cause;
        if(err.errno==android.system.OsConstants.EEXIST||err.errno==android.system.OsConstants.ENOTEMPTY)
            throw new IOException("Device destination already exists");
        if(err.errno==android.system.OsConstants.ELOOP)throw new IOException("Device path is a symbolic link: "+to);
        if(linkUnsupported(err.errno)||err.errno==android.system.OsConstants.EINVAL||err.errno==android.system.OsConstants.EPERM)
            return 1;
        throw new IOException("Device rename could not finish: "+to+". "+err.getMessage(),err);
    }

    private static boolean linkUnsupported(int errno){
        return errno==android.system.OsConstants.ENOSYS||errno==android.system.OsConstants.EPERM
            ||errno==android.system.OsConstants.EOPNOTSUPP||errno==android.system.OsConstants.ENOTSUP
            ||errno==android.system.OsConstants.EXDEV;
    }

    private static String relocateFile(HeldDirectory sourceDirectory,String sourceName,String sourceLabel,HeldDirectory destinationDirectory,String destinationName,Check check)throws IOException {
        String from=sourceDirectory.childPath(sourceName),to=destinationDirectory.childPath(destinationName);
        if(renameNoReplace(from,to)==0){
            try{
                android.system.StructStat destStat=android.system.Os.lstat(to);
                if((destStat.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFREG)
                    throw new IOException("Device path is not a regular file: "+sourceLabel);
                try{
                    android.system.StructStat origin=android.system.Os.lstat(from);
                    if(origin.st_dev==destStat.st_dev&&origin.st_ino==destStat.st_ino)
                        throw new IOException("Device rename left both names; the original remains at "+sourceLabel);
                }catch(android.system.ErrnoException gone){
                    if(gone.errno!=android.system.OsConstants.ENOENT)
                        throw new IOException("Device file is unavailable: "+sourceLabel+". "+gone.getMessage(),gone);
                }
                return "moved by rename";
            }catch(android.system.ErrnoException problem){
                throw new IOException("Device rename could not finish: "+destinationName+". "+problem.getMessage(),problem);
            }
        }
        try{android.system.Os.link(from,to);}
        catch(android.system.ErrnoException problem){
            if(problem.errno==android.system.OsConstants.EEXIST)throw new IOException("Device destination already exists");
            if(problem.errno==android.system.OsConstants.ELOOP)throw new IOException("Device path is a symbolic link: "+destinationName);
            if(!linkUnsupported(problem.errno))throw new IOException("Device rename could not finish: "+destinationName+". "+problem.getMessage(),problem);
            android.system.StructStat stat;
            try{stat=android.system.Os.lstat(from);}
            catch(android.system.ErrnoException missing){throw new IOException("Device file is unavailable: "+sourceLabel+". "+missing.getMessage(),missing);}
            int format=stat.st_mode&android.system.OsConstants.S_IFMT;
            if(format==android.system.OsConstants.S_IFLNK)throw new IOException("Device path is a symbolic link: "+sourceLabel);
            if(format!=android.system.OsConstants.S_IFREG)throw new IOException("Device path is not a regular file: "+sourceLabel);
            return moveByCopy(sourceDirectory,sourceName,sourceLabel,destinationDirectory,destinationName,check);
        }
        try{
            android.system.StructStat origin=android.system.Os.lstat(from),linkedStat=android.system.Os.lstat(to);
            if(origin.st_ino!=linkedStat.st_ino||origin.st_dev!=linkedStat.st_dev
                ||(origin.st_mode&android.system.OsConstants.S_IFMT)!=(linkedStat.st_mode&android.system.OsConstants.S_IFMT))
                throw new IOException("Device rename could not finish: "+destinationName);
            removeMatched(sourceDirectory,sourceName,origin.st_dev,origin.st_ino);
            return "moved by link";
        }catch(android.system.ErrnoException problem){
            throw new IOException("Device file is linked at "+destinationName+" but the original name remains at "+sourceLabel+". "+problem.getMessage(),problem);
        }catch(IOException problem){
            try{
                android.system.StructStat origin=android.system.Os.lstat(from),linkedStat=android.system.Os.lstat(to);
                if(origin.st_dev==linkedStat.st_dev&&origin.st_ino==linkedStat.st_ino)
                    discardIfMatch(destinationDirectory,destinationName,linkedStat.st_dev,linkedStat.st_ino);
            }catch(android.system.ErrnoException ignored){}
            throw problem;
        }
    }

    private static String moveByCopy(HeldDirectory sourceDirectory,String sourceName,String sourceLabel,HeldDirectory destinationDirectory,String destinationName,Check check)throws IOException {
        CopiedFile copied=copyExclusive(sourceDirectory,sourceName,sourceLabel,destinationDirectory,destinationName,check);
        try{verifyClaimed(sourceDirectory,sourceName,destinationDirectory,destinationName,copied,check);}
        catch(IOException problem){
            discardIfMatch(destinationDirectory,destinationName,copied.destDev,copied.destIno);
            throw problem;
        }
        try{removeMatched(sourceDirectory,sourceName,copied.sourceDev,copied.sourceIno);}
        catch(IOException problem){
            throw new IOException("Copied to "+destinationName+" but the original remains at "+sourceLabel+". "+problem.getMessage(),problem);
        }
        return "moved by copy";
    }

    private static void claimDirectory(HeldDirectory sourceDirectory,File source,HeldDirectory destinationDirectory,File destination,Check check)throws IOException {
        String name=destination.getName(),destPath=destinationDirectory.childPath(name);
        boolean claimed=false;
        HeldDirectory claimedDir=null,moving=null;
        try{
            android.system.Os.mkdir(destPath,0700);
            claimed=true;
            claimedDir=holdDirectory(openRelative(destinationDirectory,name,android.system.OsConstants.O_RDONLY|OPEN_DIRECTORY,0));
            android.system.StructStat claimedStat=android.system.Os.fstat(claimedDir.descriptor());
            if((claimedStat.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFDIR)
                throw new IOException("Device destination is not a directory: "+destination);
            moving=holdDirectory(openRelative(sourceDirectory,source.getName(),android.system.OsConstants.O_RDONLY|OPEN_DIRECTORY,0));
            android.system.StructStat sourceStat=android.system.Os.fstat(moving.descriptor());
            if((sourceStat.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFDIR)
                throw new IOException("Device path is not a directory: "+source);
            if(sourceStat.st_dev!=claimedStat.st_dev)throw new IOException("Directory copy across volumes is not available; nothing was moved");
            MoveWalk walk=new MoveWalk(check);
            try{moveChildren(moving,claimedDir,false,walk,1);}
            catch(IOException problem){restoreDirectory(destinationDirectory,name,claimedDir,moving,problem);}
            android.system.StructStat pathStat=android.system.Os.lstat(destPath);
            if(pathStat.st_ino!=claimedStat.st_ino||pathStat.st_dev!=claimedStat.st_dev
                ||(pathStat.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFDIR){
                restoreDirectory(destinationDirectory,name,claimedDir,moving,new IOException("Device destination changed during the move; nothing was moved"));
            }
            android.system.StructStat still=android.system.Os.lstat(sourceDirectory.childPath(source.getName()));
            if(still.st_ino!=sourceStat.st_ino||still.st_dev!=sourceStat.st_dev
                ||(still.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFDIR)
                throw new IOException("Device path changed before removal; the moved entries are at "+destination);
            claimed=false;
            releaseMatched(sourceDirectory,source.getName(),sourceStat.st_dev,sourceStat.st_ino);
        }catch(android.system.ErrnoException problem){
            if(problem.errno==android.system.OsConstants.EEXIST||problem.errno==android.system.OsConstants.ENOTEMPTY)
                throw new IOException("Device destination already exists");
            if(problem.errno==android.system.OsConstants.ELOOP)throw new IOException("Device path is a symbolic link: "+destination);
            throw new IOException("Device rename could not finish: "+destination+". "+problem.getMessage(),problem);
        }finally{
            if(claimed&&claimedDir!=null){
                try{
                    android.system.StructStat stat=android.system.Os.fstat(claimedDir.descriptor());
                    releaseMatched(destinationDirectory,name,stat.st_dev,stat.st_ino);
                }catch(IOException|android.system.ErrnoException ignored){}
            }
            if(moving!=null)moving.quiet();
            if(claimedDir!=null)claimedDir.quiet();
        }
    }

    private static HeldDirectory holdDirectory(FileDescriptor opened)throws IOException { return new HeldDirectory(opened); }

    private static void restoreDirectory(HeldDirectory destinationDirectory,String name,HeldDirectory claimedDir,HeldDirectory moving,IOException problem)throws IOException {
        try{
            moveChildren(claimedDir,moving,true,new MoveWalk(ROLLBACK),1);
            android.system.StructStat stat=android.system.Os.fstat(claimedDir.descriptor());
            releaseMatched(destinationDirectory,name,stat.st_dev,stat.st_ino);
        }catch(IOException|android.system.ErrnoException rollback){
            throw new IOException("Device directory move did not finish; source and destination may both contain entries. "+problem.getMessage(),problem);
        }
        throw problem;
    }

    private static void moveChildren(HeldDirectory source,HeldDirectory destination,boolean restore,MoveWalk walk,int depth)throws IOException {
        walk.depth(depth);
        walk.entry(null);
        try(java.nio.file.DirectoryStream<java.nio.file.Path> entries=java.nio.file.Files.newDirectoryStream(java.nio.file.Paths.get("/proc/self/fd/"+source.number()))){
            for(java.nio.file.Path entry:entries){
                String name=entry.getFileName().toString();
                if(name.equals(".")||name.equals(".."))continue;
                walk.entry(name);
                moveEntry(source,destination,name,restore,walk,depth);
            }
        }
    }

    private static void moveEntry(HeldDirectory source,HeldDirectory destination,String name,boolean restore,MoveWalk walk,int depth)throws IOException {
        leafName(name);
        String from=source.childPath(name),to=destination.childPath(name);
        android.system.StructStat stat;
        try{stat=android.system.Os.lstat(from);}
        catch(android.system.ErrnoException problem){
            if(problem.errno==android.system.OsConstants.ENOENT)return;
            throw new IOException("Device path is unavailable: "+name+". "+problem.getMessage(),problem);
        }
        int format=stat.st_mode&android.system.OsConstants.S_IFMT;
        if(format==android.system.OsConstants.S_IFDIR){
            boolean created=false;
            try{android.system.Os.mkdir(to,0700);created=true;}
            catch(android.system.ErrnoException problem){
                if(problem.errno!=android.system.OsConstants.EEXIST||!restore){
                    if(problem.errno==android.system.OsConstants.EEXIST)throw new IOException("Device destination already exists");
                    throw new IOException("Device rename could not finish: "+name+". "+problem.getMessage(),problem);
                }
            }
            HeldDirectory sourceHeld=null,destHeld=null;
            long sourceDev=0,sourceIno=0;
            try{
                destHeld=holdDirectory(android.system.Os.open(to,android.system.OsConstants.O_RDONLY|OPEN_DIRECTORY|android.system.OsConstants.O_NOFOLLOW|android.system.OsConstants.O_CLOEXEC,0));
                sourceHeld=holdDirectory(android.system.Os.open(from,android.system.OsConstants.O_RDONLY|OPEN_DIRECTORY|android.system.OsConstants.O_NOFOLLOW|android.system.OsConstants.O_CLOEXEC,0));
                android.system.StructStat opened=android.system.Os.fstat(sourceHeld.descriptor());
                sourceDev=opened.st_dev;sourceIno=opened.st_ino;
                moveChildren(sourceHeld,destHeld,restore,walk,depth+1);
                if(created)try{android.system.Os.chmod(to,stat.st_mode&0777);}catch(android.system.ErrnoException ignored){}
                android.system.StructStat still=android.system.Os.lstat(from);
                if(still.st_dev!=sourceDev||still.st_ino!=sourceIno
                    ||(still.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFDIR)
                    throw new IOException("Device path changed before removal; it was left in place");
            }catch(android.system.ErrnoException problem){
                throw new IOException("Device rename could not finish: "+name+". "+problem.getMessage(),problem);
            }finally{
                if(sourceHeld!=null)sourceHeld.quiet();
                if(destHeld!=null)destHeld.quiet();
            }
            releaseMatched(source,name,sourceDev,sourceIno);
            return;
        }
        if(format==android.system.OsConstants.S_IFLNK)throw new IOException("Device path is a symbolic link: "+name);
        if(format!=android.system.OsConstants.S_IFREG)throw new IOException("Device path is not a regular file: "+name);
        relocateFile(source,name,name,destination,name,walk.check);
    }

    private static CopiedFile copyExclusive(HeldDirectory sourceDirectory,String sourceName,String sourceLabel,HeldDirectory destinationDirectory,String destinationName,Check check)throws IOException {
        FileDescriptor inputFd=null,outputFd=null;
        boolean claimed=false,identified=false;
        long sourceDev=0,sourceIno=0,destDev=0,destIno=0;
        try{
            inputFd=openRelative(sourceDirectory,sourceName,android.system.OsConstants.O_RDONLY|android.system.OsConstants.O_NONBLOCK,0);
            android.system.StructStat stat=android.system.Os.fstat(inputFd);
            if((stat.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFREG)
                throw new IOException("Device path is not a regular file: "+sourceLabel);
            sourceDev=stat.st_dev;sourceIno=stat.st_ino;
            outputFd=openRelative(destinationDirectory,destinationName,android.system.OsConstants.O_WRONLY|android.system.OsConstants.O_CREAT|android.system.OsConstants.O_EXCL,0600);
            claimed=true;
            android.system.StructStat created=android.system.Os.fstat(outputFd);
            destDev=created.st_dev;destIno=created.st_ino;identified=true;
            try(InputStream input=new DeviceFileInput(inputFd);OutputStream output=new DeviceFileOutput(outputFd)){
                inputFd=null;outputFd=null;
                transfer(input,output,check);
            }
            return new CopiedFile(sourceDev,sourceIno,destDev,destIno);
        }catch(android.system.ErrnoException problem){
            if(inputFd!=null)try{android.system.Os.close(inputFd);}catch(android.system.ErrnoException ignored){}
            if(outputFd!=null)try{android.system.Os.close(outputFd);}catch(android.system.ErrnoException ignored){}
            if(identified)discardIfMatch(destinationDirectory,destinationName,destDev,destIno);
            else if(claimed)throw new IOException("Device copy could not finish: "+destinationName+". An exclusive file may remain because its identity was not recorded. "+problem.getMessage(),problem);
            if(problem.errno==android.system.OsConstants.EEXIST)throw new IOException("Device destination already exists");
            if(problem.errno==android.system.OsConstants.ELOOP)throw new IOException("Device path is a symbolic link: "+destinationName);
            throw new IOException("Device copy could not finish: "+destinationName+". "+problem.getMessage(),problem);
        }catch(IOException problem){
            if(inputFd!=null)try{android.system.Os.close(inputFd);}catch(android.system.ErrnoException ignored){}
            if(outputFd!=null)try{android.system.Os.close(outputFd);}catch(android.system.ErrnoException ignored){}
            if(identified)discardIfMatch(destinationDirectory,destinationName,destDev,destIno);
            if(problem.getMessage()!=null&&(problem.getMessage().startsWith("Device destination already exists")||problem.getMessage().startsWith("Device copy could not finish")))throw problem;
            throw new IOException("Device copy could not finish: "+destinationName+". "+problem.getMessage(),problem);
        }
    }

    private static void verifyClaimed(HeldDirectory sourceDirectory,String sourceName,HeldDirectory destinationDirectory,String destinationName,CopiedFile copied,Check check)throws IOException {
        FileDescriptor sourceFd=null,destinationFd=null;
        try{
            destinationFd=openRelative(destinationDirectory,destinationName,android.system.OsConstants.O_RDONLY|android.system.OsConstants.O_NONBLOCK,0);
            android.system.StructStat destinationStat=android.system.Os.fstat(destinationFd);
            if(destinationStat.st_dev!=copied.destDev||destinationStat.st_ino!=copied.destIno)
                throw new IOException("Device destination changed during the copy; original kept at "+sourceName);
            sourceFd=openRelative(sourceDirectory,sourceName,android.system.OsConstants.O_RDONLY|android.system.OsConstants.O_NONBLOCK,0);
            android.system.StructStat sourceStat=android.system.Os.fstat(sourceFd);
            if(sourceStat.st_dev!=copied.sourceDev||sourceStat.st_ino!=copied.sourceIno)
                throw new IOException("Device source changed during the copy; original kept at "+sourceName);
            JSONObject expected,actual;
            try(InputStream input=new DeviceFileInput(sourceFd)){sourceFd=null;expected=fingerprint(input,check);}
            try(InputStream input=new DeviceFileInput(destinationFd)){destinationFd=null;actual=fingerprint(input,check);}
            if(expected.getLong("bytes")!=actual.getLong("bytes")||!expected.getString("sha256").equals(actual.getString("sha256")))
                throw new IOException("Device copy differs from the source; original kept at "+sourceName);
        }catch(android.system.ErrnoException problem){
            throw new IOException("Device copy could not be verified: "+destinationName+". "+problem.getMessage(),problem);
        }catch(IOException problem){throw problem;}
        catch(Exception problem){throw new IOException("Device copy differs from the source; original kept at "+sourceName+". "+problem.getMessage(),problem);}
        finally{
            if(sourceFd!=null)try{android.system.Os.close(sourceFd);}catch(android.system.ErrnoException ignored){}
            if(destinationFd!=null)try{android.system.Os.close(destinationFd);}catch(android.system.ErrnoException ignored){}
        }
    }

    private static final Object DETACH=new Object();
    private static volatile File[] holdingDirectories=new File[0];
    private static int detachSerial;

    static void prepareHolding(Context context){
        ArrayList<File> roots=new ArrayList<>();
        File internal=context.getFilesDir();
        if(internal!=null){
            if(!internal.isDirectory())internal.mkdirs();
            roots.add(internal);
        }
        File[] external=context.getExternalFilesDirs(null);
        if(external!=null)for(File directory:external){
            if(directory==null)continue;
            if(!directory.isDirectory())directory.mkdirs();
            if(directory.isDirectory())roots.add(directory);
        }
        holdingDirectories=roots.toArray(new File[0]);
    }

    static String holdingArgument(){
        StringBuilder builder=new StringBuilder();
        File[] roots=holdingDirectories;
        if(roots==null)return "";
        for(File root:roots){
            if(root==null)continue;
            if(builder.length()>0)builder.append('\n');
            builder.append(new File(root,"device-hold").getAbsolutePath());
        }
        return builder.toString();
    }

    private static File holdingDirectory(long device)throws IOException {
        IOException last=null;
        File[] roots=holdingDirectories;
        if(roots==null)roots=new File[0];
        for(File root:roots){
            HeldDirectory parent=null;
            try{
                parent=openDirectory(root);
                android.system.StructStat stat=android.system.Os.fstat(parent.descriptor());
                if(stat.st_dev!=device)continue;
                String path=parent.childPath("device-hold");
                try{android.system.Os.mkdir(path,0700);}
                catch(android.system.ErrnoException problem){
                    if(problem.errno!=android.system.OsConstants.EEXIST)throw problem;
                }
                android.system.StructStat held=android.system.Os.lstat(path);
                int format=held.st_mode&android.system.OsConstants.S_IFMT;
                if(format!=android.system.OsConstants.S_IFDIR||held.st_dev!=device)
                    throw new IOException("Device holding directory is not on the source volume");
                try{android.system.Os.chmod(path,0700);}catch(android.system.ErrnoException ignored){}
                return new File(root,"device-hold");
            }catch(IOException problem){last=problem;}
            catch(android.system.ErrnoException problem){last=new IOException(problem.getMessage(),problem);}
            finally{if(parent!=null)parent.quiet();}
        }
        if(last!=null)throw new IOException("Device file could not be removed safely because no private directory is on the same volume; it was left in place. "+last.getMessage(),last);
        throw new IOException("Device file could not be removed safely because no private directory is on the same volume; it was left in place");
    }

    private static String detachToken(){
        detachSerial++;
        return "d"+android.system.Os.getpid()+"-"+detachSerial+"-"+System.nanoTime();
    }

    // The shared name is renamed into an app-private directory before any removal.
    // A concurrent replacement is moved aside and kept; it is never unlinked by the old path.
    private static void releaseMatched(HeldDirectory directory,String name,long dev,long ino)throws IOException {
        synchronized(DETACH){
            String path=directory.childPath(name);
            FileDescriptor fd=null;
            HeldDirectory hold=null;
            String token=null;
            boolean displaced=false;
            try{
                fd=openRelative(directory,name,android.system.OsConstants.O_RDONLY,0);
                android.system.StructStat opened=android.system.Os.fstat(fd);
                android.system.StructStat linked=android.system.Os.lstat(path);
                if(opened.st_dev!=dev||opened.st_ino!=ino||linked.st_dev!=dev||linked.st_ino!=ino
                    ||(opened.st_mode&android.system.OsConstants.S_IFMT)!=(linked.st_mode&android.system.OsConstants.S_IFMT))
                    throw new IOException("Device path changed before removal; it was left in place");
                boolean directoryEntry=(opened.st_mode&android.system.OsConstants.S_IFMT)==android.system.OsConstants.S_IFDIR;
                hold=openDirectory(holdingDirectory(opened.st_dev));
                token=detachToken();
                String holdPath=hold.childPath(token);
                try{android.system.Os.lstat(holdPath);throw new IOException("Device holding name is already in use");}
                catch(android.system.ErrnoException absent){if(absent.errno!=android.system.OsConstants.ENOENT)throw absent;}
                long links=opened.st_nlink;
                try{
                    if(renameNoReplace(path,holdPath)!=0)android.system.Os.rename(path,holdPath);
                }catch(IOException problem){
                    if(problem.getMessage()!=null&&problem.getMessage().startsWith("Device destination already exists"))
                        throw new IOException("Device holding name is already in use");
                    throw problem;
                }
                displaced=true;
                android.system.StructStat held=android.system.Os.lstat(holdPath);
                if(held.st_dev!=dev||held.st_ino!=ino||(held.st_mode&android.system.OsConstants.S_IFMT)!=(opened.st_mode&android.system.OsConstants.S_IFMT)){
                    boolean restored=restoreDisplaced(hold,token,directory,name);
                    displaced=false;
                    throw new IOException("Device path changed before removal; a concurrent file was kept and was not deleted."+holdingNote(restored,token));
                }
                try{
                    if(directoryEntry)java.nio.file.Files.delete(java.nio.file.Paths.get(holdPath));
                    else android.system.Os.remove(holdPath);
                }catch(IOException|android.system.ErrnoException problem){
                    boolean restored=restoreDisplaced(hold,token,directory,name);
                    displaced=false;
                    throw new IOException("Device path could not be removed: "+name+". "+problem.getMessage()+holdingNote(restored,token),problem);
                }
                displaced=false;
                android.system.StructStat after=android.system.Os.fstat(fd);
                if(after.st_dev!=dev||after.st_ino!=ino||after.st_nlink>=links)
                    throw new IOException("Device removal did not match the opened file; the shared path was not unlinked");
            }catch(android.system.ErrnoException problem){
                boolean restored=displaced&&hold!=null&&token!=null&&restoreDisplaced(hold,token,directory,name);
                throw new IOException("Device path could not be removed: "+name+". "+problem.getMessage()+holdingNote(restored,token),problem);
            }finally{if(fd!=null)try{android.system.Os.close(fd);}catch(android.system.ErrnoException ignored){}if(hold!=null)hold.quiet();}
        }
    }

    private static String holdingNote(boolean restored,String token){
        return restored?"":" The original remains in the private holding directory as "+token+" and was not deleted.";
    }

    // Restores a displaced name without replacing a file that appeared at the original path.
    // link and O_EXCL fail with EEXIST. A directory is claimed with mkdir, never with Os.rename.
    private static boolean restoreDisplaced(HeldDirectory hold,String token,HeldDirectory directory,String name)throws IOException{
        String holdPath=hold.childPath(token),path=directory.childPath(name);
        try{
            android.system.StructStat held=android.system.Os.lstat(holdPath);
            try{android.system.Os.lstat(path);return false;}
            catch(android.system.ErrnoException absent){if(absent.errno!=android.system.OsConstants.ENOENT)return false;}
            try{
                if(renameNoReplace(holdPath,path)==0){
                    android.system.StructStat restored=android.system.Os.lstat(path);
                    if(restored.st_dev!=held.st_dev||restored.st_ino!=held.st_ino)return false;
                    try{android.system.Os.lstat(holdPath);return false;}
                    catch(android.system.ErrnoException gone){return gone.errno==android.system.OsConstants.ENOENT;}
                }
            }catch(IOException occupied){return false;}
            int format=held.st_mode&android.system.OsConstants.S_IFMT;
            try{android.system.Os.link(holdPath,path);}
            catch(android.system.ErrnoException link){
                if(link.errno==android.system.OsConstants.EEXIST)return false;
                if(!linkUnsupported(link.errno))return false;
                if(format==android.system.OsConstants.S_IFDIR)return restoreDirectoryExclusive(hold,token,directory,name,held);
                if(format==android.system.OsConstants.S_IFREG)return restoreFileExclusive(hold,token,directory,name,held);
                return false;
            }
            android.system.StructStat restored=android.system.Os.lstat(path);
            if(restored.st_dev!=held.st_dev||restored.st_ino!=held.st_ino)return false;
            if(format==android.system.OsConstants.S_IFDIR)java.nio.file.Files.delete(java.nio.file.Paths.get(holdPath));
            else android.system.Os.remove(holdPath);
            return true;
        }catch(android.system.ErrnoException ignored){return false;}
        catch(IOException problem){
            String message=problem.getMessage();
            if(message!=null&&(message.contains("left in place")||message.contains("was not deleted")||message.contains("was kept")))throw problem;
            return false;
        }
    }

    private static boolean restoreFileExclusive(HeldDirectory hold,String token,HeldDirectory directory,String name,android.system.StructStat held)throws IOException{
        FileDescriptor in=null,out=null;
        long destDev=0,destIno=0;
        boolean claimed=false,restored=false;
        IOException cleanup=null;
        restore:{
        try{
            try{android.system.Os.lstat(directory.childPath(name));break restore;}
            catch(android.system.ErrnoException absent){if(absent.errno!=android.system.OsConstants.ENOENT)break restore;}
            in=openRelative(hold,token,android.system.OsConstants.O_RDONLY|android.system.OsConstants.O_NONBLOCK,0);
            android.system.StructStat opened=android.system.Os.fstat(in);
            if(opened.st_dev!=held.st_dev||opened.st_ino!=held.st_ino||(opened.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFREG)
                break restore;
            long size=opened.st_size;
            if(size<0||Long.toString(size).length()>15)break restore;
            out=openRelative(directory,name,android.system.OsConstants.O_WRONLY|android.system.OsConstants.O_CREAT|android.system.OsConstants.O_EXCL,0600);
            claimed=true;
            android.system.StructStat created=android.system.Os.fstat(out);
            destDev=created.st_dev;destIno=created.st_ino;
            byte[] buffer=new byte[65536];
            long remaining=size;
            while(remaining>0){
                int want=(int)Math.min(buffer.length,remaining);
                int n=android.system.Os.read(in,buffer,0,want);
                if(n<=0)break;
                int at=0;
                while(at<n){
                    int wrote=android.system.Os.write(out,buffer,at,n-at);
                    if(wrote<=0)throw new IOException("Device write stopped before all bytes were written");
                    at+=wrote;
                }
                remaining-=n;
            }
            android.system.Os.fsync(out);
            android.system.StructStat after=android.system.Os.fstat(out);
            android.system.StructStat sourceNow=android.system.Os.fstat(in);
            if(after.st_dev!=destDev||after.st_ino!=destIno||after.st_size!=size||sourceNow.st_dev!=held.st_dev||sourceNow.st_ino!=held.st_ino||sourceNow.st_size!=size||remaining!=0)
                break restore;
            android.system.Os.close(out);out=null;
            android.system.Os.close(in);in=null;
            Check check=()->{};
            JSONObject expected,actual;
            try(InputStream input=new DeviceFileInput(openRelative(hold,token,android.system.OsConstants.O_RDONLY|android.system.OsConstants.O_NONBLOCK,0))){expected=fingerprint(input,check);}
            try(InputStream input=new DeviceFileInput(openRelative(directory,name,android.system.OsConstants.O_RDONLY|android.system.OsConstants.O_NONBLOCK,0))){actual=fingerprint(input,check);}
            if(expected.getLong("bytes")!=size||actual.getLong("bytes")!=size||!expected.getString("sha256").equals(actual.getString("sha256")))
                break restore;
            android.system.StructStat linked=android.system.Os.lstat(hold.childPath(token));
            if(linked.st_dev!=held.st_dev||linked.st_ino!=held.st_ino)break restore;
            android.system.Os.remove(hold.childPath(token));
            claimed=false;
            restored=true;
        }catch(Exception ignored){}
        finally{
            if(in!=null)try{android.system.Os.close(in);}catch(android.system.ErrnoException ignored){}
            if(out!=null)try{android.system.Os.close(out);}catch(android.system.ErrnoException ignored){}
            if(claimed){
                try{dropRestoreFile(directory,name,destDev,destIno);}
                catch(IOException problem){cleanup=problem;}
            }
        }
        }
        if(cleanup!=null)throw cleanup;
        return restored;
    }

    private static void dropRestoreFile(HeldDirectory directory,String name,long dev,long ino)throws IOException{
        try{releaseMatched(directory,name,dev,ino);}
        catch(IOException problem){
            String message=problem.getMessage();
            if(message!=null&&(message.contains("left in place")||message.contains("was not deleted")||message.contains("was kept")))throw problem;
            throw new IOException((message==null?"Device path could not be removed":message)+" The path was left in place.",problem);
        }
    }

    private static boolean restoreDirectoryExclusive(HeldDirectory hold,String token,HeldDirectory directory,String name,android.system.StructStat held)throws IOException{
        String path=directory.childPath(name);
        HeldDirectory from=null,to=null;
        boolean claimed=false,restored=false;
        IOException cleanup=null;
        restoreDirectory:{
        try{
            try{android.system.Os.lstat(path);break restoreDirectory;}
            catch(android.system.ErrnoException absent){if(absent.errno!=android.system.OsConstants.ENOENT)break restoreDirectory;}
            android.system.Os.mkdir(path,0700);
            claimed=true;
            from=holdDirectory(openRelative(hold,token,android.system.OsConstants.O_RDONLY|OPEN_DIRECTORY,0));
            to=holdDirectory(openRelative(directory,name,android.system.OsConstants.O_RDONLY|OPEN_DIRECTORY,0));
            android.system.StructStat source=android.system.Os.fstat(from.descriptor());
            android.system.StructStat dest=android.system.Os.fstat(to.descriptor());
            if(source.st_dev!=held.st_dev||source.st_ino!=held.st_ino||(source.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFDIR)
                break restoreDirectory;
            if(dest.st_dev!=source.st_dev||(dest.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFDIR)
                break restoreDirectory;
            restoreChildren(from,to,new MoveWalk(()->{}),1);
            android.system.StructStat linked=android.system.Os.lstat(hold.childPath(token));
            if(linked.st_dev!=held.st_dev||linked.st_ino!=held.st_ino)break restoreDirectory;
            java.nio.file.Files.delete(java.nio.file.Paths.get(hold.childPath(token)));
            claimed=false;
            restored=true;
        }catch(Exception ignored){}
        finally{
            if(claimed&&from!=null&&to!=null)try{restoreChildren(to,from,new MoveWalk(()->{}),1);}catch(Exception ignored){}
            if(from!=null)from.quiet();
            if(to!=null)to.quiet();
            if(claimed){
                try{rmdirIfEmpty(path);}
                catch(IOException problem){cleanup=problem;}
            }
        }
        }
        if(cleanup!=null)throw cleanup;
        return restored;
    }

    // Removes only an empty directory. A file or a directory that gained children is left in place.
    private static void rmdirIfEmpty(String path)throws IOException{
        try{
            java.lang.reflect.Method rmdir=android.system.Os.class.getMethod("rmdir",String.class);
            rmdir.invoke(null,path);
        }catch(java.lang.reflect.InvocationTargetException problem){
            Throwable cause=problem.getCause();
            if(cause instanceof android.system.ErrnoException){
                android.system.ErrnoException errno=(android.system.ErrnoException)cause;
                if(errno.errno==android.system.OsConstants.ENOENT)return;
                if(errno.errno==android.system.OsConstants.ENOTDIR)throw new IOException("Device path changed before removal; it was left in place",errno);
                if(errno.errno==android.system.OsConstants.ENOTEMPTY||errno.errno==android.system.OsConstants.EEXIST)throw new IOException("Device directory was left in place because it was not empty",errno);
                throw new IOException("Device directory was left in place",errno);
            }
            throw new IOException("Device directory was left in place",problem);
        }catch(ReflectiveOperationException problem){
            throw new IOException("Device directory was left in place",problem);
        }
    }

    private static void restoreChildren(HeldDirectory source,HeldDirectory destination,MoveWalk walk,int depth)throws IOException {
        if(source==null||destination==null)return;
        walk.depth(depth);
        walk.entry(null);
        try(java.nio.file.DirectoryStream<java.nio.file.Path> entries=java.nio.file.Files.newDirectoryStream(java.nio.file.Paths.get("/proc/self/fd/"+source.number()))){
            for(java.nio.file.Path entry:entries){
                String child=entry.getFileName().toString();
                if(child.equals(".")||child.equals(".."))continue;
                walk.entry(child);
                restoreEntry(source,destination,child,walk,depth);
            }
        }
    }

    private static void restoreEntry(HeldDirectory source,HeldDirectory destination,String child,MoveWalk walk,int depth)throws IOException {
        leafName(child);
        String from=source.childPath(child),to=destination.childPath(child);
        android.system.StructStat stat;
        try{stat=android.system.Os.lstat(from);}
        catch(android.system.ErrnoException problem){
            if(problem.errno==android.system.OsConstants.ENOENT)return;
            throw new IOException("Device path is unavailable: "+child+". "+problem.getMessage(),problem);
        }
        int format=stat.st_mode&android.system.OsConstants.S_IFMT;
        if(format==android.system.OsConstants.S_IFDIR){
            try{android.system.Os.mkdir(to,0700);}
            catch(android.system.ErrnoException problem){
                if(problem.errno==android.system.OsConstants.EEXIST)throw new IOException("Device destination already exists");
                throw new IOException("Device rename could not finish: "+child+". "+problem.getMessage(),problem);
            }
            HeldDirectory fromDir=null,toDir=null;
            try{
                fromDir=holdDirectory(openRelative(source,child,android.system.OsConstants.O_RDONLY|OPEN_DIRECTORY,0));
                toDir=holdDirectory(openRelative(destination,child,android.system.OsConstants.O_RDONLY|OPEN_DIRECTORY,0));
                restoreChildren(fromDir,toDir,walk,depth+1);
            }catch(android.system.ErrnoException problem){
                throw new IOException("Device rename could not finish: "+child+". "+problem.getMessage(),problem);
            }finally{if(fromDir!=null)fromDir.quiet();if(toDir!=null)toDir.quiet();}
            android.system.StructStat linked;
            try{linked=android.system.Os.lstat(from);}
            catch(android.system.ErrnoException problem){throw new IOException(problem.getMessage(),problem);}
            if(linked.st_dev!=stat.st_dev||linked.st_ino!=stat.st_ino)throw new IOException("Device path changed before removal; it was left in place");
            try{java.nio.file.Files.delete(java.nio.file.Paths.get(from));}
            catch(IOException problem){throw new IOException("Device path could not be removed: "+child+". "+problem.getMessage(),problem);}
            return;
        }
        if(format!=android.system.OsConstants.S_IFREG)throw new IOException("Device path is not a regular file: "+child);
        if(!restoreFileExclusive(source,child,destination,child,stat))throw new IOException("Device rename could not finish: "+child);
    }

    private static void removeMatched(HeldDirectory directory,String name,long dev,long ino)throws IOException {
        releaseMatched(directory,name,dev,ino);
    }

    private static void discardIfMatch(HeldDirectory directory,String name,long dev,long ino){
        try{releaseMatched(directory,name,dev,ino);}catch(IOException ignored){}
    }

    static void ensureReviewDirectory(File review)throws IOException {
        File parent=review.getParentFile();
        if(parent==null)throw new IOException("Device review path is not a directory");
        leafName(review.getName());
        HeldDirectory directory=openDirectory(parent);
        try{
            if(!reviewDirectory(directory,review.getName())){
                try{android.system.Os.mkdir(directory.childPath(review.getName()),0700);}
                catch(android.system.ErrnoException problem){
                    if(problem.errno!=android.system.OsConstants.EEXIST)
                        throw new IOException("Device review path is unavailable: "+review+". "+problem.getMessage(),problem);
                }
                if(!reviewDirectory(directory,review.getName()))throw new IOException("Device review path is not a directory");
            }
        }finally{directory.quiet();}
    }

    private static boolean reviewDirectory(HeldDirectory directory,String name)throws IOException {
        try{
            android.system.StructStat stat=android.system.Os.lstat(directory.childPath(name));
            int format=stat.st_mode&android.system.OsConstants.S_IFMT;
            if(format==android.system.OsConstants.S_IFLNK||format!=android.system.OsConstants.S_IFDIR)
                throw new IOException("Device review path is not a directory");
            return true;
        }catch(android.system.ErrnoException problem){
            if(problem.errno==android.system.OsConstants.ENOENT)return false;
            throw new IOException("Device review path is unavailable: "+name+". "+problem.getMessage(),problem);
        }
    }

    private static File retainSource(File source,Check check)throws IOException {
        File parent=source.getParentFile();
        if(parent==null)throw new IOException("Device original remains at "+source);
        File review=new File(parent,"review");
        ensureReviewDirectory(review);
        long stamp=System.currentTimeMillis();
        IOException collision=null;
        for(int counter=0;counter<1000;counter++){
            check.run();
            File candidate=new File(review,reviewLeaf(source.getName(),stamp,counter));
            try{
                HeldDirectory sourceDirectory=openDirectory(parent);
                try{
                    HeldDirectory reviewDirectory=openDirectory(review);
                    try{relocateFile(sourceDirectory,source.getName(),source.getPath(),reviewDirectory,candidate.getName(),check);return candidate;}
                    finally{reviewDirectory.quiet();}
                }finally{sourceDirectory.quiet();}
            }catch(IOException problem){
                String message=problem.getMessage();
                if(message==null||!message.startsWith("Device destination already exists"))throw problem;
                collision=problem;
            }
        }
        throw collision==null?new IOException("Device review destination already exists"):collision;
    }

    static void create(File file,InputStream input,Check check)throws IOException {
        File parent=file.getParentFile();
        if(parent==null)throw new IOException("Choose a file inside a device folder");
        leafName(file.getName());
        check.run();
        HeldDirectory directory=openDirectory(parent);
        FileDescriptor fd=null;
        try{
            fd=openRelative(directory,file.getName(),android.system.OsConstants.O_WRONLY|android.system.OsConstants.O_CREAT|android.system.OsConstants.O_EXCL,0600);
            try(OutputStream out=new DeviceFileOutput(fd)){
                fd=null;
                transfer(input,out,check);
            }
        }catch(android.system.ErrnoException problem){
            if(fd!=null)try{android.system.Os.close(fd);}catch(android.system.ErrnoException ignored){}
            if(problem.errno==android.system.OsConstants.EEXIST)throw new IOException("Device destination already exists");
            if(problem.errno==android.system.OsConstants.ELOOP)throw new IOException("Device path is a symbolic link: "+file);
            throw new IOException("New device file may be incomplete: "+file+". "+problem.getMessage(),problem);
        }catch(IOException problem){
            if(fd!=null)try{android.system.Os.close(fd);}catch(android.system.ErrnoException ignored){}
            if(problem.getMessage()!=null&&problem.getMessage().startsWith("Device destination already exists"))throw problem;
            throw new IOException("New device file may be incomplete: "+file+". "+problem.getMessage(),problem);
        }finally{directory.quiet();}
    }

    static JSONObject fingerprint(InputStream input,Check check)throws Exception {
        MessageDigest hash=MessageDigest.getInstance("SHA-256");
        long size=0;
        byte[] buffer=new byte[65536];
        int n;
        while((n=input.read(buffer))!=-1){
            check.run();
            if(n<0||size>Long.MAX_VALUE-n)throw new IOException("Device file exceeds the readable size");
            hash.update(buffer,0,n);size+=n;
        }
        check.run();
        StringBuilder hex=new StringBuilder(64);
        for(byte b:hash.digest())hex.append(Character.forDigit((b>>>4)&15,16)).append(Character.forDigit(b&15,16));
        return new JSONObject().put("bytes",size).put("sha256",hex.toString());
    }

    static void transfer(InputStream input,OutputStream output,Check check)throws IOException {
        byte[] buffer=new byte[65536];
        int n;
        while((n=input.read(buffer))!=-1){check.run();output.write(buffer,0,n);}
        check.run();
    }

    static void transferTo(FileDescriptor fd,InputStream input,Check check)throws IOException {
        byte[] buffer=new byte[65536];
        int n;
        while((n=input.read(buffer))!=-1){
            check.run();
            if(n<0)throw new IOException("Device read failed");
            int at=0;
            while(at<n){
                int wrote;
                try{wrote=android.system.Os.write(fd,buffer,at,n-at);}
                catch(android.system.ErrnoException|InterruptedIOException problem){throw new IOException(problem.getMessage(),problem);}
                if(wrote<=0)throw new IOException("Device write stopped before all bytes were written");
                at+=wrote;
            }
        }
        check.run();
        try{android.system.Os.fsync(fd);}
        catch(android.system.ErrnoException problem){throw new IOException(problem.getMessage(),problem);}
    }

    static String searchMode(String mode)throws IOException {
        if(mode==null||mode.isEmpty()||"name".equals(mode))return "name";
        if("content".equals(mode)||"both".equals(mode))return mode;
        throw new IOException("Choose a device search mode of name, content, or both");
    }

    static JSONObject search(File file,String query,String mode,Check check)throws Exception {
        validateQuery(query);
        String selected=searchMode(mode);
        SearchRun run=new SearchRun();
        searchAt(file,"",query,selected,check,run,0);
        return run.result();
    }

    private static void validateQuery(String query)throws IOException {
        if(query==null||query.isEmpty()||query.length()>1024||query.indexOf(0)>=0)
            throw new IOException("Enter a device search query of at most 1024 characters");
    }

    private static void searchAt(File file,String relative,String query,String mode,Check check,SearchRun run,int depth)throws Exception {
        check.run();
        if(depth>32)throw new IOException("Device search exceeds 32 nested directories");
        if(run.seen==100000)throw new IOException("Device search exceeds 100000 entries");
        run.seen++;
        JSONObject meta=metadata(file);
        String kind=meta.getString("kind");
        if("link".equals(kind))throw new IOException("Device path is a symbolic link: "+file);
        boolean directory="directory".equals(kind);
        String label=relative.isEmpty()?file.getName():relative;
        boolean names=!"content".equals(mode),bodies=!"name".equals(mode);
        boolean nameMatch=names&&file.getName().contains(query);
        boolean contentMatch=false;
        if(bodies&&!directory&&"file".equals(kind)&&!nameMatch){
            try{contentMatch=run.content(label,regularInput(file),query,check);}
            catch(IOException problem){
                if(cancelled(problem))throw problem;
                run.note(label+": "+(problem.getMessage()==null?"Device search could not read the file":problem.getMessage()));
            }
        }
        if(nameMatch||contentMatch)addMatch(run.rows,label,directory,run.used);
        if(!directory)return;
        JSONArray listing=list(file,check,5000);
        for(int index=0;index<listing.length();index++){
            JSONObject row=listing.getJSONObject(index);
            String name=row.getString("name");
            String child=relative.isEmpty()?name:relative+"/"+name;
            searchAt(new File(file,name),child,query,mode,check,run,depth+1);
        }
    }

    private static final class ContentProbe {
        final boolean match;
        final String incomplete;
        ContentProbe(boolean match,String incomplete){this.match=match;this.incomplete=incomplete;}
    }

    private static final class SearchRun {
        final JSONArray rows=new JSONArray();
        final int[] used=new int[]{2};
        int seen;
        private String reason;
        private int notes;
        boolean content(String label,InputStream input,String query,Check check)throws IOException {
            try{
                ContentProbe probe=readContains(input,query,check);
                if(probe.incomplete!=null)note(label+": "+probe.incomplete);
                return probe.match;
            }catch(IOException problem){
                if(cancelled(problem))throw problem;
                note(label+": "+(problem.getMessage()==null?"Device search could not read the file":problem.getMessage()));
                return false;
            }
        }
        void note(String text){
            if(text==null||text.isEmpty())return;
            notes++;
            if(reason==null)reason=text;
            else if(reason.length()<1200)reason=reason+"; "+text;
            else if(!reason.endsWith("further files were not fully searched"))reason=reason+"; further files were not fully searched";
        }
        JSONObject result()throws Exception {
            JSONObject result=new JSONObject().put("matches",rows).put("coverage",reason==null?"complete":"partial");
            if(reason!=null)result.put("reason",notes>1?notes+" files were not fully searched. "+reason:reason);
            return result;
        }
    }

    private static boolean cancelled(Throwable problem){
        if(problem instanceof android.os.OperationCanceledException)return true;
        String message=problem.getMessage();
        return message!=null&&(message.contains("cancel")||message.contains("Cancel"));
    }

    private static ContentProbe readContains(InputStream input,String query,Check check)throws IOException {
        try(InputStream in=input){
            byte[] needle=query.getBytes(StandardCharsets.UTF_8);
            if(needle.length==0||needle.length>4096)throw new IOException("Enter a device search query of at most 1024 characters");
            int[] border=prefixBorders(needle);
            byte[] buffer=new byte[65536];
            int matched=0,zeros=0;
            long seen=0;
            final long cap=1024L*1024L;
            while(seen<cap){
                check.run();
                int space=(int)Math.min(buffer.length,cap-seen);
                if(space<=0)throw new IOException("Device search could not read the file");
                int n=in.read(buffer,0,space);
                if(n<0)return new ContentProbe(false,null);
                if(n==0){if(++zeros>8)throw new IOException("Device search could not read the file");continue;}
                zeros=0;
                for(int i=0;i<n;i++){
                    if((i&4095)==0)check.run();
                    matched=advanceMatch(buffer[i],needle,border,matched);
                    if(matched==needle.length)return new ContentProbe(true,null);
                }
                seen+=n;
            }
            check.run();
            int extra=in.read();
            if(extra<0)return new ContentProbe(false,null);
            return new ContentProbe(false,"content search stopped after 1 MiB; the rest of the file was not read");
        }
    }

    private static int[] prefixBorders(byte[] needle){
        int[] border=new int[needle.length];
        int length=0;
        for(int i=1;i<needle.length;i++){
            while(length>0&&needle[i]!=needle[length])length=border[length-1];
            if(needle[i]==needle[length])length++;
            border[i]=length;
        }
        return border;
    }

    private static int advanceMatch(byte value,byte[] needle,int[] border,int matched){
        while(matched>0&&value!=needle[matched])matched=border[matched-1];
        if(value==needle[matched])matched++;
        return matched;
    }

    private static void addMatch(JSONArray rows,String path,boolean directory,int[] used)throws Exception {
        if(path==null||path.isEmpty()||path.indexOf(0)>=0||path.length()>MAX_PATH_CHARS)throw new IOException("Device search path exceeds 4096 characters");
        if(rows.length()>=500)throw new IOException("Device search exceeds 500 matches");
        JSONObject row=new JSONObject().put("path",path).put("directory",directory);
        int addition=row.toString().getBytes(StandardCharsets.UTF_8).length+1;
        if(used[0]>MAX_LIST_BYTES-addition)throw new IOException("Device search exceeds 1 MiB");
        rows.put(row);
        used[0]+=addition;
    }

    static JSONArray list(File folder,Check check)throws Exception { return list(folder,check,500); }

    static JSONArray list(File folder,Check check,int limit)throws Exception {
        HeldDirectory directory=openDirectory(folder);
        try{
            JSONArray rows=new JSONArray();
            int[] used=new int[]{2};
            try(java.nio.file.DirectoryStream<java.nio.file.Path> entries=java.nio.file.Files.newDirectoryStream(java.nio.file.Paths.get("/proc/self/fd/"+directory.number()))){
                for(java.nio.file.Path entry:entries){
                    check.run();
                    String name=entry.getFileName().toString();
                    if(name.equals(".")||name.equals(".."))continue;
                    android.system.StructStat stat;
                    try{stat=android.system.Os.lstat(directory.childPath(name));}
                    catch(android.system.ErrnoException problem){throw new IOException("Device path is unavailable: "+name+". "+problem.getMessage(),problem);}
                    addListed(rows,name,(stat.st_mode&android.system.OsConstants.S_IFMT)==android.system.OsConstants.S_IFDIR,limit,"Device folder exceeds "+limit+" entries",used);
                }
            }
            return rows;
        }finally{directory.quiet();}
    }

    static final class Tree {
        private final ContentResolver resolver;
        private final Uri tree;
        private final CancellationSignal signal;
        private final Check check;
        final String root;

        Tree(ContentResolver resolver,Uri tree,boolean write,CancellationSignal signal,Check check)throws IOException {
            this.resolver=resolver;this.tree=tree;this.signal=signal;this.check=check;
            requireLocalTree(tree);
            for(UriPermission grant:resolver.getPersistedUriPermissions()){
                if(tree.equals(grant.getUri())&&grant.isReadPermission()&&(!write||grant.isWritePermission())){
                    root=DocumentsContract.getTreeDocumentId(tree);
                    return;
                }
            }
            throw new IOException("The selected device folder grant is unavailable");
        }

        private Uri uri(String id) { return DocumentsContract.buildDocumentUriUsingTree(tree,id); }

        private Cursor children(String id)throws Exception { return children(id,true); }

        private Cursor children(String id,boolean observe)throws Exception {
            if(observe)check.run();
            Cursor cursor=resolver.query(DocumentsContract.buildChildDocumentsUriUsingTree(tree,id),
                new String[]{DocumentsContract.Document.COLUMN_DOCUMENT_ID,DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                    DocumentsContract.Document.COLUMN_MIME_TYPE},null,null,null,observe?signal:null);
            if(cursor==null)throw new IOException("Device folder is unavailable");
            return cursor;
        }

        String child(String id,String name)throws Exception {
            String match=null;
            int count=0;
            try(Cursor cursor=children(id)){
                while(cursor.moveToNext()){
                    check.run();
                    if(++count>5000)throw new IOException("Device folder lookup limit exceeded");
                    if(!name.equals(cursor.getString(1)))continue;
                    if(match!=null)throw new IOException("Ambiguous device filename");
                    match=cursor.getString(0);
                }
            }
            return match;
        }

        String resolve(String path)throws Exception {
            String id=root;
            if(path.isEmpty())return id;
            boundPath(path);
            for(String part:path.split("/",-1)){
                leaf(part);
                id=child(id,part);
                if(id==null)throw new IOException("Device document is unavailable: "+part);
            }
            return id;
        }

        JSONArray list(String id)throws Exception {
            JSONArray rows=new JSONArray();
            int[] used=new int[]{2};
            try(Cursor cursor=children(id)){
                while(cursor.moveToNext()){
                    check.run();
                    addListed(rows,cursor.getString(1),DocumentsContract.Document.MIME_TYPE_DIR.equals(cursor.getString(2)),5000,"Device folder exceeds 5000 entries",used);
                }
            }
            return rows;
        }

        JSONObject search(String id,String path,String query,String mode)throws Exception {
            validateQuery(query);
            String selected=searchMode(mode);
            SearchRun run=new SearchRun();
            boolean names=!"content".equals(selected),bodies=!"name".equals(selected);
            if(DocumentsContract.Document.MIME_TYPE_DIR.equals(documentMime(id)))searchDirectory(id,path,query,selected,run,0);
            else{
                String name=path.isEmpty()?"selected":new File(path).getName();
                String label=path.isEmpty()?name:path;
                boolean nameMatch=names&&name.contains(query);
                boolean contentMatch=bodies&&!nameMatch&&searchContent(run,label,id,query);
                if(nameMatch||contentMatch)addMatch(run.rows,label,false,run.used);
            }
            return run.result();
        }

        private boolean searchContent(SearchRun run,String label,String id,String query)throws Exception {
            try{return run.content(label,input(id),query,check);}
            catch(Exception problem){
                if(cancelled(problem)){
                    if(problem instanceof IOException)throw (IOException)problem;
                    if(problem instanceof RuntimeException)throw (RuntimeException)problem;
                    throw new IOException(problem.getMessage(),problem);
                }
                run.note(label+": "+(problem.getMessage()==null?"Device search could not read the file":problem.getMessage()));
                return false;
            }
        }

        private void searchDirectory(String id,String relative,String query,String mode,SearchRun run,int depth)throws Exception {
            check.run();
            if(depth>32)throw new IOException("Device search exceeds 32 nested directories");
            boolean names=!"content".equals(mode),bodies=!"name".equals(mode);
            try(Cursor cursor=children(id)){
                while(cursor.moveToNext()){
                    check.run();
                    if(run.seen==100000)throw new IOException("Device search exceeds 100000 entries");
                    run.seen++;
                    String childId=cursor.getString(0),childName=cursor.getString(1),childMime=cursor.getString(2);
                    if(childId==null||childName==null)throw new IOException("Invalid device directory entry");
                    leaf(childName);
                    boolean directory=DocumentsContract.Document.MIME_TYPE_DIR.equals(childMime);
                    String childPath=relative==null||relative.isEmpty()?childName:relative+"/"+childName;
                    boolean nameMatch=names&&childName.contains(query);
                    boolean contentMatch=bodies&&!directory&&!nameMatch&&searchContent(run,childPath,childId,query);
                    if(nameMatch||contentMatch)addMatch(run.rows,childPath,directory,run.used);
                    if(directory)searchDirectory(childId,childPath,query,mode,run,depth+1);
                }
            }
        }

        private static void leaf(String name)throws IOException {
            if(name.isEmpty()||name.equals(".")||name.equals("..")||name.indexOf('/')>=0||name.indexOf(0)>=0)
                throw new IOException("Enter a name inside the selected device folder");
            for(int i=0;i<name.length();i++)if(Character.isISOControl(name.charAt(i)))
                throw new IOException("Enter a name inside the selected device folder");
        }

        String create(String parent,String name,String mime)throws Exception {
            leaf(name);
            if(child(parent,name)!=null)throw new IOException("Device destination already exists");
            check.run();
            try{
                Uri created=DocumentsContract.createDocument(resolver,uri(parent),mime,name);
                return returnedId(created,parent,name);
            }catch(IOException problem){
                if(problem.getMessage()!=null&&problem.getMessage().startsWith("Device destination already exists"))throw problem;
                try{if(child(parent,name)!=null)throw new IOException("Device destination already exists");}
                catch(IOException exists){if(exists.getMessage()!=null&&exists.getMessage().startsWith("Device destination already exists"))throw exists;}
                throw problem;
            }catch(Exception problem){
                try{if(child(parent,name)!=null)throw new IOException("Device destination already exists");}
                catch(IOException exists){if(exists.getMessage()!=null&&exists.getMessage().startsWith("Device destination already exists"))throw exists;}
                throw new IOException("The document provider could not create the document. "+problem.getMessage(),problem);
            }
        }

        private String returnedId(Uri result,String parent,String name)throws Exception {
            if(result==null||!tree.getAuthority().equals(result.getAuthority()))throw new IOException("The document provider could not confirm the result");
            String id=DocumentsContract.getDocumentId(result);
            if(!id.equals(child(parent,name)))throw new IOException("The document provider did not confirm the requested destination");
            return id;
        }

        InputStream input(String id)throws Exception {
            check.run();
            android.os.ParcelFileDescriptor file=resolver.openFileDescriptor(uri(id),"r",signal);
            if(file==null)throw new IOException("Device file is unavailable");
            return new android.os.ParcelFileDescriptor.AutoCloseInputStream(file);
        }

        // A selected-folder replacement is written to a new document and synced before the original
        // name changes. If that name cannot be published, the original is restored and the new
        // document is removed. A provider that cannot create a sibling is the only truncating path,
        // and that path keeps the original bytes so a failed write can put them back.
        void write(String id,InputStream incoming)throws Exception {
            check.run();
            if(replaceWithSibling(id,incoming))return;
            File saved=null;
            byte[] savedBytes=null;
            try{saved=copyOriginal(id);}
            catch(Exception problem){
                discardSnapshot(saved);saved=null;
                if(cancelled(problem))throw problem;
            }
            if(saved==null){
                try{savedBytes=copyOriginalBytes(id);}
                catch(Exception problem){
                    if(cancelled(problem))throw problem;
                    if(problem instanceof IOException){
                        IOException io=(IOException)problem;
                        if(io.getMessage()!=null&&io.getMessage().contains("left unchanged"))throw io;
                        throw new IOException(leftUnchanged(io),io);
                    }
                    throw new IOException("Device file was left unchanged. "+(problem.getMessage()==null?"":problem.getMessage()),problem);
                }
            }
            android.os.ParcelFileDescriptor file=null;
            boolean truncated=false,finished=false;
            try{
                file=resolver.openFileDescriptor(uri(id),"wt",signal);
                if(file==null)throw new IOException("Device file cannot be opened for writing");
                truncated=true;
                transferTo(file.getFileDescriptor(),incoming,check);
                file.getFileDescriptor().sync();
                finished=true;
            }catch(Exception problem){
                if(file!=null){try{file.close();}catch(IOException ignored){} file=null;}
                if(!truncated){
                    discardSnapshot(saved);
                    throw problem;
                }
                try{restoreTruncated(id,saved,savedBytes);}
                catch(Exception restore){
                    discardSnapshot(saved);
                    problem.addSuppressed(restore);
                    throw new IOException("Device file could not be restored. "+(problem.getMessage()==null?"":problem.getMessage()),problem);
                }
                discardSnapshot(saved);
                if(cancelled(problem))throw problem;
                IOException io=problem instanceof IOException?(IOException)problem:new IOException(problem.getMessage(),problem);
                throw new IOException(leftUnchanged(io),io);
            }finally{if(file!=null)try{file.close();}catch(IOException ignored){}}
            discardSnapshot(saved);
            if(!finished)throw new IOException("Device file was left unchanged");
        }

        void writeFresh(String id,InputStream incoming)throws Exception {
            check.run();
            android.os.ParcelFileDescriptor file=resolver.openFileDescriptor(uri(id),"rw",signal);
            if(file==null)throw new IOException("Device file cannot be opened for writing");
            try{
                android.system.StructStat stat=android.system.Os.fstat(file.getFileDescriptor());
                if(stat.st_size!=0)throw new IOException("Device destination already exists");
                long dev=stat.st_dev,ino=stat.st_ino;
                transferTo(file.getFileDescriptor(),incoming,check);
                android.system.StructStat after=android.system.Os.fstat(file.getFileDescriptor());
                if(after.st_dev!=dev||after.st_ino!=ino)throw new IOException("Device destination changed during the write; it was not overwritten");
            }catch(android.system.ErrnoException problem){
                throw new IOException(problem.getMessage(),problem);
            }finally{file.close();}
        }

        private boolean replaceWithSibling(String id,InputStream incoming)throws Exception {
            String parent=parentDocument(id),name=displayName(id);
            if(parent==null||name==null)return false;
            try{leaf(name);}catch(IOException problem){return false;}
            String listed;
            try{listed=child(parent,name);}
            catch(Exception problem){if(cancelled(problem))throw problem;return false;}
            if(!id.equals(listed))return false;
            boolean createSupported,renameSupported;
            try{
                createSupported=documentSupports(parent,DocumentsContract.Document.FLAG_DIR_SUPPORTS_CREATE);
                renameSupported=documentSupports(id,DocumentsContract.Document.FLAG_SUPPORTS_RENAME);
            }catch(Exception problem){if(cancelled(problem))throw problem;return false;}
            if(!createSupported||!renameSupported)return false;
            try{if(!replaceableFile(id))return false;}
            catch(Exception problem){if(cancelled(problem))throw problem;return false;}
            NodeId original;
            try{original=nodeId(id);}
            catch(Exception problem){if(cancelled(problem))throw problem;return false;}
            String mime=documentMime(id);
            if(DocumentsContract.Document.MIME_TYPE_DIR.equals(mime))return false;
            String siblingName=null,sibling=null;
            for(int attempt=0;attempt<8&&sibling==null;attempt++){
                String candidate="w"+android.system.Os.getpid()+"-"+System.nanoTime();
                if(candidate.equals(name)||candidate.getBytes(StandardCharsets.UTF_8).length>MAX_NAME_BYTES)continue;
                try{leaf(candidate);}catch(IOException problem){continue;}
                try{if(child(parent,candidate)!=null)continue;}
                catch(Exception problem){if(cancelled(problem))throw problem;continue;}
                try{sibling=create(parent,candidate,mime);siblingName=candidate;}
                catch(Exception problem){
                    if(cancelled(problem)){quietRelease(childOrNull(parent,candidate));throw problem;}
                    String message=problem.getMessage();
                    if(message!=null&&message.startsWith("Device destination already exists"))continue;
                    quietRelease(childOrNull(parent,candidate));
                    return false;
                }
            }
            if(sibling==null)return false;
            try{if(!documentSupports(sibling,DocumentsContract.Document.FLAG_SUPPORTS_RENAME)){quietRelease(sibling);return false;}}
            catch(Exception problem){quietRelease(sibling);throw problem;}
            try{if(nodeId(sibling).size!=0){quietRelease(sibling);return false;}}
            catch(Exception problem){quietRelease(sibling);if(cancelled(problem))throw problem;return false;}
            try{writeCompleted(sibling,incoming);}
            catch(IOException problem){
                if(problem.getMessage()!=null&&problem.getMessage().startsWith("Device destination already exists")){quietRelease(sibling);return false;}
                quietRelease(sibling);
                throw problem;
            }
            NodeId now=nodeId(id);
            if(now.dev!=original.dev||now.ino!=original.ino){
                quietRelease(sibling);
                throw new IOException("Device destination changed during the write; it was not overwritten");
            }
            String parkedName=unusedName(parent,name,siblingName);
            if(parkedName==null){
                quietRelease(sibling);
                throw new IOException("Device file was left unchanged");
            }
            String parked;
            try{parked=renameConfirmed(id,parent,parkedName);}
            catch(Exception problem){
                boolean restored=recoverOriginal(parent,name,sibling,siblingName,id,parkedName,original);
                if(restored||nodeKind(quietChild(parent,name),original)==1){
                    if(cancelled(problem))throw problem;
                    if(problem instanceof IOException){
                        IOException io=(IOException)problem;
                        if(io.getMessage()!=null&&io.getMessage().contains("left unchanged"))throw io;
                        throw new IOException(leftUnchanged(io),io);
                    }
                    throw new IOException("Device file was left unchanged. "+(problem.getMessage()==null?"":problem.getMessage()),problem);
                }
                quietRelease(sibling);
                throw new IOException("The original remains as "+parkedName+" and was not deleted. "+(problem.getMessage()==null?"":problem.getMessage()),problem);
            }
            try{
                NodeId moved=nodeId(parked);
                if(moved.dev!=original.dev||moved.ino!=original.ino)throw new IOException("Device destination changed during the write; it was not overwritten");
                if(child(parent,name)!=null)throw new IOException("Device destination changed during the write; it was not overwritten");
                renameConfirmed(sibling,parent,name);
            }catch(Exception problem){
                boolean restored=recoverOriginal(parent,name,sibling,siblingName,parked,parkedName,original);
                if(restored){
                    if(cancelled(problem))throw problem;
                    if(problem instanceof IOException){
                        IOException io=(IOException)problem;
                        if(io.getMessage()!=null&&(io.getMessage().contains("left unchanged")||io.getMessage().contains("was not overwritten")))throw io;
                        throw new IOException(leftUnchanged(io),io);
                    }
                    throw new IOException("Device file was left unchanged. "+(problem.getMessage()==null?"":problem.getMessage()),problem);
                }
                throw new IOException("The original remains as "+parkedName+" and was not deleted. "+(problem.getMessage()==null?"":problem.getMessage()),problem);
            }
            try{if(!releaseDocument(parked,original))throw new IOException("Wrote the device file but the previous contents remain as "+parkedName);}
            catch(Exception problem){
                if(problem instanceof IOException&&problem.getMessage()!=null&&problem.getMessage().startsWith("Wrote the device file"))throw problem;
                throw new IOException("Wrote the device file but the previous contents remain as "+parkedName+". "+(problem.getMessage()==null?"":problem.getMessage()),problem);
            }
            return true;
        }

        private String unusedName(String parent,String avoidA,String avoidB)throws Exception {
            for(int attempt=0;attempt<8;attempt++){
                String candidate="w"+android.system.Os.getpid()+"-"+System.nanoTime();
                if(candidate.equals(avoidA)||candidate.equals(avoidB)||candidate.getBytes(StandardCharsets.UTF_8).length>MAX_NAME_BYTES)continue;
                try{leaf(candidate);}catch(IOException problem){continue;}
                try{if(child(parent,candidate)!=null)continue;}
                catch(Exception problem){if(cancelled(problem))throw problem;continue;}
                return candidate;
            }
            return null;
        }

        private String renameConfirmed(String id,String parent,String name)throws Exception {
            check.run();
            Uri renamed;
            try{renamed=DocumentsContract.renameDocument(resolver,uri(id),name);}
            catch(Exception problem){throw new IOException(problem.getMessage()==null?"Device file could not be renamed":problem.getMessage(),problem);}
            String placed=id;
            if(renamed!=null){
                if(!tree.getAuthority().equals(renamed.getAuthority()))throw new IOException("The document provider could not confirm the result");
                placed=DocumentsContract.getDocumentId(renamed);
                if(placed==null)throw new IOException("The document provider could not confirm the result");
            }
            if(!placed.equals(child(parent,name)))throw new IOException("The document provider did not confirm the requested destination");
            return placed;
        }

        private int nodeKind(String id,NodeId original){
            if(id==null||original==null)return -1;
            try{
                NodeId now=nodeId(id,false);
                return now.dev==original.dev&&now.ino==original.ino?1:0;
            }catch(Exception problem){return -1;}
        }

        private String quietChild(String parent,String name){
            try{
                String match=null;
                try(Cursor cursor=children(parent,false)){
                    while(cursor.moveToNext()){
                        if(!name.equals(cursor.getString(1)))continue;
                        if(match!=null)return null;
                        match=cursor.getString(0);
                    }
                }
                return match;
            }catch(Exception problem){return null;}
        }

        private String quietUnused(String parent,String avoidA,String avoidB){
            for(int attempt=0;attempt<8;attempt++){
                String candidate="w"+android.system.Os.getpid()+"-"+System.nanoTime();
                if(candidate.equals(avoidA)||candidate.equals(avoidB)||candidate.getBytes(StandardCharsets.UTF_8).length>MAX_NAME_BYTES)continue;
                try{leaf(candidate);}catch(IOException problem){continue;}
                if(quietChild(parent,candidate)!=null)continue;
                return candidate;
            }
            return null;
        }

        private String quietRename(String id,String parent,String name)throws Exception {
            Uri renamed=DocumentsContract.renameDocument(resolver,uri(id),name);
            String placed=id;
            if(renamed!=null){
                if(!tree.getAuthority().equals(renamed.getAuthority()))throw new IOException("The document provider could not confirm the result");
                placed=DocumentsContract.getDocumentId(renamed);
                if(placed==null)throw new IOException("The document provider could not confirm the result");
            }
            String listed=quietChild(parent,name);
            if(listed==null||!listed.equals(placed))throw new IOException("The document provider did not confirm the requested destination");
            return placed;
        }

        // Puts the original document back on its name and removes the unpublished replacement.
        // A document whose identity is unknown is left in place.
        private boolean recoverOriginal(String parent,String name,String sibling,String siblingName,String parked,String parkedName,NodeId original){
            try{
                String atName=quietChild(parent,name);
                if(atName!=null){
                    int kind=nodeKind(atName,original);
                    if(kind==1){
                        String leftover=quietChild(parent,siblingName);
                        if(leftover!=null&&(leftover.equals(sibling)||nodeKind(leftover,original)==0))quietRelease(leftover);
                        String extra=quietChild(parent,parkedName);
                        if(extra!=null&&!extra.equals(atName)&&nodeKind(extra,original)==1)quietRelease(extra);
                        return nodeKind(quietChild(parent,name),original)==1;
                    }
                    if(kind!=0||!atName.equals(sibling))return false;
                    String away=quietUnused(parent,name,parkedName);
                    if(away==null)return false;
                    quietRelease(quietRename(atName,parent,away));
                }
                if(quietChild(parent,name)!=null)return false;
                String held=quietChild(parent,parkedName);
                if(held==null&&nodeKind(parked,original)==1)held=parked;
                if(held==null||nodeKind(held,original)!=1)return false;
                String placed=quietRename(held,parent,name);
                if(!placed.equals(quietChild(parent,name))||nodeKind(placed,original)!=1)return false;
                String leftover=quietChild(parent,siblingName);
                if(leftover!=null&&(leftover.equals(sibling)||nodeKind(leftover,original)==0))quietRelease(leftover);
                return true;
            }catch(Exception problem){return false;}
        }

        private static File snapshotRoot(){
            File[] roots=holdingDirectories;
            if(roots==null)return null;
            for(File root:roots){
                if(root==null)continue;
                if(!root.isDirectory())root.mkdirs();
                if(root.isDirectory())return root;
            }
            return null;
        }

        private File copyOriginal(String id)throws Exception {
            File root=snapshotRoot();
            if(root==null)return null;
            String token="w"+android.system.Os.getpid()+"-"+System.nanoTime();
            HeldDirectory directory=openDirectory(root);
            FileDescriptor fd=null;
            boolean keep=false;
            try{
                fd=openRelative(directory,token,android.system.OsConstants.O_WRONLY|android.system.OsConstants.O_CREAT|android.system.OsConstants.O_EXCL,0600);
                try(InputStream in=input(id)){transferTo(fd,in,check);}
                android.system.Os.fsync(fd);
                keep=true;
                return new File(root,token);
            }catch(Exception problem){
                try{android.system.Os.remove(directory.childPath(token));}catch(android.system.ErrnoException ignored){}
                throw problem;
            }finally{
                if(fd!=null)try{android.system.Os.close(fd);}catch(android.system.ErrnoException ignored){}
                directory.quiet();
                if(!keep)discardSnapshot(new File(root,token));
            }
        }

        private byte[] copyOriginalBytes(String id)throws Exception {
            ByteArrayOutputStream out=new ByteArrayOutputStream();
            try(InputStream in=input(id)){transfer(in,out,check);}
            return out.toByteArray();
        }

        private void restoreTruncated(String id,File saved,byte[] savedBytes)throws Exception {
            android.os.ParcelFileDescriptor file=resolver.openFileDescriptor(uri(id),"wt",null);
            if(file==null)throw new IOException("Device file could not be restored");
            Check quiet=()->{};
            try(InputStream in=saved!=null?new FileInputStream(saved):new ByteArrayInputStream(savedBytes==null?new byte[0]:savedBytes)){
                transferTo(file.getFileDescriptor(),in,quiet);
                file.getFileDescriptor().sync();
            }finally{file.close();}
        }

        private static void discardSnapshot(File saved){
            if(saved==null)return;
            try{
                File parent=saved.getParentFile();
                if(parent==null)return;
                HeldDirectory directory=openDirectory(parent);
                try{android.system.Os.remove(directory.childPath(saved.getName()));}
                catch(android.system.ErrnoException ignored){}
                finally{directory.quiet();}
            }catch(Exception ignored){}
        }

        private void writeCompleted(String id,InputStream incoming)throws Exception {
            check.run();
            android.os.ParcelFileDescriptor file=resolver.openFileDescriptor(uri(id),"rw",signal);
            if(file==null)throw new IOException("Device file cannot be opened for writing");
            try{
                android.system.StructStat stat=android.system.Os.fstat(file.getFileDescriptor());
                if(stat.st_size!=0)throw new IOException("Device destination already exists");
                long dev=stat.st_dev,ino=stat.st_ino;
                transferTo(file.getFileDescriptor(),incoming,check);
                android.system.StructStat after=android.system.Os.fstat(file.getFileDescriptor());
                if(after.st_dev!=dev||after.st_ino!=ino)throw new IOException("Device destination changed during the write; it was not overwritten");
                file.getFileDescriptor().sync();
            }catch(android.system.ErrnoException problem){
                throw new IOException(problem.getMessage(),problem);
            }finally{file.close();}
        }

        private boolean replaceableFile(String id)throws Exception {
            if(DocumentsContract.Document.MIME_TYPE_DIR.equals(documentMime(id)))return false;
            check.run();
            android.os.ParcelFileDescriptor file=resolver.openFileDescriptor(uri(id),"r",signal);
            if(file==null)return false;
            try{
                android.system.StructStat stat=android.system.Os.fstat(file.getFileDescriptor());
                if((stat.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFREG)return false;
                String linked=android.system.Os.readlink("/proc/self/fd/"+file.getFd());
                return linked!=null&&linked.startsWith("/")&&!linked.startsWith("/proc/")&&!linked.contains(" (deleted)")&&new File(linked).getParentFile()!=null;
            }catch(android.system.ErrnoException problem){return false;}
            finally{file.close();}
        }

        private String displayName(String id)throws Exception {
            check.run();
            try(Cursor cursor=resolver.query(uri(id),new String[]{DocumentsContract.Document.COLUMN_DISPLAY_NAME},null,null,null,signal)){
                if(cursor==null||!cursor.moveToFirst()||cursor.isNull(0))return null;
                String value=cursor.getString(0);
                return value==null||value.isEmpty()?null:value;
            }
        }

        private static String parentDocument(String id){
            if(id==null)return null;
            int colon=id.indexOf(':');
            if(colon<=0||colon>=id.length()-1)return null;
            String path=id.substring(colon+1);
            int slash=path.lastIndexOf('/');
            if(slash<0)return id.substring(0,colon+1);
            if(slash==0)return null;
            return id.substring(0,colon+1)+path.substring(0,slash);
        }

        private String childOrNull(String parent,String name){try{return child(parent,name);}catch(Exception ignored){return null;}}

        private void quietRelease(String id){
            if(id==null)return;
            try{releaseDocument(id,nodeId(id,false),false);}catch(Exception ignored){}
        }

        String rename(String id,String parent,String name)throws Exception {
            leaf(name);
            if(child(parent,name)!=null)throw new IOException("Device destination already exists");
            check.run();
            String created=transferExclusive(id,parent,name);
            if(!created.equals(child(parent,name)))throw new IOException("The document provider did not confirm the requested destination");
            return created;
        }

        String move(String id,String parent,String sourceName,String targetParent,String name)throws Exception {
            leaf(sourceName);leaf(name);
            if(targetParent.equals(parent)&&name.equals(sourceName))throw new IOException("Choose a destination outside the source");
            if(child(targetParent,name)!=null)throw new IOException("Device destination already exists");
            check.run();
            if(moveWholeFolder(id,parent,sourceName,targetParent,name))return "by moving the folder in one step";
            String created=transferExclusive(id,targetParent,name);
            if(!created.equals(child(targetParent,name)))throw new IOException("The document provider did not confirm the requested destination");
            if(id.equals(child(parent,sourceName))||(!name.equals(sourceName)&&id.equals(child(parent,name))))
                throw new IOException("Copied into the destination as "+name+", but the original remains");
            return "by copying into the selected folder and removing the original after verification";
        }

        // One provider move takes the folder and every child with it. A different leaf name is applied
        // only when that provider can rename, and only after the moved id is confirmed.
        private boolean moveWholeFolder(String id,String parent,String sourceName,String targetParent,String name)throws Exception {
            if(targetParent.equals(parent))return false;
            if(!DocumentsContract.Document.MIME_TYPE_DIR.equals(documentMime(id)))return false;
            if(!documentSupports(id,DocumentsContract.Document.FLAG_SUPPORTS_MOVE))return false;
            boolean sameName=name.equals(sourceName);
            if(!sameName){
                if(!documentSupports(id,DocumentsContract.Document.FLAG_SUPPORTS_RENAME))return false;
                if(child(targetParent,sourceName)!=null)return false;
            }
            refuseNestedDocument(id,targetParent);
            check.run();
            Uri moved;
            try{moved=DocumentsContract.moveDocument(resolver,uri(id),uri(parent),uri(targetParent));}
            catch(Exception problem){throw new IOException("The document provider could not move the folder. "+providerText(problem),problem);}
            if(moved==null||!tree.getAuthority().equals(moved.getAuthority()))throw new IOException("The document provider did not confirm the requested destination");
            String movedId=DocumentsContract.getDocumentId(moved);
            if(movedId==null||!movedId.equals(child(targetParent,sourceName)))throw new IOException("The document provider did not confirm the requested destination");
            if(!sameName){
                Uri renamed;
                try{renamed=DocumentsContract.renameDocument(resolver,uri(movedId),name);}
                catch(Exception problem){throw new IOException("Moved into the destination as "+sourceName+", but the name was not changed to "+name+". "+providerText(problem),problem);}
                if(renamed==null||!tree.getAuthority().equals(renamed.getAuthority()))throw new IOException("Moved into the destination as "+sourceName+", but the name was not changed to "+name);
                movedId=DocumentsContract.getDocumentId(renamed);
                if(movedId==null||!movedId.equals(child(targetParent,name))||movedId.equals(child(targetParent,sourceName)))
                    throw new IOException("Moved into the destination as "+sourceName+", but the name was not changed to "+name);
            }
            if(id.equals(child(parent,sourceName))||(!sameName&&id.equals(child(parent,name))))
                throw new IOException("The folder was moved into the destination, but the original remains");
            return true;
        }

        private static String providerText(Exception problem){
            String message=problem.getMessage();
            return message==null||message.isEmpty()?"The folder was not confirmed at the destination":message;
        }

        String copy(String id,String parent,String sourceName,String targetParent,String name)throws Exception {
            leaf(sourceName);leaf(name);
            if(targetParent.equals(parent)&&name.equals(sourceName))throw new IOException("Choose a destination outside the source");
            if(child(targetParent,name)!=null)throw new IOException("Device destination already exists");
            check.run();
            String created=transferExclusive(id,targetParent,name,true);
            if(!created.equals(child(targetParent,name)))throw new IOException("The document provider did not confirm the requested destination");
            if(child(parent,sourceName)==null)throw new IOException("Device copy removed the original");
            return created;
        }

        private static final class NodeId {
            final long dev,ino,size;
            NodeId(long dev,long ino,long size){this.dev=dev;this.ino=ino;this.size=size;}
        }

        private static final class Made {
            final String id; final boolean directory; final NodeId node; final String name;
            Made(String id,boolean directory,NodeId node){this(id,directory,node,null);}
            Made(String id,boolean directory,NodeId node,String name){this.id=id;this.directory=directory;this.node=node;this.name=name;}
        }

        private void refuseNestedDocument(String sourceId,String targetParent)throws Exception {
            if(sourceId.equals(targetParent)||documentPathInside(sourceId,targetParent))throw new IOException("Choose a destination outside the source");
            try{
                if(DocumentsContract.isChildDocument(resolver,uri(sourceId),uri(targetParent)))throw new IOException("Choose a destination outside the source");
            }catch(IOException problem){
                if(problem.getMessage()!=null&&problem.getMessage().startsWith("Choose a destination"))throw problem;
                throw new IOException("Device copy was refused because the destination could not be confirmed outside the source",problem);
            }catch(RuntimeException problem){
                throw new IOException("Device copy was refused because the destination could not be confirmed outside the source",problem);
            }
        }

        private static boolean documentPathInside(String sourceId,String otherId){
            if(sourceId==null||otherId==null||sourceId.isEmpty()||otherId.isEmpty()||otherId.equals(sourceId))return otherId!=null&&otherId.equals(sourceId);
            int sourceColon=sourceId.indexOf(':'),otherColon=otherId.indexOf(':');
            if(sourceColon<=0||sourceColon!=otherColon||!sourceId.regionMatches(0,otherId,0,sourceColon+1))return false;
            String sourcePath=sourceId.substring(sourceColon+1),otherPath=otherId.substring(otherColon+1);
            if(sourcePath.isEmpty())return true;
            return otherPath.equals(sourcePath)||otherPath.startsWith(sourcePath+"/");
        }

        private String transferExclusive(String id,String targetParent,String name)throws Exception {
            return transferExclusive(id,targetParent,name,false);
        }

        private String transferExclusive(String id,String targetParent,String name,boolean retainSource)throws Exception {
            if(child(targetParent,name)!=null)throw new IOException("Device destination already exists");
            check.run();
            String mime=documentMime(id);
            boolean directory=DocumentsContract.Document.MIME_TYPE_DIR.equals(mime);
            if(directory)refuseNestedDocument(id,targetParent);
            ArrayList<Made> made=new ArrayList<>();
            ArrayList<Made> sources=new ArrayList<>();
            boolean committed=false;
            try{
                String claimed=create(targetParent,name,directory?DocumentsContract.Document.MIME_TYPE_DIR:mime);
                made.add(new Made(claimed,directory,null));
                if(directory){
                    copyDocumentTree(id,claimed,made,sources,new MoveWalk(check),1);
                    sources.add(new Made(id,true,null,name));
                }else{
                    NodeId source=nodeId(id);
                    NodeId dest=nodeId(claimed);
                    made.set(made.size()-1,new Made(claimed,false,dest));
                    if(dest.size!=0)throw new IOException("Device destination already exists");
                    copyOntoClaim(id,claimed,source,dest);
                    sources.add(new Made(id,false,source,name));
                }
                committed=true;
                if(!retainSource)deleteSources(sources);
                return claimed;
            }catch(StackOverflowError overflow){
                if(!committed)discardMade(made);
                throw new IOException("Device directory move exceeds 64 nested directories",overflow);
            }catch(Exception problem){
                if(committed)throw problem;
                boolean removed=discardMade(made);
                String message=problem.getMessage()==null?"Device rename could not finish":problem.getMessage();
                String tail=discardTail(made,removed);
                if(message.startsWith("Device destination already exists")||message.startsWith("The document provider did not confirm"))
                    throw new IOException(message+tail,problem);
                throw new IOException(message+tail,problem);
            }
        }

        private void copyDocumentTree(String sourceId,String destId,ArrayList<Made> made,ArrayList<Made> sources,MoveWalk walk,int depth)throws Exception {
            walk.depth(depth);
            try(Cursor cursor=children(sourceId)){
                while(cursor.moveToNext()){
                    String childId=cursor.getString(0),childName=cursor.getString(1),childMime=cursor.getString(2);
                    if(childId==null||childName==null)throw new IOException("Invalid device directory entry");
                    walk.entry(childName);
                    leaf(childName);
                    if(childId.equals(destId))throw new IOException("Choose a destination outside the source");
                    if(DocumentsContract.Document.MIME_TYPE_DIR.equals(childMime)){
                        String sub=create(destId,childName,DocumentsContract.Document.MIME_TYPE_DIR);
                        made.add(new Made(sub,true,null));
                        copyDocumentTree(childId,sub,made,sources,walk,depth+1);
                        sources.add(new Made(childId,true,null,childName));
                    }else{
                        NodeId source=nodeId(childId);
                        String mime=childMime==null||childMime.isEmpty()?"application/octet-stream":childMime;
                        String sub=create(destId,childName,mime);
                        made.add(new Made(sub,false,null));
                        NodeId dest=nodeId(sub);
                        made.set(made.size()-1,new Made(sub,false,dest));
                        if(dest.size!=0)throw new IOException("Device destination already exists");
                        copyOntoClaim(childId,sub,source,dest);
                        sources.add(new Made(childId,false,source,childName));
                    }
                }
            }
        }

        private void copyOntoClaim(String sourceId,String destId,NodeId source,NodeId dest)throws Exception {
            check.run();
            android.os.ParcelFileDescriptor out=resolver.openFileDescriptor(uri(destId),"rw",signal);
            if(out==null)throw new IOException("Device file cannot be opened for writing");
            try{
                android.system.StructStat stat=android.system.Os.fstat(out.getFileDescriptor());
                if(stat.st_dev!=dest.dev||stat.st_ino!=dest.ino||stat.st_size!=0)
                    throw new IOException("Device destination changed during the move; it was not overwritten");
                try(InputStream in=input(sourceId)){transferTo(out.getFileDescriptor(),in,check);}
                android.system.StructStat after=android.system.Os.fstat(out.getFileDescriptor());
                if(after.st_dev!=dest.dev||after.st_ino!=dest.ino)
                    throw new IOException("Device destination changed during the move; it was not overwritten");
            }catch(android.system.ErrnoException problem){
                throw new IOException(problem.getMessage(),problem);
            }finally{out.close();}
            JSONObject expected,actual;
            try(InputStream in=input(sourceId)){expected=fingerprint(in,check);}
            try(InputStream in=input(destId)){actual=fingerprint(in,check);}
            if(expected.getLong("bytes")!=actual.getLong("bytes")||!expected.getString("sha256").equals(actual.getString("sha256")))
                throw new IOException("Device copy differs from the source; original kept");
            NodeId sourceNow=nodeId(sourceId);
            if(sourceNow.dev!=source.dev||sourceNow.ino!=source.ino)
                throw new IOException("Device source changed during the copy; original kept");
        }

        private NodeId nodeId(String id)throws Exception { return nodeId(id,true); }

        private NodeId nodeId(String id,boolean observe)throws Exception {
            if(observe)check.run();
            android.os.ParcelFileDescriptor file=resolver.openFileDescriptor(uri(id),"r",observe?signal:null);
            if(file==null)throw new IOException("Device file is unavailable");
            try{
                android.system.StructStat stat=android.system.Os.fstat(file.getFileDescriptor());
                return new NodeId(stat.st_dev,stat.st_ino,stat.st_size);
            }catch(android.system.ErrnoException problem){
                throw new IOException(problem.getMessage(),problem);
            }finally{file.close();}
        }

        private void deleteSources(ArrayList<Made> sources)throws Exception {
            // Files go first, by identity. Directories were recorded children-before-parent,
            // so the following pass removes emptied directories from the bottom up.
            // DocumentsContract.deleteDocument is not used: that provider delete removes children recursively.
            ArrayList<String> moved=new ArrayList<>();
            ArrayList<Made> directories=new ArrayList<>();
            for(int index=0;index<sources.size();index++){
                Made item=sources.get(index);
                if(item.directory){directories.add(item);continue;}
                try{deleteMatchedDocument(item.id,item.node);}
                catch(Exception problem){throw new IOException(movedNote(problem.getMessage(),moved),problem);}
                moved.add(movedName(item));
            }
            for(int index=0;index<directories.size();index++){
                Made item=directories.get(index);
                try{removeEmptySourceDirectory(item.id);}
                catch(Exception problem){throw new IOException(movedNote(problem.getMessage(),moved),problem);}
                moved.add(movedName(item));
            }
        }

        private static String movedName(Made item){
            return item.name==null||item.name.isEmpty()?item.id:item.name;
        }

        private static String movedNote(String detail,ArrayList<String> moved){
            String message=detail==null||detail.isEmpty()?"Device move could not finish":detail;
            if(moved.isEmpty())return message;
            StringBuilder listed=new StringBuilder();
            for(int index=0;index<moved.size();index++){
                if(index>0)listed.append(", ");
                listed.append(moved.get(index));
            }
            if(!message.endsWith("."))message+=".";
            return message+" Entries already moved: "+listed;
        }

        private void deleteMatchedDocument(String id,NodeId expected)throws Exception {
            if(!releaseDocument(id,expected))throw new IOException("Copied into the destination, but the original remains");
        }

        private boolean releaseDocument(String id,NodeId expected)throws Exception {
            return releaseDocument(id,expected,true);
        }

        private boolean releaseDocument(String id,NodeId expected,boolean observe)throws Exception {
            if(expected==null)return false;
            NodeId now=nodeId(id,observe);
            if(now.dev!=expected.dev||now.ino!=expected.ino)return false;
            android.os.ParcelFileDescriptor file=resolver.openFileDescriptor(uri(id),"r",observe?signal:null);
            if(file==null)return false;
            HeldDirectory directory=null;
            try{
                android.system.StructStat opened=android.system.Os.fstat(file.getFileDescriptor());
                if(opened.st_dev!=expected.dev||opened.st_ino!=expected.ino)return false;
                String linked=android.system.Os.readlink("/proc/self/fd/"+file.getFd());
                if(linked==null||!linked.startsWith("/")||linked.startsWith("/proc/")||linked.contains(" (deleted)"))return false;
                File path=new File(linked);
                File parent=path.getParentFile();
                if(parent==null)return false;
                try{directory=openDirectory(parent);}catch(IOException problem){return false;}
                releaseMatched(directory,path.getName(),expected.dev,expected.ino);
                return true;
            }catch(android.system.ErrnoException problem){return false;}
            finally{if(directory!=null)directory.quiet();file.close();}
        }

        private void removeEmptySourceDirectory(String id)throws Exception {
            check.run();
            if(listedChild(id))throw new IOException("Device directory still contains entries; it was not removed");
            ParcelFileDescriptor file=null;
            HeldDirectory parent=null;
            try{
                file=resolver.openFileDescriptor(uri(id),"r",null);
                if(file==null)throw new IOException("Device directory was left in place");
                android.system.StructStat opened=android.system.Os.fstat(file.getFileDescriptor());
                if((opened.st_mode&android.system.OsConstants.S_IFMT)!=android.system.OsConstants.S_IFDIR)
                    throw new IOException("Device directory was left in place");
                String linked=android.system.Os.readlink("/proc/self/fd/"+file.getFd());
                if(linked==null||!linked.startsWith("/")||linked.startsWith("/proc/")||linked.contains(" (deleted)"))
                    throw new IOException("Device directory was left in place");
                File path=new File(linked);
                File folder=path.getParentFile();
                if(folder==null)throw new IOException("Device directory was left in place");
                check.run();
                if(listedChild(id))throw new IOException("Device directory still contains entries; it was not removed");
                parent=openDirectory(folder);
                releaseMatched(parent,path.getName(),opened.st_dev,opened.st_ino);
            }catch(IOException problem){
                String message=problem.getMessage();
                if(message!=null&&message.startsWith("Device "))throw problem;
                throw new IOException("Device directory was left in place",problem);
            }catch(Exception problem){
                throw new IOException("Device directory was left in place",problem);
            }finally{
                if(parent!=null)parent.quiet();
                if(file!=null)try{file.close();}catch(IOException ignored){}
            }
        }

        private boolean listedChild(String id)throws Exception {
            try(Cursor cursor=children(id,false)){return cursor.moveToNext();}
        }

        private boolean discardMade(ArrayList<Made> made){
            boolean removed=true;
            for(int index=made.size()-1;index>=0;index--)if(!discardOne(made.get(index)))removed=false;
            return removed;
        }

        private static String discardTail(ArrayList<Made> made,boolean removed){
            if(made.isEmpty())return "";
            if(removed)return " The incomplete destination was removed.";
            for(int index=0;index<made.size();index++)if(made.get(index).directory)return " A destination directory was left in place.";
            return " An incomplete destination may remain.";
        }

        private boolean discardOne(Made item){
            try{
                if(item.directory){
                    try(Cursor cursor=children(item.id,false)){if(cursor.moveToNext())return false;}
                    return false;
                }
                if(item.node==null)return false;
                return releaseDocument(item.id,item.node,false);
            }catch(Exception ignored){return false;}
        }

        JSONObject describe(String id)throws Exception {
            check.run();
            String mime="application/octet-stream";
            long modified=0;
            boolean timed=false;
            Long listedSize=null;
            try(Cursor cursor=resolver.query(uri(id),new String[]{DocumentsContract.Document.COLUMN_MIME_TYPE,DocumentsContract.Document.COLUMN_LAST_MODIFIED,DocumentsContract.Document.COLUMN_SIZE},null,null,null,signal)){
                if(cursor!=null&&cursor.moveToFirst()){
                    if(!cursor.isNull(0))mime=cursor.getString(0);
                    if(!cursor.isNull(1)){modified=cursor.getLong(1);timed=true;if(modified>100000000000L)modified/=1000L;}
                    if(!cursor.isNull(2))listedSize=cursor.getLong(2);
                }
            }
            boolean directory=DocumentsContract.Document.MIME_TYPE_DIR.equals(mime);
            if(directory)return new JSONObject().put("kind","directory").put("bytes",listedSize==null?0:listedSize.longValue()).put("modified",timed?modified:0);
            android.os.ParcelFileDescriptor file=resolver.openFileDescriptor(uri(id),"r",signal);
            if(file==null)throw new IOException("Device file is unavailable");
            try{
                android.system.StructStat stat=android.system.Os.fstat(file.getFileDescriptor());
                return new JSONObject().put("kind","file").put("bytes",stat.st_size).put("modified",timed?modified:stat.st_mtime);
            }catch(android.system.ErrnoException problem){
                throw new IOException(problem.getMessage(),problem);
            }finally{file.close();}
        }

        private String documentMime(String id)throws Exception {
            check.run();
            try(Cursor cursor=resolver.query(uri(id),new String[]{DocumentsContract.Document.COLUMN_MIME_TYPE},null,null,null,signal)){
                if(cursor==null||!cursor.moveToFirst()||cursor.isNull(0))return "application/octet-stream";
                String mime=cursor.getString(0);
                return mime==null||mime.isEmpty()?"application/octet-stream":mime;
            }
        }

        private boolean documentSupports(String id,int flag)throws Exception {
            check.run();
            try(Cursor cursor=resolver.query(uri(id),new String[]{DocumentsContract.Document.COLUMN_FLAGS},null,null,null,signal)){
                if(cursor==null||!cursor.moveToFirst()||cursor.isNull(0))return false;
                return (cursor.getInt(0)&flag)!=0;
            }
        }

        String review(String id,String parent,String name)throws Exception {
            if(parent.equals(child(root,"review")))throw new IOException("Device file is already in review");
            String reviewFolder=child(root,"review");
            if(reviewFolder==null)reviewFolder=create(root,"review",DocumentsContract.Document.MIME_TYPE_DIR);
            if(reviewFolder.equals(id))throw new IOException("Device review destination already exists");
            long stamp=System.currentTimeMillis();
            IOException collision=null;
            for(int counter=-1;counter<1000;counter++){
                String leaf=counter<0?name:reviewLeaf(name,stamp,counter);
                if(counter>=0&&leaf.getBytes(StandardCharsets.UTF_8).length>MAX_NAME_BYTES)
                    throw new IOException("Device review name exceeds 255 bytes");
                leaf(leaf);
                if(child(reviewFolder,leaf)!=null)continue;
                check.run();
                try{
                    String moved=transferExclusive(id,reviewFolder,leaf);
                    if(!moved.equals(child(reviewFolder,leaf)))throw new IOException("The document provider did not confirm the requested destination");
                    if(id.equals(child(parent,name))||id.equals(child(parent,leaf)))
                        throw new IOException("The document provider did not confirm removal from the original folder");
                    return leaf;
                }catch(IOException problem){
                    String message=problem.getMessage();
                    if(message!=null&&message.startsWith("Device destination already exists")){collision=problem;continue;}
                    throw problem;
                }
            }
            throw collision==null?new IOException("Device review destination already exists"):collision;
        }
    }

    static File findSu() {
        LinkedHashSet<String> directories=new LinkedHashSet<>();
        for(String directory:new String[]{"/system/bin","/system/xbin","/sbin","/su/bin","/debug_ramdisk","/vendor/bin","/vendor/xbin","/product/bin","/odm/bin"})
            directories.add(directory);
        String path=System.getenv("PATH");
        if(path!=null)for(String directory:path.split(":"))if(directory.startsWith("/"))directories.add(directory);
        for(String directory:directories){
            File candidate=new File(directory,"su");
            if(candidate.isFile()&&candidate.canExecute())return candidate;
        }
        return null;
    }

    private static String quote(File path) { return quoteText(path.getPath()); }

    private static String quoteText(String path) { return "'"+path.replace("'","'\"'\"'")+"'"; }

    private static String rootMarker(InputStream in)throws IOException {
        ByteArrayOutputStream out=new ByteArrayOutputStream();
        byte[] buffer=new byte[256];
        int n;
        while((n=in.read(buffer))>=0){
            if(out.size()+n>4096)throw new IOException("Device root move did not report a result");
            out.write(buffer,0,n);
        }
        return out.toString(StandardCharsets.UTF_8.name()).trim();
    }

    private static String destinationIdentity(String text){return taggedIdentity(text,"dest");}

    private static String sourceIdentity(String text){return taggedIdentity(text,"src");}

    private static String taggedIdentity(String text,String label){
        String prefix=label+" ",found=null;
        if(text==null)return null;
        for(String line:text.split("\n")){
            line=line.trim();
            if(!line.startsWith(prefix))continue;
            String id=line.substring(prefix.length()).trim();
            if(id.matches("[0-9]+:[0-9]+"))found=id;
        }
        return found;
    }

    private static String markerOf(String text)throws IOException {
        String marker=null;
        if(text!=null)for(String line:text.split("\n")){
            line=line.trim();
            if(line.isEmpty()||line.startsWith("dest ")||line.startsWith("src "))continue;
            marker=line;
        }
        if(marker==null)throw new IOException("Device root move did not report a result");
        return marker;
    }

    static final class Root {
        private final File su;
        private final ScheduledExecutorService deadlines;
        private final CancellationSignal signal;
        Root(File su,ScheduledExecutorService deadlines,CancellationSignal signal){
            this.su=su;this.deadlines=deadlines;this.signal=signal;
        }

        // The program is selected internally. Every path is one quoted positional argument.
        private <T> T run(String program,InputStream bytes,RootReader<T> reader,File... paths)throws Exception {
            signal.throwIfCanceled();
            program="test $(/system/bin/id -u) = 0 || exit 77; "+program;
            StringBuilder command=new StringBuilder("/system/bin/sh -c '")
                .append(program.replace("'","'\"'\"'")).append("' device");
            for(File path:paths)command.append(' ').append(quote(path));
            command.append(' ').append(quoteText(holdingArgument()));
            Process process=new ProcessBuilder(su.getPath(),"-c",command.toString()).start();
            Thread drain=new Thread(()->{try{byte[] sink=new byte[4096];InputStream err=process.getErrorStream();while(err.read(sink)>=0){}}catch(IOException ignored){}},"device-root-error");
            drain.setDaemon(true);drain.start();
            ScheduledFuture<?> deadline=null;
            try{
                deadline=deadlines.schedule(()->process.destroyForcibly(),120,TimeUnit.SECONDS);
                signal.setOnCancelListener(()->process.destroyForcibly());
                try(OutputStream stdin=process.getOutputStream()){
                    if(bytes!=null)transfer(bytes,stdin,signal::throwIfCanceled);
                }
                T result;
                try(InputStream stdout=process.getInputStream()){result=reader.read(stdout);}
                int code=process.waitFor();
                signal.throwIfCanceled();
                if(code==77)throw new IOException("Device root permission was denied");
                if(code==81)throw new IOException("Device destination already exists");
                if(code==82)throw new IOException("Device path is a symbolic link");
                if(code==83)throw new IOException("Directory copy across volumes is not available; nothing was moved");
                if(code==84)throw new IOException("Device path is not a regular file or directory");
                if(code==85)throw new IOException("Device file exceeds the copy size limit");
                if(code==86)throw new IOException("Device directory move did not finish; source and destination may both contain entries");
                if(code==87)throw new IOException("Device directory move exceeds 64 nested directories");
                if(code==88)throw new IOException("Device directory move exceeds 100000 entries");
                if(code==89)throw new IOException("Device path changed before removal; it was left in place");
                if(code==90)throw new IOException("Device removal did not match the opened file; the shared path was not unlinked");
                if(code==92)throw new IOException("Device file could not be removed safely because no private directory is on the same volume; it was left in place");
                if(code==93)throw new IOException("Device path changed before removal; a concurrent file was kept and was not deleted"+(result instanceof String?" "+result:""));
                if(code==94)throw new IOException("Device directory listing could not be bounded");
                if(code==95)throw new IOException("Device copy differs from the source; original kept");
                if(code==96)throw new IOException("Device copy could not be verified; an unverified destination may remain");
                if(code!=0)throw new IOException("Device root command failed (exit "+code+")");
                return result;
            }finally{
                process.destroyForcibly();
                if(deadline!=null)deadline.cancel(false);
                signal.setOnCancelListener(null);
            }
        }

        void verify()throws Exception {
            run("exit 0",null,in->null);
        }

        File resolve(File file)throws Exception {
            String resolved=run("/system/bin/readlink -f -- \"$1\" && printf '\\000'",null,in->{
                ByteArrayOutputStream path=new ByteArrayOutputStream();
                int value;
                while((value=in.read())>0){
                    if(path.size()>=16384)throw new IOException("Invalid root path");
                    path.write(value);
                }
                if(value!=0)throw new IOException("Root path could not be resolved");
                return path.toString(StandardCharsets.UTF_8.name());
            },file);
            if(!resolved.endsWith("\n"))throw new IOException("Invalid resolved root path");
            return absolute(resolved.substring(0,resolved.length()-1));
        }

        JSONArray list(File folder,Check check)throws Exception {
            return run("test -d \"$1\" && test -r \"$1\" && test -x \"$1\" || exit 1; "
                +"exec /system/bin/find \"$1\" -mindepth 1 -maxdepth 1 -exec /system/bin/sh -c '"
                +"p=$1; if [ -d \"$p\" ] && [ ! -L \"$p\" ]; then t=d; else t=f; fi; "
                +"printf \"%s%s\\000\" \"$t\" \"${p##*/}\"' sh {} \\;",null,in->{
                JSONArray rows=new JSONArray();
                int[] used=new int[]{2};
                BufferedInputStream input=new BufferedInputStream(in);
                ByteArrayOutputStream name=new ByteArrayOutputStream();
                int value;
                while((value=input.read())!=-1){
                    if(value==0){
                        check.run();
                        String entry=name.toString(StandardCharsets.UTF_8.name());
                        if(entry.length()<2||(entry.charAt(0)!='d'&&entry.charAt(0)!='f'))throw new IOException("Invalid device directory entry");
                        addListed(rows,entry.substring(1),entry.charAt(0)=='d',5000,"Device folder exceeds 5000 entries",used);
                        name.reset();
                    }else{
                        if(name.size()>=MAX_ENTRY_CHARS)throw new IOException("Device folder entry name exceeds 1024 characters");
                        name.write(value);
                    }
                }
                if(name.size()!=0)throw new IOException("Incomplete device directory entry");
                return rows;
            },folder);
        }

        JSONObject fingerprint(File file,Check check)throws Exception {
            return run("test -f \"$1\" && exec /system/bin/cat -- \"$1\"",null,
                in->DocumentAccess.fingerprint(in,check),file);
        }

        void write(File file,InputStream bytes,boolean create)throws Exception {
            if(create){
                run("umask 077; set -C; exec /system/bin/cat > \"$1\"",bytes,in->null,file);
                return;
            }
            File parent=file.getParentFile();
            if(parent==null)throw new IOException("Choose a file inside a device folder");
            String candidate="w"+android.system.Os.getpid()+"-"+System.nanoTime();
            if(candidate.equals(file.getName()))throw new IOException("Device file could not be written beside the original");
            File temp=new File(parent,candidate);
            try{run(rootReplaceProgram(),bytes,in->null,file,temp);}
            catch(Exception problem){
                int code=exitCode(problem);
                boolean removed=code==91||removeRootTemp(temp);
                if(cancelled(problem))throw problem;
                IOException io=problem instanceof IOException?(IOException)problem:new IOException(problem.getMessage()==null?"Device root command failed":problem.getMessage(),problem);
                String text=code==91?"Device file could not be written beside the original. The original was left unchanged.":leftUnchanged(io);
                if(!removed&&!text.contains("may remain"))text=text+" A temporary file may remain.";
                throw new IOException(text,io);
            }
        }

        private boolean removeRootTemp(File temp){
            Process process=null;
            try{
                String program="test $(/system/bin/id -u) = 0 || exit 1; if [ -e \"$1\" ] || [ -L \"$1\" ]; then /system/bin/rm -f -- \"$1\" || exit 1; fi";
                StringBuilder command=new StringBuilder("/system/bin/sh -c '")
                    .append(program.replace("'","'\"'\"'")).append("' device ").append(quote(temp));
                process=new ProcessBuilder(su.getPath(),"-c",command.toString()).start();
                Process started=process;
                Thread drain=new Thread(()->{try{byte[] sink=new byte[4096];InputStream err=started.getErrorStream();while(err.read(sink)>=0){}}catch(IOException ignored){}},"device-root-error");
                drain.setDaemon(true);drain.start();
                try{process.getOutputStream().close();}catch(IOException ignored){}
                try{process.getInputStream().close();}catch(IOException ignored){}
                if(!process.waitFor(10,TimeUnit.SECONDS)){process.destroyForcibly();return false;}
                return process.exitValue()==0;
            }catch(Exception ignored){
                if(process!=null)process.destroyForcibly();
                return false;
            }
        }

        // The new bytes land in a sibling, are synced, and only then replace the target.
        // Any failure removes that sibling and leaves the target untouched.
        private static String rootReplaceProgram(){
            return "if [ -L \"$1\" ] || [ ! -f \"$1\" ]; then /system/bin/cat >/dev/null; exit 1; fi; "
                +"if [ \"$1\" = \"$2\" ]; then /system/bin/cat >/dev/null; exit 91; fi; "
                +"p1=$(/system/bin/dirname -- \"$1\") || { /system/bin/cat >/dev/null; exit 1; }; "
                +"p2=$(/system/bin/dirname -- \"$2\") || { /system/bin/cat >/dev/null; exit 1; }; "
                +"if [ \"$p1\" != \"$p2\" ]; then /system/bin/cat >/dev/null; exit 1; fi; "
                +"d1=$(/system/bin/stat -c %d -- \"$p1\") || { /system/bin/cat >/dev/null; exit 1; }; "
                +"d2=$(/system/bin/stat -c %d -- \"$p2\") || { /system/bin/cat >/dev/null; exit 1; }; "
                +"if [ \"$d1\" != \"$d2\" ]; then /system/bin/cat >/dev/null; exit 1; fi; "
                +"if [ -e \"$2\" ] || [ -L \"$2\" ]; then /system/bin/cat >/dev/null; exit 91; fi; "
                +"mode=$(/system/bin/stat -c %a -- \"$1\") || { /system/bin/cat >/dev/null; exit 1; }; "
                +"case \"$mode\" in [0-7][0-7][0-7]|[0-7][0-7][0-7][0-7]) ;; *) /system/bin/cat >/dev/null; exit 1 ;; esac; "
                +"umask 077; set -C; "
                +"if ! /system/bin/cat > \"$2\"; then /system/bin/rm -f -- \"$2\"; exit 1; fi; "
                +"/system/bin/chmod \"$mode\" \"$2\"; "
                +"if ! /system/bin/dd if=/dev/null of=\"$2\" conv=notrunc,fsync status=none; then /system/bin/rm -f -- \"$2\"; exit 1; fi; "
                +"if ! /system/bin/mv -f -- \"$2\" \"$1\"; then /system/bin/rm -f -- \"$2\"; exit 1; fi; "
                +"exit 0";
        }

        void copy(File from,File to)throws Exception {
            final String[] claimed=new String[1];
            try{
                String text=run(rootCopyProgram(),null,in->{
                    String body=rootMarker(in);claimed[0]=destinationIdentity(body);return body;
                },from,to);
                if(!"copied".equals(markerOf(text)))throw new IOException("Device root copy did not report a result");
            }catch(Exception problem){
                String message=problem.getMessage()==null?"Device copy could not finish":problem.getMessage();
                if(message.startsWith("Device destination already exists")||message.startsWith("Device path is a symbolic link")
                    ||message.startsWith("Device path is not a regular file")||message.startsWith("Device file exceeds the copy size limit")
                    ||message.startsWith("Device root permission was denied")){
                    if(problem instanceof IOException)throw (IOException)problem;
                    throw new IOException(message,problem);
                }
                if(claimed[0]!=null)discardUnverified(to,claimed[0]);
                boolean remains=claimed[0]==null||sameNodeRemains(to,claimed[0]);
                if(!remains)throw new IOException("Device copy could not finish. The incomplete copy is no longer at "+to+". "+message,problem);
                if(message.contains(String.valueOf(to))){
                    if(problem instanceof IOException)throw (IOException)problem;
                    throw new IOException(message,problem);
                }
                throw new IOException(message+" An incomplete destination may remain at "+to+".",problem);
            }
        }

        private boolean sameNodeRemains(File destination,String identity){
            if(identity==null||!identity.matches("[0-9]+:[0-9]+"))return true;
            try{
                String found=run("if [ -e \"$1\" ] || [ -L \"$1\" ]; then /system/bin/stat -c '%d:%i' -- \"$1\"; else /system/bin/echo gone; fi",
                    null,in->{
                        ByteArrayOutputStream out=new ByteArrayOutputStream();
                        byte[] buffer=new byte[64];
                        int n;
                        while((n=in.read(buffer))>=0){
                            if(out.size()+n>128)throw new IOException("Device copy could not be identified");
                            out.write(buffer,0,n);
                        }
                        return out.toString(StandardCharsets.UTF_8.name()).trim();
                    },destination);
                return identity.equals(found);
            }catch(Exception problem){return true;}
        }

        private static String rootCopyProgram(){
            return "if [ -L \"$1\" ]; then exit 82; fi; "
                +"if [ ! -f \"$1\" ]; then exit 84; fi; "
                +"if [ -e \"$2\" ] || [ -L \"$2\" ]; then exit 81; fi; "
                +"size=$(/system/bin/stat -c %s -- \"$1\") || exit 1; "
                +"case \"$size\" in ''|*[!0-9]*) exit 1 ;; esac; "
                +"if [ ${#size} -gt 15 ]; then exit 85; fi; "
                +"set -C; if ! exec 3>\"$2\"; then exit 81; fi; "
                +"ours=$(/system/bin/stat -c '%d:%i' -- /proc/self/fd/3) || { exec 3>&-; exit 96; }; "
                +"/system/bin/echo \"dest $ours\" || { exec 3>&-; exit 96; }; "
                +"if [ \"$size\" -gt 0 ]; then blocks=$((size / 65536)); rem=$((size % 65536)); "
                +"if [ \"$blocks\" -gt 0 ]; then /system/bin/dd if=\"$1\" bs=65536 count=\"$blocks\" status=none >&3 || { exec 3>&-; exit 96; }; fi; "
                +"if [ \"$rem\" -gt 0 ]; then /system/bin/dd if=\"$1\" bs=1 skip=$((blocks * 65536)) count=\"$rem\" status=none >&3 || { exec 3>&-; exit 96; }; fi; fi; "
                +"/system/bin/dd if=/dev/null of=/proc/self/fd/3 conv=notrunc,fsync status=none || { exec 3>&-; exit 96; }; "
                +"got=$(/system/bin/stat -c %s -- /proc/self/fd/3) || { exec 3>&-; exit 96; }; "
                +"exec 3>&-; if [ \"$got\" != \"$size\" ]; then exit 96; fi; "
                +"/system/bin/echo copied; exit 0";
        }

        Relocation move(File from,File to)throws Exception {
            // Same-filesystem names are linked when the filesystem allows it, and otherwise copied
            // into an exclusive name. A cross-volume regular file is copied only up to its measured size.
            boolean copying=false,verified=false;
            final String[] claimed=new String[1];
            try{
                String text=run(rootMoveProgram(true),null,in->{
                    String body=rootMarker(in);claimed[0]=destinationIdentity(body);return body;
                },from,to);
                String marker=markerOf(text);
                if("moved by link".equals(marker)||"moved by directory claim".equals(marker)||"moved by rename".equals(marker))
                    return new Relocation(marker,to,null);
                if("copied pending verify".equals(marker)||"moved by copy".equals(marker)){
                    copying=true;
                    JSONObject original=fingerprint(from,signal::throwIfCanceled);
                    JSONObject copied=fingerprint(to,signal::throwIfCanceled);
                    if(original.getLong("bytes")!=copied.getLong("bytes")||!original.getString("sha256").equals(copied.getString("sha256")))
                        throw new IOException("Device copy differs from the source; original kept at "+from);
                    verified=true;copying=false;
                    try{removeVerified(from,sourceIdentity(text));}
                    catch(Exception problem){throw new IOException("Copied to "+to+" but the original remains at "+from+". "+problem.getMessage(),problem);}
                    return new Relocation("moved by copy",to,null);
                }
                if(!"copied across volumes".equals(marker))throw new IOException("Device root move did not report a result");
                copying=true;
                JSONObject original=fingerprint(from,signal::throwIfCanceled);
                JSONObject copied=fingerprint(to,signal::throwIfCanceled);
                if(original.getLong("bytes")!=copied.getLong("bytes")||!original.getString("sha256").equals(copied.getString("sha256")))
                    throw new IOException("Device copy differs from the source; original kept at "+from);
                verified=true;copying=false;
                File parent=from.getParentFile();
                if(parent==null)throw new IOException("Copied across volumes to "+to+" but the original remains at "+from);
                File review=new File(parent,"review");
                reviewFolder(review);
                long stamp=System.currentTimeMillis();
                IOException collision=null;
                for(int counter=0;counter<1000;counter++){
                    signal.throwIfCanceled();
                    File candidate=new File(review,reviewLeaf(from.getName(),stamp,counter));
                    try{renameClaim(from,candidate);return new Relocation("copied across volumes",to,candidate);}
                    catch(IOException problem){
                        String message=problem.getMessage();
                        if(message==null||!message.startsWith("Device destination already exists"))
                            throw new IOException("Copied across volumes to "+to+" but the original remains at "+from+". "+message,problem);
                        collision=problem;
                    }
                }
                throw collision==null?new IOException("Copied across volumes to "+to+" but the original remains at "+from)
                    :new IOException("Copied across volumes to "+to+" but the original remains at "+from+". "+collision.getMessage(),collision);
            }catch(Exception problem){
                int code=exitCode(problem);
                if(!verified&&claimed[0]!=null&&(copying||discardAfter(problem)||code==1||code==95||code==96))
                    discardUnverified(to,claimed[0]);
                if(code==96){
                    String message=problem.getMessage()==null?"Device copy could not be verified":problem.getMessage();
                    if(!message.contains(String.valueOf(to)))
                        throw new IOException(message+" An unverified destination may remain at "+to+".",problem);
                }
                if(copying&&!(problem instanceof android.os.OperationCanceledException)){
                    String message=problem.getMessage();
                    if(message==null||!message.startsWith("Device copy differs"))
                        throw new IOException("Device copy could not be verified; original kept at "+from+". "+message,problem);
                }
                throw problem;
            }
        }

        private void renameClaim(File from,File to)throws Exception {
            final String[] claimed=new String[1];
            String text;
            try{text=run(rootMoveProgram(false),null,in->{
                String body=rootMarker(in);claimed[0]=destinationIdentity(body);return body;
            },from,to);}
            catch(Exception problem){
                int code=exitCode(problem);
                if(claimed[0]!=null&&(discardAfter(problem)||code==1||code==95||code==96))discardUnverified(to,claimed[0]);
                if(code==96){
                    String message=problem.getMessage()==null?"Device copy could not be verified":problem.getMessage();
                    if(!message.contains(String.valueOf(to)))
                        throw new IOException(message+" An unverified destination may remain at "+to+".",problem);
                }
                throw problem;
            }
            String marker=markerOf(text);
            if("moved by link".equals(marker)||"moved by directory claim".equals(marker)||"moved by rename".equals(marker))return;
            if("copied pending verify".equals(marker)||"moved by copy".equals(marker)){
                try{
                    JSONObject original=fingerprint(from,signal::throwIfCanceled);
                    JSONObject copied=fingerprint(to,signal::throwIfCanceled);
                    if(original.getLong("bytes")!=copied.getLong("bytes")||!original.getString("sha256").equals(copied.getString("sha256")))
                        throw new IOException("Device copy differs from the source; original kept at "+from);
                }catch(Exception problem){
                    discardUnverified(to,claimed[0]);
                    if(problem instanceof IOException&&problem.getMessage()!=null&&problem.getMessage().startsWith("Device copy differs"))throw (IOException)problem;
                    throw new IOException("Device copy could not be verified; original kept at "+from+". "+problem.getMessage(),problem);
                }
                try{removeVerified(from,sourceIdentity(text));}
                catch(Exception problem){throw new IOException("Copied to "+to+" but the original remains at "+from+". "+problem.getMessage(),problem);}
                return;
            }
            throw new IOException("Device root move did not report a result");
        }

        private static int exitCode(Exception problem){
            if(problem==null||problem.getMessage()==null)return Integer.MIN_VALUE;
            String message=problem.getMessage();
            int mark=message.lastIndexOf("exit ");
            if(mark<0)return Integer.MIN_VALUE;
            int end=mark+5;
            while(end<message.length()&&(Character.isDigit(message.charAt(end))||message.charAt(end)=='-'))end++;
            try{return Integer.parseInt(message.substring(mark+5,end));}
            catch(NumberFormatException ignored){return Integer.MIN_VALUE;}
        }

        private static boolean discardAfter(Exception problem){
            if(problem instanceof android.os.OperationCanceledException)return true;
            String message=problem.getMessage();
            if(message!=null&&(message.startsWith("Device copy differs")||message.startsWith("Device copy could not be verified")))return true;
            int code=exitCode(problem);
            return code==9||code==95||code==96||code==137||code==143||code<0&&code!=Integer.MIN_VALUE||code>=128;
        }

        private void removeVerified(File source,String identity)throws Exception {
            if(identity==null||!identity.matches("[0-9]+:[0-9]+"))
                throw new IOException("Copied to the destination but the original remains at "+source);
            run("holds=\"$2\"; id='"+identity+"'; "
                +"if [ -L \"$1\" ] || [ ! -f \"$1\" ]; then exit 89; fi; "
                +"if ! exec 4<\"$1\"; then exit 89; fi; "
                +"fdid=$(/system/bin/stat -c '%d:%i' -- /proc/self/fd/4) || { exec 4<&-; exit 89; }; "
                +"path=$(/system/bin/stat -c '%d:%i' -- \"$1\") || { exec 4<&-; exit 89; }; "
                +"if [ \"$fdid\" != \"$id\" ] || [ \"$path\" != \"$id\" ]; then exec 4<&-; exit 89; fi; "
                +"before=$(/system/bin/stat -c %h -- /proc/self/fd/4) || { exec 4<&-; exit 1; }; "
                +"dev=$(/system/bin/stat -c %d -- \"$1\") || { exec 4<&-; exit 1; }; "
                +"hold=$(printf '%s\\n' \"$holds\" | while IFS= read -r h; do "
                +"[ -n \"$h\" ] || continue; "
                +"if [ ! -d \"$h\" ] || [ -L \"$h\" ]; then /system/bin/mkdir -p -- \"$h\" || exit 1; fi; "
                +"[ -d \"$h\" ] && [ ! -L \"$h\" ] || continue; "
                +"d=$(/system/bin/stat -c %d -- \"$h\") || continue; "
                +"[ \"$d\" = \"$dev\" ] || continue; printf '%s\\n' \"$h\"; exit 0; done); "
                +"if [ -z \"$hold\" ]; then exec 4<&-; exit 92; fi; "
                +"token=$hold/r.$$; if [ -e \"$token\" ] || [ -L \"$token\" ]; then exec 4<&-; exit 1; fi; "
                +"if ! /system/bin/mv -n -- \"$1\" \"$token\"; then exec 4<&-; exit 89; fi; "
                +"held=$(/system/bin/stat -c '%d:%i' -- \"$token\" 2>/dev/null || printf '%s' missing); "
                +"if [ \"$held\" != \"$id\" ]; then "
                +"if [ ! -e \"$1\" ] && [ ! -L \"$1\" ]; then /system/bin/mv -n -- \"$token\" \"$1\"; fi; "
                +"exec 4<&-; exit 93; fi; "
                +"if ! /system/bin/rm -- \"$token\"; then exec 4<&-; exit 1; fi; "
                +"after=$(/system/bin/stat -c %h -- /proc/self/fd/4) || { exec 4<&-; exit 90; }; "
                +"exec 4<&-; if [ \"$after\" -ge \"$before\" ]; then exit 90; fi; exit 0",
                null,in->null,source);
        }

        private void discardUnverified(File destination,String identity){
            if(identity==null||!identity.matches("[0-9]+:[0-9]+"))return;
            try{
                String program="test $(/system/bin/id -u) = 0 || exit 0; holds=\"$3\"; "
                    +"case \"$2\" in [0-9]*:[0-9]*) ;; *) exit 0 ;; esac; "
                    +"if [ -L \"$1\" ] || [ ! -f \"$1\" ]; then exit 0; fi; "
                    +"if ! exec 3<\"$1\"; then exit 0; fi; "
                    +"now=$(/system/bin/stat -c '%d:%i' -- /proc/self/fd/3 2>/dev/null || printf '%s' x); "
                    +"path=$(/system/bin/stat -c '%d:%i' -- \"$1\" 2>/dev/null || printf '%s' y); "
                    +"if [ \"$now\" != \"$2\" ] || [ \"$path\" != \"$2\" ]; then exec 3<&-; exit 0; fi; "
                    +"dev=$(/system/bin/stat -c %d -- \"$1\") || { exec 3<&-; exit 0; }; "
                    +"hold=$(printf '%s\\n' \"$holds\" | while IFS= read -r h; do "
                    +"[ -n \"$h\" ] || continue; /system/bin/mkdir -p -- \"$h\" || exit 1; "
                    +"[ -d \"$h\" ] && [ ! -L \"$h\" ] || continue; "
                    +"d=$(/system/bin/stat -c %d -- \"$h\") || continue; "
                    +"[ \"$d\" = \"$dev\" ] || continue; printf '%s\\n' \"$h\"; exit 0; done); "
                    +"if [ -z \"$hold\" ]; then exec 3<&-; exit 0; fi; "
                    +"token=$hold/d.$$; if [ -e \"$token\" ] || [ -L \"$token\" ]; then exec 3<&-; exit 0; fi; "
                    +"if ! /system/bin/mv -n -- \"$1\" \"$token\"; then exec 3<&-; exit 0; fi; "
                    +"held=$(/system/bin/stat -c '%d:%i' -- \"$token\" 2>/dev/null || printf '%s' missing); "
                    +"if [ \"$held\" != \"$2\" ]; then "
                    +"if [ ! -e \"$1\" ] && [ ! -L \"$1\" ]; then /system/bin/mv -n -- \"$token\" \"$1\"; fi; "
                    +"exec 3<&-; exit 0; fi; "
                    +"/system/bin/rm -- \"$token\"; exec 3<&-; exit 0";
                StringBuilder command=new StringBuilder("/system/bin/sh -c '")
                    .append(program.replace("'","'\"'\"'")).append("' device ")
                    .append(quote(destination)).append(" '").append(identity).append("' ")
                    .append(quoteText(holdingArgument()));
                Process process=new ProcessBuilder(su.getPath(),"-c",command.toString()).start();
                Thread drain=new Thread(()->{try{byte[] sink=new byte[4096];InputStream err=process.getErrorStream();while(err.read(sink)>=0){}}catch(IOException ignored){}},"device-root-error");
                drain.setDaemon(true);drain.start();
                try{process.getOutputStream().close();process.getInputStream().close();}
                finally{if(!process.waitFor(10,TimeUnit.SECONDS))process.destroyForcibly();}
            }catch(Exception ignored){}
        }

        // mkdir and O_EXCL claim a name without replacing it. link is tried first and, where the
        // volume does not support it, a size-bounded copy is used. Nothing is reported as atomic.
        private static String rootMoveProgram(boolean crossVolume){
            String allow=crossVolume?"yes":"no";
            return "allow="+allow+"; entries=0; serial=0; holds=\"$3\"; "
                +"pick_hold() { local dev found; dev=$(/system/bin/stat -c %d -- \"$1\") || return 1; "
                +"found=$(printf '%s\\n' \"$holds\" | while IFS= read -r h; do "
                +"[ -n \"$h\" ] || continue; "
                +"if [ ! -d \"$h\" ] || [ -L \"$h\" ]; then /system/bin/mkdir -p -- \"$h\" || exit 1; fi; "
                +"[ -d \"$h\" ] && [ ! -L \"$h\" ] || continue; "
                +"d=$(/system/bin/stat -c %d -- \"$h\") || continue; "
                +"[ \"$d\" = \"$dev\" ] || continue; printf '%s\\n' \"$h\"; exit 0; done); "
                +"[ -n \"$found\" ] || return 92; printf '%s\\n' \"$found\"; return 0; }; "
                +"remove_matched() { local now fdid before after hold token held status; "
                +"now=$(/system/bin/stat -c '%d:%i' -- \"$1\" 2>/dev/null) || return 89; "
                +"if [ \"$now\" != \"$2\" ]; then return 89; fi; "
                +"if ! exec 4<\"$1\"; then return 89; fi; "
                +"fdid=$(/system/bin/stat -c '%d:%i' -- /proc/self/fd/4) || { exec 4<&-; return 1; }; "
                +"if [ \"$fdid\" != \"$2\" ]; then exec 4<&-; return 89; fi; "
                +"before=$(/system/bin/stat -c %h -- /proc/self/fd/4) || { exec 4<&-; return 1; }; "
                +"hold=$(pick_hold \"$1\") || { status=$?; exec 4<&-; return $status; }; "
                +"serial=$((serial+1)); token=$hold/d.$$.$serial; "
                +"if [ -e \"$token\" ] || [ -L \"$token\" ]; then exec 4<&-; return 1; fi; "
                +"if ! /system/bin/mv -n -- \"$1\" \"$token\"; then exec 4<&-; return 1; fi; "
                +"held=$(/system/bin/stat -c '%d:%i' -- \"$token\" 2>/dev/null || printf '%s' missing); "
                +"if [ \"$held\" != \"$2\" ]; then "
                +"if [ ! -e \"$1\" ] && [ ! -L \"$1\" ]; then /system/bin/mv -n -- \"$token\" \"$1\" || /system/bin/echo \"kept $token\"; fi; "
                +"if [ -e \"$token\" ] || [ -L \"$token\" ]; then /system/bin/echo \"kept $token\"; fi; "
                +"exec 4<&-; return 93; fi; "
                +"if ! /system/bin/rm -- \"$token\"; then exec 4<&-; return 1; fi; "
                +"after=$(/system/bin/stat -c %h -- /proc/self/fd/4) || { exec 4<&-; return 90; }; "
                +"exec 4<&-; if [ \"$after\" -ge \"$before\" ]; then return 90; fi; return 0; }; "
                +"remove_empty_matched() { local now hold token held; "
                +"now=$(/system/bin/stat -c '%d:%i' -- \"$1\") || return 89; "
                +"if [ \"$now\" != \"$2\" ]; then return 89; fi; "
                +"hold=$(pick_hold \"$1\") || return $?; "
                +"serial=$((serial+1)); token=$hold/e.$$.$serial; "
                +"if [ -e \"$token\" ] || [ -L \"$token\" ]; then return 1; fi; "
                +"if ! /system/bin/mv -n -- \"$1\" \"$token\"; then return 89; fi; "
                +"held=$(/system/bin/stat -c '%d:%i' -- \"$token\" 2>/dev/null || printf '%s' missing); "
                +"if [ \"$held\" != \"$2\" ]; then "
                +"if [ ! -e \"$1\" ] && [ ! -L \"$1\" ]; then /system/bin/mv -n -- \"$token\" \"$1\" || /system/bin/echo \"kept $token\"; fi; "
                +"if [ -e \"$token\" ] || [ -L \"$token\" ]; then /system/bin/echo \"kept $token\"; fi; "
                +"return 93; fi; "
                +"if ! /system/bin/rmdir -- \"$token\"; then "
                +"if [ ! -e \"$1\" ] && [ ! -L \"$1\" ]; then /system/bin/mv -n -- \"$token\" \"$1\"; fi; return 1; fi; return 0; }; "
                +"drop_owned() { local ours path hold token held; "
                +"ours=$(/system/bin/stat -c '%d:%i' -- /proc/self/fd/3 2>/dev/null || printf '%s' x); "
                +"path=$(/system/bin/stat -c '%d:%i' -- \"$1\" 2>/dev/null || printf '%s' y); "
                +"if [ \"$ours\" != \"$path\" ]; then exec 3>&-; return 1; fi; "
                +"hold=$(pick_hold \"$1\") || { exec 3>&-; return 1; }; "
                +"serial=$((serial+1)); token=$hold/d.$$.$serial; "
                +"if [ -e \"$token\" ] || [ -L \"$token\" ]; then exec 3>&-; return 1; fi; "
                +"if ! /system/bin/mv -n -- \"$1\" \"$token\"; then exec 3>&-; return 1; fi; "
                +"held=$(/system/bin/stat -c '%d:%i' -- \"$token\" 2>/dev/null || printf '%s' missing); "
                +"if [ \"$held\" != \"$ours\" ]; then "
                +"if [ ! -e \"$1\" ] && [ ! -L \"$1\" ]; then /system/bin/mv -n -- \"$token\" \"$1\"; fi; "
                +"exec 3>&-; return 1; fi; "
                +"/system/bin/rm -- \"$token\" || { exec 3>&-; return 1; }; exec 3>&-; return 1; }; "
                +"copy_exclusive() { local size srcid ours oursize pathid pathsize srcsize blocks rem nowsrc; "
                +"if [ -L \"$1\" ] || [ ! -f \"$1\" ]; then return 84; fi; "
                +"if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 81; fi; "
                +"size=$(/system/bin/stat -c %s -- \"$1\") || return 1; "
                +"case \"$size\" in ''|*[!0-9]*) return 1 ;; esac; "
                +"if [ ${#size} -gt 15 ]; then return 85; fi; "
                +"srcid=$(/system/bin/stat -c '%d:%i' -- \"$1\") || return 1; "
                +"if [ \"$3\" = report ]; then /system/bin/echo \"src $srcid\" || return 1; fi; "
                +"set -C; if ! exec 3>\"$2\"; then return 81; fi; "
                +"ours=$(/system/bin/stat -c '%d:%i' -- /proc/self/fd/3) || { exec 3>&-; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 1; }; "
                +"if [ \"$3\" = report ]; then /system/bin/echo \"dest $ours\" || { drop_owned \"$2\"; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 1; }; fi; "
                +"if [ \"$size\" -gt 0 ]; then blocks=$((size / 65536)); rem=$((size % 65536)); "
                +"if [ \"$blocks\" -gt 0 ]; then /system/bin/dd if=\"$1\" bs=65536 count=\"$blocks\" status=none >&3 || { drop_owned \"$2\"; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 1; }; fi; "
                +"if [ \"$rem\" -gt 0 ]; then /system/bin/dd if=\"$1\" bs=1 skip=$((blocks * 65536)) count=\"$rem\" status=none >&3 || { drop_owned \"$2\"; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 1; }; fi; fi; "
                +"/system/bin/dd if=/dev/null of=/proc/self/fd/3 conv=notrunc,fsync status=none || { drop_owned \"$2\"; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 1; }; "
                +"oursize=$(/system/bin/stat -c %s -- /proc/self/fd/3) || { drop_owned \"$2\"; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 1; }; "
                +"pathid=$(/system/bin/stat -c '%d:%i' -- \"$2\") || { drop_owned \"$2\"; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 1; }; "
                +"pathsize=$(/system/bin/stat -c %s -- \"$2\") || { drop_owned \"$2\"; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 1; }; "
                +"srcsize=$(/system/bin/stat -c %s -- \"$1\") || { drop_owned \"$2\"; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 1; }; "
                +"if [ \"$ours\" != \"$pathid\" ]; then exec 3>&-; return 89; fi; exec 3>&-; "
                +"if [ \"$oursize\" != \"$size\" ] || [ \"$pathsize\" != \"$size\" ] || [ \"$srcsize\" != \"$size\" ]; then remove_matched \"$2\" \"$ours\" || return 96; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 1; fi; "
                +"nowsrc=$(/system/bin/stat -c '%d:%i' -- \"$1\") || { remove_matched \"$2\" \"$ours\" || return 96; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 1; }; "
                +"if [ \"$nowsrc\" != \"$srcid\" ]; then remove_matched \"$2\" \"$ours\" || return 96; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 1; fi; "
                +"/system/bin/cmp -s -- \"$1\" \"$2\"; cmp_status=$?; "
                +"if [ \"$cmp_status\" -ne 0 ]; then remove_matched \"$2\" \"$ours\" || return 96; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 95; fi; return 0; }; "
                +"move_file() { local a b; "
                +"if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 81; fi; "
                +"if [ -L \"$1\" ]; then return 82; fi; "
                +"if [ ! -f \"$1\" ]; then return 84; fi; "
                +"if /system/bin/ln -- \"$1\" \"$2\"; then "
                +"a=$(/system/bin/stat -c '%d:%i' -- \"$1\") || return 1; "
                +"b=$(/system/bin/stat -c '%d:%i' -- \"$2\") || return 1; "
                +"if [ \"$a\" != \"$b\" ]; then return 1; fi; "
                +"remove_matched \"$1\" \"$a\" || return $?; "
                +"if [ \"$3\" = report ]; then /system/bin/echo \"moved by link\"; fi; return 0; fi; "
                +"if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 81; fi; "
                +"copy_exclusive \"$1\" \"$2\" \"$3\" || return $?; "
                +"if [ \"$3\" = report ]; then /system/bin/echo \"copied pending verify\"; return 0; fi; "
                +"a=$(/system/bin/stat -c '%d:%i' -- \"$1\") || { remove_matched \"$2\" \"$(/system/bin/stat -c '%d:%i' -- \"$2\" 2>/dev/null)\" || return 96; if [ -e \"$2\" ] || [ -L \"$2\" ]; then return 96; fi; return 1; }; "
                +"remove_matched \"$1\" \"$a\" || return $?; return 0; }; "
                +"move_dir() { local from into depth child base target mode owner dirid fifo finder status find_status enum_base; "
                +"from=$1; into=$2; depth=$3; "
                +"if [ \"$depth\" -gt 64 ]; then return 87; fi; "
                +"enum_base=${holds%%$'\\n'*}; [ -n \"$enum_base\" ] || return 92; "
                +"/system/bin/mkdir -p -- \"$enum_base\" || return 94; "
                +"fifo=$enum_base/enum.$$.$depth; /system/bin/rm -f -- \"$fifo\"; "
                +"/system/bin/mkfifo \"$fifo\" || return 94; "
                +"/system/bin/find \"$from\" -mindepth 1 -maxdepth 1 -print0 > \"$fifo\" & finder=$!; status=0; "
                +"while IFS= read -r -d '' child; do "
                +"entries=$((entries+1)); if [ \"$entries\" -gt 100000 ]; then status=88; break; fi; "
                +"base=${child##*/}; if [ \"${#base}\" -gt 1024 ]; then status=1; break; fi; "
                +"target=$into/$base; "
                +"if [ -e \"$target\" ] || [ -L \"$target\" ]; then status=81; break; fi; "
                +"if [ -d \"$child\" ] && [ ! -L \"$child\" ]; then "
                +"mode=$(/system/bin/stat -c %a -- \"$child\") || { status=1; break; }; "
                +"owner=$(/system/bin/stat -c %u:%g -- \"$child\") || { status=1; break; }; "
                +"dirid=$(/system/bin/stat -c '%d:%i' -- \"$child\") || { status=1; break; }; "
                +"/system/bin/mkdir -- \"$target\" || { status=81; break; }; "
                +"move_dir \"$child\" \"$target\" $((depth+1)) || { status=$?; break; }; "
                +"/system/bin/chmod \"$mode\" \"$target\" || { status=1; break; }; "
                +"/system/bin/chown \"$owner\" \"$target\" || { status=1; break; }; "
                +"remove_empty_matched \"$child\" \"$dirid\" || { status=$?; break; }; "
                +"else move_file \"$child\" \"$target\" || { status=$?; break; }; fi; "
                +"done < \"$fifo\"; "
                +"if [ \"$status\" -eq 0 ]; then wait \"$finder\"; find_status=$?; "
                +"if [ \"$find_status\" -ne 0 ]; then status=1; fi; "
                +"else kill \"$finder\" 2>/dev/null; wait \"$finder\" 2>/dev/null; fi; "
                +"/system/bin/rm -f -- \"$fifo\"; return \"$status\"; }; "
                +"if [ -e \"$2\" ] || [ -L \"$2\" ]; then exit 81; fi; "
                +"if [ -L \"$1\" ]; then exit 82; fi; "
                +"if [ ! -e \"$1\" ]; then exit 1; fi; "
                +"case \"$2\" in */*) destparent=\"${2%/*}\" ;; *) exit 1 ;; esac; "
                +"srcdev=$(/system/bin/stat -c %d -- \"$1\") || exit 1; "
                +"dstdev=$(/system/bin/stat -c %d -- \"$destparent\") || exit 1; "
                +"umask 077; "
                +"if [ \"$srcdev\" != \"$dstdev\" ]; then if [ \"$allow\" != yes ]; then exit 83; fi; "
                +"if [ -d \"$1\" ]; then exit 83; fi; "
                +"copy_exclusive \"$1\" \"$2\" report || exit $?; "
                +"/system/bin/echo \"copied across volumes\"; exit 0; fi; "
                +"if [ -d \"$1\" ]; then "
                +"srcmode=$(/system/bin/stat -c %a -- \"$1\") || exit 1; "
                +"srcowner=$(/system/bin/stat -c %u:%g -- \"$1\") || exit 1; "
                +"srcid=$(/system/bin/stat -c '%d:%i' -- \"$1\") || exit 1; "
                +"/system/bin/mkdir -- \"$2\" || exit 81; "
                +"claim=$(/system/bin/stat -c '%d:%i' -- \"$2\") || exit 1; "
                +"entries=0; move_dir \"$1\" \"$2\" 1; status=$?; "
                +"now=$(/system/bin/stat -c '%d:%i' -- \"$2\" 2>/dev/null || printf '%s' missing); "
                +"if [ \"$status\" -ne 0 ] || [ \"$claim\" != \"$now\" ]; then "
                +"if [ \"$claim\" = \"$now\" ]; then entries=0; move_dir \"$2\" \"$1\" 1 || exit 86; remove_empty_matched \"$2\" \"$claim\" || exit 86; fi; "
                +"if [ \"$status\" -eq 0 ]; then exit 1; fi; exit \"$status\"; fi; "
                +"if ! /system/bin/chmod \"$srcmode\" \"$2\" || ! /system/bin/chown \"$srcowner\" \"$2\"; then "
                +"entries=0; move_dir \"$2\" \"$1\" 1 || exit 86; remove_empty_matched \"$2\" \"$claim\" || exit 86; exit 1; fi; "
                +"nowsrc=$(/system/bin/stat -c '%d:%i' -- \"$1\") || exit 89; "
                +"if [ \"$nowsrc\" != \"$srcid\" ]; then exit 89; fi; "
                +"remove_empty_matched \"$1\" \"$srcid\" || exit 86; "
                +"if [ -e \"$1\" ] || [ -L \"$1\" ]; then exit 86; fi; "
                +"/system/bin/echo \"moved by directory claim\"; exit 0; fi; "
                +"if [ ! -f \"$1\" ]; then exit 84; fi; "
                +"move_file \"$1\" \"$2\" report || exit $?";
        }

        void reviewFolder(File folder)throws Exception {
            run("if [ ! -e \"$1\" ]; then umask 077; /system/bin/mkdir -- \"$1\" || exit 1; fi; "
                +"test -d \"$1\" && test ! -L \"$1\"",null,in->null,folder);
        }

        interface RootReader<T> { T read(InputStream input)throws Exception; }
    }

    static JSONArray release(ContentResolver resolver,JSONArray owned,String phoneUri)throws Exception {
        if(owned==null||owned.length()>65)throw new IOException("Selected document grant list is invalid");
        JSONArray released=new JSONArray();List<UriPermission> permissions=resolver.getPersistedUriPermissions();
        for(int i=0;i<owned.length();i++) {
            String value=owned.optString(i);Uri uri=Uri.parse(value);
            if(!valid(uri))continue;
            boolean shared=value.equals(phoneUri)||DocumentsContract.isTreeUri(uri),failed=false;
            for(UriPermission permission:permissions)if(uri.equals(permission.getUri())) {
                // A writable grant belongs to another file/folder workflow. Removing
                // this read selection must not revoke that workflow's shared grant.
                if(permission.isWritePermission())shared=true;
                if(!shared&&permission.isReadPermission())try {
                    resolver.releasePersistableUriPermission(uri,Intent.FLAG_GRANT_READ_URI_PERMISSION);
                }catch(SecurityException|IllegalArgumentException problem){failed=true;}
            }
            if(!failed)released.put(value);
        }
        return released;
    }
}
