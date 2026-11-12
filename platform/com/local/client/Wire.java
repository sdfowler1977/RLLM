package com.local.client;

import android.util.Base64;
import org.json.JSONObject;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.Arrays;

/** Platform stream I/O; the Rust response framer owns completion and bounds. */
final class Wire {
    interface Budget { void active() throws Exception; }
    static long upload(OutputStream output,JSONObject command,Budget budget)throws Exception{
        String purpose=command.getString("purpose"),ticket=command.getString("media_upload");long written=0,expected=command.getLong("upload_bytes");
        try{for(;;){budget.active();JSONObject part=(JSONObject)Core.call(new JSONObject().put("op","media_upload_read").put("purpose",purpose).put("ticket",ticket));byte[] bytes=Base64.decode(part.getString("data"),Base64.NO_WRAP);if(bytes.length>8192||bytes.length>expected-written)throw new IOException("Upload chunk exceeds its owned length");if(bytes.length==0&&!part.getBoolean("done"))throw new IOException("Upload made no progress");budget.active();output.write(bytes);written+=bytes.length;if(part.getBoolean("done")){if(written!=expected)throw new IOException("Upload ended before its owned length");return written;}}}
        finally{try{Core.call(new JSONObject().put("op","media_upload_cancel").put("purpose",purpose).put("ticket",ticket));}catch(Exception ignored){}}
    }
    static String download(InputStream input,String purpose,String ticket,Budget budget)throws Exception{
        try{byte[] buffer=new byte[8192];for(;;){budget.active();int count=input.read(buffer);budget.active();if(count<0)return (String)Core.call(new JSONObject().put("op","media_stream_finish").put("purpose",purpose).put("ticket",ticket).put("eof",true));if(count==0)throw new IOException("Artifact stream made no progress");JSONObject part=(JSONObject)Core.call(new JSONObject().put("op","media_stream_feed").put("purpose",purpose).put("ticket",ticket).put("data",Base64.encodeToString(Arrays.copyOf(buffer,count),Base64.NO_WRAP)));if(part.getBoolean("ready"))return (String)Core.call(new JSONObject().put("op","media_stream_finish").put("purpose",purpose).put("ticket",ticket).put("eof",false));}}
        finally{try{Core.call(new JSONObject().put("op","media_stream_cancel").put("purpose",purpose).put("ticket",ticket));}catch(Exception ignored){}}
    }
    static byte[] read(InputStream input,int limit,String owner,boolean head,Budget budget)throws Exception {
        long id=((Number)Core.call(new JSONObject().put("op","network_frame_open").put("owner",owner).put("limit",limit).put("head_only",head))).longValue();
        try {
            ByteArrayOutputStream bytes=new ByteArrayOutputStream();byte[] buffer=new byte[8192];
            for(;;){
                budget.active();int count=input.read(buffer);budget.active();
                if(count<0){Core.call(new JSONObject().put("op","network_frame_finish").put("owner",owner).put("id",id).put("eof",true));return bytes.toByteArray();}
                if(count==0)throw new IOException("Response stream made no progress");
                if(count>limit-bytes.size())throw new IOException("Response exceeds current memory bound");
                bytes.write(buffer,0,count);
                JSONObject part=(JSONObject)Core.call(new JSONObject().put("op","network_frame_feed").put("owner",owner).put("id",id).put("data",Base64.encodeToString(Arrays.copyOf(buffer,count),Base64.NO_WRAP)));
                if(part.getBoolean("ready")){Core.call(new JSONObject().put("op","network_frame_finish").put("owner",owner).put("id",id).put("eof",false));return bytes.toByteArray();}
            }
        }finally{try{Core.call(new JSONObject().put("op","network_frame_close").put("owner",owner).put("id",id));}catch(Exception ignored){}}
    }
}
