package com.local.client;

import android.graphics.Bitmap;
import android.os.Handler;
import android.os.Looper;
import android.view.MotionEvent;
import android.widget.Button;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.TextView;
import android.util.Base64;
import org.json.JSONObject;
import java.util.concurrent.ExecutorService;

/** Native controls and pixel presentation; Rust owns scene data and rendering. */
final class SceneView extends LinearLayout implements AutoCloseable {
    private final Activity activity;
    private final JSONObject owner;
    private final Handler handler=new Handler(Looper.getMainLooper());
    private final ExecutorService worker=new java.util.concurrent.ThreadPoolExecutor(1,1,0L,java.util.concurrent.TimeUnit.SECONDS,new java.util.concurrent.ArrayBlockingQueue<Runnable>(1));
    private final ImageView image;
    private final TextView status;
    private volatile boolean closed;
    private String ticket="";
    private boolean busy;
    private boolean hasFrame;
    private int generation;
    private double yaw=0.4,pitch=0.2,zoom=1.0;
    private float lastX,lastY;
    private final int size;
    private int renderSize;
    SceneView(Activity activity,JSONObject effect)throws Exception {
        super(activity);this.activity=activity;owner=new JSONObject(effect.toString());setOrientation(VERTICAL);
        size=Math.max(128,Math.min(640,getResources().getDisplayMetrics().widthPixels));renderSize=size;
        status=new TextView(activity);status.setText("Opening returned scene…");status.setPadding(dp(12),dp(8),dp(12),dp(8));addView(status,new LayoutParams(-1,-2));
        image=new ImageView(activity);image.setAdjustViewBounds(true);image.setScaleType(ImageView.ScaleType.FIT_CENTER);image.setBackgroundColor(0xff171b22);image.setContentDescription("3D geometry viewport. Drag to rotate, or use the labeled rotate and zoom buttons.");image.setFocusable(true);addView(image,new LayoutParams(-1,dp(280)));
        image.setOnTouchListener((v,event)->{if(closed)return false;switch(event.getActionMasked()){case MotionEvent.ACTION_DOWN:v.getParent().requestDisallowInterceptTouchEvent(true);lastX=event.getX();lastY=event.getY();return true;case MotionEvent.ACTION_MOVE:yaw+=(event.getX()-lastX)/160.0;pitch+=(event.getY()-lastY)/160.0;lastX=event.getX();lastY=event.getY();changed();return true;case MotionEvent.ACTION_UP:v.getParent().requestDisallowInterceptTouchEvent(false);v.performClick();return true;case MotionEvent.ACTION_CANCEL:v.getParent().requestDisallowInterceptTouchEvent(false);return true;default:return true;}});
        LinearLayout orbit=row();button(orbit,"Rotate left",()->{yaw-=0.25;changed();});button(orbit,"Rotate right",()->{yaw+=0.25;changed();});button(orbit,"Tilt up",()->{pitch-=0.25;changed();});
        LinearLayout camera=row();button(camera,"Tilt down",()->{pitch+=0.25;changed();});button(camera,"Zoom in",()->{zoom=Math.min(8.0,zoom*1.25);changed();});button(camera,"Zoom out",()->{zoom=Math.max(0.25,zoom/1.25);changed();});
        LinearLayout reset=row();button(reset,"Reset view",()->{yaw=0.4;pitch=0.2;zoom=1.0;changed();});
        busy=true;try{worker.execute(()->{String opened="";try{JSONObject result=(JSONObject)Core.call(request("scene_open",""));opened=result.getString("scene_ticket");final String value=opened;final int admitted=result.getInt("max_size");handler.post(()->{busy=false;if(closed||!activity.mediaCurrent(owner)){release(value);return;}ticket=value;renderSize=Math.max(128,Math.min(size,admitted));requestFrame();});}catch(Exception error){final String value=opened;handler.post(()->{busy=false;if(!value.isEmpty())release(value);if(!closed)status.setText("Scene preview unavailable: "+error.getMessage()+". The original file remains available for export.");});}});}catch(java.util.concurrent.RejectedExecutionException unavailable){busy=false;status.setText("Scene preview worker unavailable. The original file remains available for export.");}
    }
    private int dp(int value){return Math.round(value*getResources().getDisplayMetrics().density);}
    private LinearLayout row(){LinearLayout row=new LinearLayout(activity);row.setOrientation(HORIZONTAL);addView(row,new LayoutParams(-1,-2));return row;}
    private void button(LinearLayout row,String label,Runnable action){Button button=new Button(activity);button.setText(label);button.setContentDescription(label);button.setMinHeight(dp(48));button.setOnClickListener(v->{if(!closed)action.run();});row.addView(button,new LayoutParams(0,-2,1));}
    private JSONObject request(String op,String value)throws Exception{
        JSONObject result=new JSONObject().put("op",op).put("root",owner.get("root")).put("root_identity",owner.get("root_identity")).put("chat",owner.opt("chat")).put("record",owner.get("record")).put("scene_ticket",value).put("resource_config",owner.opt("resource_config"));
        if(op.equals("scene_open")){JSONObject device=new JSONObject().put("heap_bytes",Runtime.getRuntime().maxMemory()).put("cores",Runtime.getRuntime().availableProcessors());try{Object service=activity.getSystemService(android.content.Context.ACTIVITY_SERVICE);if(service instanceof android.app.ActivityManager){android.app.ActivityManager.MemoryInfo memory=new android.app.ActivityManager.MemoryInfo();((android.app.ActivityManager)service).getMemoryInfo(memory);device.put("memory_bytes",memory.availMem);}}catch(RuntimeException unavailable){}result.put("device",device);}return result;
    }
    private void changed(){generation++;requestFrame();}
    private void requestFrame(){
        if(closed||busy||ticket.isEmpty()||!activity.mediaCurrent(owner))return;
        final int current=generation,frameSize=renderSize;final String held=ticket;final double y=yaw,p=pitch,z=zoom;busy=true;
        try{worker.execute(()->{try{JSONObject result=(JSONObject)Core.call(request("scene_frame",held).put("yaw",y).put("pitch",p).put("zoom",z).put("size",frameSize));final int width=result.getInt("width"),height=result.getInt("height");if(width!=frameSize||height!=frameSize)throw new Exception("Unexpected viewport dimensions");byte[] rgba=Base64.decode(result.getString("rgba"),Base64.DEFAULT);if(rgba.length!=width*height*4)throw new Exception("Incomplete viewport pixels");final int[] colors=new int[width*height];for(int i=0;i<colors.length;i++){int n=i*4;colors[i]=((rgba[n+3]&255)<<24)|((rgba[n]&255)<<16)|((rgba[n+1]&255)<<8)|(rgba[n+2]&255);}final String description=result.getInt("triangles")+" triangles. "+result.getString("description");handler.post(()->{busy=false;if(closed||!held.equals(ticket)||!activity.mediaCurrent(owner))return;if(current==generation){Bitmap bitmap=Bitmap.createBitmap(colors,width,height,Bitmap.Config.ARGB_8888);image.setImageBitmap(bitmap);hasFrame=true;status.setText(description);}else requestFrame();});}catch(Exception error){handler.post(()->{busy=false;if(!closed&&held.equals(ticket)){status.setText((hasFrame?"Previous view retained. ":"Scene preview unavailable. ")+error.getMessage()+". The original file remains available for export.");if(current!=generation)requestFrame();}});}});}catch(java.util.concurrent.RejectedExecutionException unavailable){busy=false;if(!closed)status.setText("Preview worker unavailable; original file remains available for export.");}
    }
    private void release(String value){if(value.isEmpty())return;try{Core.call(request("scene_close",value));}catch(Exception ignored){}}
    @Override public void close(){if(closed)return;closed=true;generation++;String value=ticket;ticket="";release(value);worker.shutdownNow();image.setImageDrawable(null);}
}
