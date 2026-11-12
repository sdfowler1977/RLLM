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

/** Platform-only encrypted storage for the Rust-selected immutable export snapshot. */
final class ExportStore {
    private final AtomicFile file;
    private static final byte[] AAD="RLLM/document-export/v1".getBytes(StandardCharsets.UTF_8);
    ExportStore(File directory){file=new AtomicFile(new File(directory,"document-export.dat"));}
    private static byte[] read(InputStream in,int limit)throws IOException {
        ByteArrayOutputStream out=new ByteArrayOutputStream();byte[] buffer=new byte[8192];int n;
        while((n=in.read(buffer))!=-1){if(n>limit-out.size())throw new IOException("Saved export exceeds its bound");out.write(buffer,0,n);}return out.toByteArray();
    }
    JSONObject load(SecretKey key)throws Exception {
        if(!file.getBaseFile().exists()&&!new File(file.getBaseFile()+".bak").exists())return null;
        byte[] encrypted;try(InputStream in=file.openRead()){encrypted=read(in,8*1024*1024);}
        if(encrypted.length<28)throw new IOException("Saved export is incomplete");
        Cipher cipher=Cipher.getInstance("AES/GCM/NoPadding");cipher.init(Cipher.DECRYPT_MODE,key,new GCMParameterSpec(128,Arrays.copyOfRange(encrypted,0,12)));cipher.updateAAD(AAD);
        JSONObject record=new JSONObject(new String(cipher.doFinal(Arrays.copyOfRange(encrypted,12,encrypted.length)),StandardCharsets.UTF_8));
        if(record.optBoolean("cleared"))return null;
        if(record.optInt("version")!=1)throw new IOException("Unsupported saved export version");
        byte[] payload=payload(record);if(payload.length!=record.getInt("bytes"))throw new IOException("Saved export size differs");
        return record;
    }
    static byte[] payload(JSONObject record)throws Exception {
        String encoded=record.getString("payload");if(encoded.length()>7*1024*1024)throw new IOException("Saved export exceeds its bound");
        byte[] bytes=Base64.decode(encoded,Base64.NO_WRAP);if(bytes.length>5*1024*1024)throw new IOException("Export exceeds 5 MiB");return bytes;
    }
    static JSONObject metadata(JSONObject record)throws Exception {
        return new JSONObject().put("ticket",record.getString("ticket")).put("name",record.getString("name")).put("mime",record.getString("mime")).put("bytes",record.getInt("bytes"));
    }
    void stage(SecretKey key,JSONObject metadata,byte[] bytes)throws Exception {
        if(bytes.length>5*1024*1024)throw new IOException("Export exceeds 5 MiB");
        if(load(key)!=null)throw new IOException("A saved export already exists; resume or discard it first");
        JSONObject record=new JSONObject().put("ticket",metadata.getString("ticket")).put("name",metadata.getString("name")).put("mime",metadata.getString("mime")).put("version",1).put("bytes",bytes.length).put("payload",Base64.encodeToString(bytes,Base64.NO_WRAP));write(key,record);
        JSONObject actual=load(key);if(actual==null||!actual.getString("ticket").equals(metadata.getString("ticket"))||!Arrays.equals(bytes,payload(actual)))throw new IOException("Encrypted export readback differs");
    }
    void clear(SecretKey key,String ticket)throws Exception {
        JSONObject record=load(key);if(record==null)return;
        if(!ticket.equals(record.getString("ticket")))throw new IOException("Saved export changed; it was preserved");
        // Keep the storage file as a versioned encrypted marker rather than deleting it.
        write(key,new JSONObject().put("version",1).put("cleared",true));
    }
    private void write(SecretKey key,JSONObject record)throws Exception {
        byte[] plain=record.toString().getBytes(StandardCharsets.UTF_8);if(plain.length>8*1024*1024-28)throw new IOException("Saved export exceeds its bound");
        Cipher cipher=Cipher.getInstance("AES/GCM/NoPadding");cipher.init(Cipher.ENCRYPT_MODE,key);cipher.updateAAD(AAD);
        FileOutputStream out=null;try{out=file.startWrite();out.write(cipher.getIV());out.write(cipher.doFinal(plain));file.finishWrite(out);}catch(Exception error){if(out!=null)file.failWrite(out);throw error;}
    }
}
