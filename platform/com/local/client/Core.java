package com.local.client;
import org.json.JSONObject;
import java.nio.charset.StandardCharsets;
final class Core {
    static { System.loadLibrary("client"); }
    private static native byte[] process(byte[] input);
    static Object call(JSONObject input) throws Exception {
        byte[] bytes=process(input.toString().getBytes(StandardCharsets.UTF_8));
        if(bytes==null) throw new Exception("Unable to allocate core response");
        JSONObject result=new JSONObject(new String(bytes,StandardCharsets.UTF_8));
        if(result.has("error")) throw new Exception(result.getString("error"));
        return result.get("value");
    }
}
