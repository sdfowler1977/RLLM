package com.local.client;

import android.util.AtomicFile;
import android.util.Base64;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import javax.crypto.Cipher;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;
import org.json.JSONObject;

/** Android keystore and atomic-file binding. Rust owns transfer metadata and policy. */
final class TransferStore {
    private final AtomicFile file;
    private static final byte[] AAD="RLLM/project-transfer/v1".getBytes(StandardCharsets.UTF_8);
    TransferStore(File directory){file=new AtomicFile(new File(directory,"project-transfer.dat"));}
    private byte[] read(InputStream in,int limit)throws IOException {
        ByteArrayOutputStream out=new ByteArrayOutputStream();byte[] buffer=new byte[8192];int n;
        while((n=in.read(buffer))!=-1){if(n>limit-out.size())throw new IOException("Pending snapshot exceeds its bound");out.write(buffer,0,n);}return out.toByteArray();
    }
    // Every call is serialized by Activity.documentJobs, including across Activity instances.
    JSONObject load(SecretKey key)throws Exception {
        if(!file.getBaseFile().exists()&&!new File(file.getBaseFile()+".bak").exists())return null;
        try(DataInputStream in=new DataInputStream(file.openRead())){if(in.readInt()==MAGIC){try(ChunkReader reader=openChunks(key)){return reader.header;}}}
        byte[] encrypted;try(InputStream in=file.openRead()){encrypted=read(in,8*1024*1024);}
        if(encrypted.length<28)throw new IOException("Pending snapshot is incomplete");
        Cipher cipher=Cipher.getInstance("AES/GCM/NoPadding");cipher.init(Cipher.DECRYPT_MODE,key,new GCMParameterSpec(128,Arrays.copyOfRange(encrypted,0,12)));cipher.updateAAD(AAD);
        JSONObject record=new JSONObject(new String(cipher.doFinal(Arrays.copyOfRange(encrypted,12,encrypted.length)),StandardCharsets.UTF_8));
        if(record.optInt("version")!=1)throw new IOException("Unsupported pending snapshot version");
        if(record.has("cleared_ticket"))return record;
        JSONObject metadata=record.getJSONObject("record");byte[] payload=payload(record);
        if(metadata.optString("stage_ticket").isEmpty()||metadata.getInt("bytes")!=payload.length)throw new IOException("Pending snapshot metadata differs");return record;
    }
    static byte[] payload(JSONObject record)throws Exception {
        String encoded=record.getString("payload");if(encoded.length()>7*1024*1024)throw new IOException("Pending snapshot exceeds its bound");
        byte[] bytes=Base64.decode(encoded,Base64.NO_WRAP);if(bytes.length>5*1024*1024)throw new IOException("Snapshot exceeds 5 MiB");return bytes;
    }
    JSONObject stage(SecretKey key,JSONObject metadata,byte[] bytes)throws Exception {
        if(bytes.length>5*1024*1024||metadata.getInt("bytes")!=bytes.length||metadata.getString("stage_ticket").isEmpty())throw new IOException("Invalid pending snapshot");
        JSONObject existing=load(key);
        if(existing!=null&&!existing.has("cleared_ticket"))throw new IOException("An encrypted snapshot is already pending; review it before saving another");
        JSONObject record=new JSONObject().put("version",1).put("record",metadata).put("payload",Base64.encodeToString(bytes,Base64.NO_WRAP));write(key,record);
        JSONObject actual=load(key);if(actual==null||!actual.getJSONObject("record").toString().equals(metadata.toString())||!Arrays.equals(bytes,payload(actual)))throw new IOException("Encrypted snapshot readback differs");return actual.getJSONObject("record");
    }
    void clear(SecretKey key,String ticket)throws Exception {
        JSONObject record=load(key);if(record==null)throw new IOException("Pending snapshot is missing; no clear was confirmed");
        if(record.has("cleared_ticket")){if(ticket.equals(record.getString("cleared_ticket")))return;throw new IOException("A different snapshot was cleared");}
        if(!ticket.equals(record.getJSONObject("record").getString("stage_ticket")))throw new IOException("Pending snapshot changed; it was preserved");
        write(key,new JSONObject().put("version",1).put("cleared_ticket",ticket));JSONObject actual=load(key);
        if(actual==null||!ticket.equals(actual.optString("cleared_ticket")))throw new IOException("Pending snapshot cleanup readback differs");
    }
    private void write(SecretKey key,JSONObject record)throws Exception {
        byte[] plain=record.toString().getBytes(StandardCharsets.UTF_8);if(plain.length>8*1024*1024-28)throw new IOException("Pending snapshot exceeds its bound");
        Cipher cipher=Cipher.getInstance("AES/GCM/NoPadding");cipher.init(Cipher.ENCRYPT_MODE,key);cipher.updateAAD(AAD);
        FileOutputStream out=null;try{out=file.startWrite();out.write(cipher.getIV());out.write(cipher.doFinal(plain));file.finishWrite(out);}catch(Exception error){if(out!=null)file.failWrite(out);throw error;}
    }
    private static final int MAGIC=0x52544c32, CHUNK=65536;
    private static final long MAX=256L*1024*1024;
    private static final byte[] CHUNK_AAD="RLLM/project-transfer/v2".getBytes(StandardCharsets.UTF_8);
    private static byte[] aad(byte[] header,int index,int size)throws IOException {
        ByteArrayOutputStream bytes=new ByteArrayOutputStream();DataOutputStream out=new DataOutputStream(bytes);
        out.write(CHUNK_AAD);out.writeInt(header.length);out.write(header);out.writeInt(index);out.writeInt(size);return bytes.toByteArray();
    }
    private static void frame(DataOutputStream out,SecretKey key,byte[] plain,byte[] aad)throws Exception {
        Cipher cipher=Cipher.getInstance("AES/GCM/NoPadding");cipher.init(Cipher.ENCRYPT_MODE,key);cipher.updateAAD(aad);
        out.writeInt(plain.length);out.write(cipher.getIV());out.write(cipher.doFinal(plain));
    }
    private static byte[] frame(DataInputStream in,SecretKey key,int expected,byte[] aad)throws Exception {
        int length=in.readInt();if(length!=expected||length<0||length>CHUNK)throw new IOException("Encrypted chunk size differs");
        byte[] iv=new byte[12],encrypted=new byte[length+16];in.readFully(iv);in.readFully(encrypted);
        Cipher cipher=Cipher.getInstance("AES/GCM/NoPadding");cipher.init(Cipher.DECRYPT_MODE,key,new GCMParameterSpec(128,iv));cipher.updateAAD(aad);return cipher.doFinal(encrypted);
    }
    ChunkReader openChunks(SecretKey key)throws Exception{return new ChunkReader(file.openRead(),key);}
    static final class ChunkReader extends InputStream {
        private final DataInputStream in;private final SecretKey key;final JSONObject header;private final byte[] binding;
        private final long size;private long position;private int index,offset;private byte[] current=new byte[0];private boolean ended,failed;
        ChunkReader(InputStream source,SecretKey key)throws Exception {
            in=new DataInputStream(source);this.key=key;
            try{if(in.readInt()!=MAGIC)throw new IOException("Unsupported pending stream");int length=in.readInt();if(length<1||length>CHUNK)throw new IOException("Invalid stream header size");
                byte[] iv=new byte[12],encrypted=new byte[length+16];in.readFully(iv);in.readFully(encrypted);Cipher cipher=Cipher.getInstance("AES/GCM/NoPadding");cipher.init(Cipher.DECRYPT_MODE,key,new GCMParameterSpec(128,iv));cipher.updateAAD(CHUNK_AAD);binding=cipher.doFinal(encrypted);
                header=new JSONObject(new String(binding,StandardCharsets.UTF_8));JSONObject metadata=header.getJSONObject("record");size=metadata.getLong("bytes");
                if(header.getInt("version")!=2||header.getString("nonce").length()!=36||metadata.getInt("format")!=2||size<0||size>MAX||metadata.getString("stage_ticket").isEmpty())throw new IOException("Invalid stream metadata");
            }catch(Exception error){in.close();throw error;}
        }
        public int read(byte[] target,int start,int count)throws IOException {
            if(failed)throw new IOException("Encrypted stream is closed after failure");if(target==null)throw new NullPointerException();if(start<0||count<0||start>target.length-count)throw new IndexOutOfBoundsException();if(count==0)return 0;
            try{if(offset==current.length){if(position==size){if(!ended){frame(in,key,0,aad(binding,index,0));if(in.read()!=-1)throw new IOException("Extra encrypted stream bytes");ended=true;}return -1;}
                int length=(int)Math.min(CHUNK,size-position);current=frame(in,key,length,aad(binding,index++,length));offset=0;}
                int length=Math.min(count,current.length-offset);System.arraycopy(current,offset,target,start,length);offset+=length;position+=length;return length;
            }catch(Exception error){failed=true;try{in.close();}catch(IOException ignored){}throw error instanceof IOException?(IOException)error:new IOException("Encrypted chunk authentication failed",error);}
        }
        public int read()throws IOException{byte[] b=new byte[1];return read(b,0,1)<0?-1:b[0]&255;}
        public void close()throws IOException{failed=true;in.close();}
    }
    // Caller owns the process-wide document lane and the pinned Rust source descriptor.
    JSONObject stageChunks(SecretKey key,JSONObject metadata,InputStream source,Runnable rewind)throws Exception {
        long size=metadata.getLong("bytes");if(size<0||size>MAX||metadata.getInt("format")!=2)throw new IOException("Invalid stream size");
        JSONObject old=load(key);if(old!=null&&!old.has("cleared_ticket"))throw new IOException("A snapshot is already pending");
        JSONObject header=new JSONObject().put("version",2).put("record",metadata).put("nonce",java.util.UUID.randomUUID().toString());byte[] binding=header.toString().getBytes(StandardCharsets.UTF_8);if(binding.length>CHUNK)throw new IOException("Stream metadata exceeds bound");
        FileOutputStream raw=null;try{raw=file.startWrite();DataOutputStream out=new DataOutputStream(raw);out.writeInt(MAGIC);frame(out,key,binding,CHUNK_AAD);byte[] buffer=new byte[CHUNK];long left=size;int index=0;
            while(left>0){int wanted=(int)Math.min(CHUNK,left),n=0;while(n<wanted){int got=source.read(buffer,n,wanted-n);if(got<=0)throw new IOException("Source ended early or made no progress");n+=got;}frame(out,key,Arrays.copyOf(buffer,n),aad(binding,index++,n));left-=n;}
            if(source.read()!=-1)throw new IOException("Source contains extra bytes");frame(out,key,new byte[0],aad(binding,index,0));out.flush();file.finishWrite(raw);raw=null;
        }catch(Exception error){if(raw!=null)file.failWrite(raw);throw error;}
        rewind.run();try(ChunkReader actual=openChunks(key)){byte[] expected=new byte[CHUNK],received=new byte[CHUNK];long left=size;
            while(left>0){int length=(int)Math.min(CHUNK,left);new DataInputStream(source).readFully(expected,0,length);new DataInputStream(actual).readFully(received,0,length);for(int i=0;i<length;i++)if(expected[i]!=received[i])throw new IOException("Encrypted stream readback differs");left-=length;}
            if(source.read()!=-1||actual.read()!=-1)throw new IOException("Stream readback boundary differs");
            if(!actual.header.getJSONObject("record").toString().equals(metadata.toString()))throw new IOException("Stream metadata differs");return actual.header.getJSONObject("record");}
    }

}
