package com.local.client;

import android.util.Base64;
import org.json.JSONObject;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.util.Arrays;

/** Platform stream I/O; the Rust response framer owns completion and bounds. */
final class Wire {
    interface Budget { void active() throws Exception; }
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
