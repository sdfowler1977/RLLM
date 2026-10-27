package com.local.client;

import android.app.AlertDialog;
import android.os.Bundle;
import android.os.Messenger;
import android.os.Message;
import android.os.Handler;
import android.os.Looper;
import android.os.IBinder;
import android.content.ServiceConnection;
import android.content.ComponentName;
import android.content.Intent;
import android.graphics.Color;
import android.graphics.Typeface;
import android.net.Uri;
import android.provider.DocumentsContract;
import android.database.Cursor;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.text.InputType;
import android.util.AtomicFile;
import android.util.Base64;
import android.view.Gravity;
import android.view.View;
import android.view.WindowManager;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.graphics.Insets;
import android.widget.*;
import java.io.*;
import java.net.*;
import java.nio.ByteBuffer;
import java.nio.charset.*;
import java.security.KeyStore;
import java.util.*;
import java.util.concurrent.*;
import javax.crypto.*;
import javax.crypto.spec.GCMParameterSpec;
import javax.net.ssl.*;
import org.json.*;

/** Android platform bindings. Product state, protocols and workspace operations reside in Rust. */
public final class Activity extends android.app.Activity {
    private JSONObject state;
    private final Handler rateHandler=new Handler(Looper.getMainLooper());
    private Runnable rateTick;
    private boolean ratePaused;
    private long rateStarted;
    private String rateTicket="";
    private Button rateBadge;
    private TextView rateValue,rateCaption;
    private Diagram rateDiagram;

    private final Map<String,View> fields=new LinkedHashMap<>();
    private final ExecutorService workers=Executors.newFixedThreadPool(Math.max(2,Math.min(4,Runtime.getRuntime().availableProcessors())));
    private final Set<Socket> activeConnections=ConcurrentHashMap.newKeySet();
    private final Set<Socket> cleanupConnections=ConcurrentHashMap.newKeySet();
    private final ThreadPoolExecutor cleanupWorkers=new ThreadPoolExecutor(1,1,10,TimeUnit.SECONDS,new ArrayBlockingQueue<Runnable>(4));
    private final Set<android.os.CancellationSignal> documentSignals=ConcurrentHashMap.newKeySet();
    private final Set<android.os.ParcelFileDescriptor> documentFiles=ConcurrentHashMap.newKeySet();
    private final Set<android.os.CancellationSignal> exportSignals=ConcurrentHashMap.newKeySet();
    private final Set<android.os.ParcelFileDescriptor> exportFiles=ConcurrentHashMap.newKeySet();
    private final ScheduledExecutorService deadlines=Executors.newSingleThreadScheduledExecutor();
    private volatile int generation=0;
    private volatile String appOwner="";
    private AlertDialog dialog;
    private EditText draft;
    private boolean rendering=false;
    private boolean canNavigateBack=false;
    private volatile boolean closed=false;
    private int ink=Color.rgb(24,39,55),surface=Color.rgb(244,247,249);
    private int accent=Color.rgb(0,105,110);
    private final android.graphics.Bitmap[] wallpaperCache=new android.graphics.Bitmap[2];
    private final Map<String,Integer> pageOffsets=new HashMap<>(),pageSizes=new HashMap<>();
    private LinearLayout currentRoot;
    private int viewportWidth,viewportHeight,insetLeft,insetTop,insetRight,insetBottom;
    private Runnable viewportUpdate;
    private AtomicFile vault;
    private String pendingExportTicket;
    private ExportStore exportStore;
    private TransferStore transferStore;
    // One process-wide platform IO lane serializes export storage and destination writes across Activities.
    private static final ExecutorService documentJobs=Executors.newSingleThreadExecutor();
    private static final ExecutorService appJobs=new ThreadPoolExecutor(0,1,30,TimeUnit.SECONDS,new SynchronousQueue<Runnable>());
    private static final Set<String> cancelledExports=ConcurrentHashMap.newKeySet();
    private byte[] pendingImport;
    private ServiceConnection executionConnection;
    private Messenger executionMessenger;
    private float textScale=1f; private Typeface proseFont=Typeface.create("sans-serif",Typeface.NORMAL);
    private android.graphics.Bitmap customWallpaper; private String customWallpaperUri="";
    private int dp(int n){return (int)(getResources().getDisplayMetrics().density*n+.5f);}
    private String root(){return new File(getFilesDir(),"workspace").getAbsolutePath();}
    @Override public void onCreate(Bundle bundle){super.onCreate(bundle);
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        if(android.os.Build.VERSION.SDK_INT>=30)getWindow().setDecorFitsSystemWindows(false);
        getWindow().setStatusBarColor(surface);getWindow().setNavigationBarColor(surface);
        getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR|View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR);
        vault=new AtomicFile(new File(getFilesDir(),"state.dat"));
        exportStore=new ExportStore(getFilesDir());
        transferStore=new TransferStore(getFilesDir());
        pendingExportTicket=bundle==null?null:bundle.getString("export_ticket");
        try {state=load();documentJobs.execute(()->{JSONObject recovered=new JSONObject();try{JSONObject record=exportStore.load(key());recovered.put("export_record",record==null?new JSONObject():ExportStore.metadata(record)).put("export_empty",record==null);}catch(Exception problem){try{recovered.put("export_error",safe(problem));}catch(JSONException ignored){}}try{transferRecovery(recovered);}catch(Exception problem){try{recovered.put("project_error",safe(problem));}catch(JSONException ignored){}}runOnUiThread(()->{if(!closed)dispatch("resume",recovered);});});}
        catch(Throwable e){fatal("Saved state could not be opened. It has not been overwritten.\n"+safe(e));}
    }
    private String safe(Throwable e){return e.getMessage()==null?e.getClass().getSimpleName():e.getMessage();}
    private void fatal(String text){TextView v=label(text,18);v.setPadding(dp(24),dp(40),dp(24),dp(24));setContentView(v);}
    private SecretKey key() throws Exception {
        KeyStore store=KeyStore.getInstance("AndroidKeyStore");store.load(null);
        if(!store.containsAlias("client.state")){KeyGenerator gen=KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES,"AndroidKeyStore");gen.init(new KeyGenParameterSpec.Builder("client.state",KeyProperties.PURPOSE_ENCRYPT|KeyProperties.PURPOSE_DECRYPT).setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).setKeySize(256).build());gen.generateKey();}
        return (SecretKey)store.getKey("client.state",null);
    }
    private JSONObject load() throws Exception {
        if(!vault.getBaseFile().exists()&&!new File(vault.getBaseFile()+".bak").exists())return null;
        byte[] bytes;try(InputStream in=vault.openRead()){bytes=read(in,8*1024*1024);}
        if(bytes.length<29)throw new IOException("Encrypted state is incomplete");
        Cipher cipher=Cipher.getInstance("AES/GCM/NoPadding");cipher.init(Cipher.DECRYPT_MODE,key(),new GCMParameterSpec(128,Arrays.copyOfRange(bytes,0,12)));
        return new JSONObject(new String(cipher.doFinal(Arrays.copyOfRange(bytes,12,bytes.length)),StandardCharsets.UTF_8));
    }
    private void save(JSONObject next) throws Exception {
        byte[] plain=next.toString().getBytes(StandardCharsets.UTF_8);if(plain.length>6*1024*1024)throw new IOException("Encrypted history reached its 6 MiB limit. The previous saved state remains intact.");
        Cipher cipher=Cipher.getInstance("AES/GCM/NoPadding");cipher.init(Cipher.ENCRYPT_MODE,key());
        FileOutputStream out=null;try{out=vault.startWrite();out.write(cipher.getIV());out.write(cipher.doFinal(plain));vault.finishWrite(out);}catch(Exception e){if(out!=null)vault.failWrite(out);throw e;}
    }
    private boolean phoneAccessGranted(){
        if(android.os.Build.VERSION.SDK_INT>=30)return android.os.Environment.isExternalStorageManager();
        return checkSelfPermission("android.permission.READ_EXTERNAL_STORAGE")==android.content.pm.PackageManager.PERMISSION_GRANTED && checkSelfPermission("android.permission.WRITE_EXTERNAL_STORAGE")==android.content.pm.PackageManager.PERMISSION_GRANTED;
    }
    private void requestPhoneAccess(){
        if(android.os.Build.VERSION.SDK_INT>=30){
            Intent intent=new Intent("android.settings.MANAGE_APP_ALL_FILES_ACCESS_PERMISSION",Uri.parse("package:"+getPackageName()));
            try{startActivityForResult(intent,48);}catch(android.content.ActivityNotFoundException unavailable){startActivityForResult(new Intent("android.settings.MANAGE_ALL_FILES_ACCESS_PERMISSION"),48);}
        }else requestPermissions(new String[]{"android.permission.READ_EXTERNAL_STORAGE","android.permission.WRITE_EXTERNAL_STORAGE"},48);
    }
    @Override public void onRequestPermissionsResult(int code,String[] permissions,int[] grants){super.onRequestPermissionsResult(code,permissions,grants);if(code==48)dispatch("phone_access_status");}
    private Object phoneReadOrDocument(JSONObject command,int token)throws Exception {
        if(!command.optString("phone_scope").equals("shared"))return phoneTool(command,token);
        String tool=command.getString("tool");
        if(!tool.equals("phone_read")&&!tool.equals("phone_list"))throw new IOException("Shared-storage operation is unsupported");
        if(token!=generation||closed)throw new IOException("Cancelled");
        if(!phoneAccessGranted())throw new IOException("Android broad file access is not granted; enable it or choose limited browsing");
        JSONObject request=new JSONObject(command.toString()).put("op","tool").put("tool",tool.equals("phone_read")?"read":"list").put("root",android.os.Environment.getExternalStorageDirectory().getAbsolutePath());
        Object result=Core.call(request);if(token!=generation||closed)throw new IOException("Cancelled");return result;
    }
    private void appObservationActive(int token)throws IOException{if(token!=generation||closed)throw new IOException("App observation cancelled");}
    private String appOwnerFor(JSONObject current){
        if(current==null||!current.optBoolean("apps_enabled",true)||!current.optBoolean("busy"))return "";
        JSONObject expected=current.optJSONObject("apps_expected");if(expected==null)return "";
        String selected=expected.optString("package"),operation=expected.optString("operation");
        if(!selected.isEmpty()){
            if(!selected.equals(current.optString("apps_selected")))return "";
            JSONArray excluded=current.optJSONArray("apps_excluded");if(excluded!=null)for(int n=0;n<excluded.length();n++)if(selected.equals(excluded.optString(n)))return "";
        }
        if(operation.equals("usage")&&!current.optBoolean("apps_usage_enabled"))return "";
        return expected.optString("ticket");
    }
    private void appObservationOwner(JSONObject command,int token)throws IOException{
        appObservationActive(token);String ticket=command.optString("ticket");if(ticket.isEmpty()||!ticket.equals(appOwner))throw new IOException("App observation no longer selected");
    }
    private void usageAccess()throws IOException{
        if(android.os.Build.VERSION.SDK_INT<35)throw new IOException("Selected-app usage requires Android 15 or later; package metadata remains available");
        android.app.AppOpsManager ops=(android.app.AppOpsManager)getSystemService(APP_OPS_SERVICE);
        if(ops==null)throw new IOException("Android usage access status unavailable");
        int mode=ops.checkOpNoThrow(android.app.AppOpsManager.OPSTR_GET_USAGE_STATS,android.os.Process.myUid(),getPackageName());
        if(mode!=android.app.AppOpsManager.MODE_ALLOWED&&(mode!=android.app.AppOpsManager.MODE_DEFAULT||checkSelfPermission("android.permission.PACKAGE_USAGE_STATS")!=android.content.pm.PackageManager.PERMISSION_GRANTED))throw new IOException("Android usage access is not granted; open Android usage settings");
        android.os.UserManager user=(android.os.UserManager)getSystemService(USER_SERVICE);
        if(user==null||!user.isUserUnlocked())throw new IOException("Usage history unavailable while Android user storage is locked");
    }
    private void usageActive(JSONObject command,int token)throws Exception{appObservationOwner(command,token);usageAccess();appObservationOwner(command,token);}
    private JSONObject appBinding(JSONObject command)throws JSONException{
        JSONObject result=new JSONObject().put("ticket",command.getString("ticket")).put("package",command.getString("package")).put("operation",command.getString("operation"));
        if(command.optString("operation").equals("usage"))result.put("begin_ms",command.getString("begin_ms")).put("end_ms",command.getString("end_ms"));return result;
    }
    private static String appLabel(String value){
        StringBuilder out=new StringBuilder();int bytes=0;for(int offset=0;offset<value.length();){int cp=value.codePointAt(offset);String piece=new String(Character.toChars(cp));int size=piece.getBytes(StandardCharsets.UTF_8).length;if(bytes+size>253){out.append("…");break;}out.append(piece);bytes+=size;offset+=Character.charCount(cp);}return out.toString();
    }
    private JSONObject appObservation(JSONObject command,int token)throws Exception{
        appObservationActive(token);String operation=command.getString("operation"),selected=command.getString("package");android.content.pm.PackageManager pm=getPackageManager();
        if(operation.equals("catalog")){
            if(!selected.isEmpty())throw new IOException("Unexpected catalog package");
            java.util.List<android.content.pm.ApplicationInfo> apps=pm.getInstalledApplications(0);if(apps.size()>1024)throw new IOException("App catalog exceeds1024 packages; no partial catalog accepted");JSONArray entries=new JSONArray();
            for(android.content.pm.ApplicationInfo app:apps){appObservationActive(token);entries.put(new JSONObject().put("package",app.packageName).put("label",appLabel(String.valueOf(pm.getApplicationLabel(app)))).put("system",(app.flags&android.content.pm.ApplicationInfo.FLAG_SYSTEM)!=0));}
            appObservationActive(token);return new JSONObject().put("entries",entries);
        }
        if(!operation.equals("details")||!selected.matches("[A-Za-z0-9_.]{1,255}"))throw new IOException("Invalid selected package");
        android.content.pm.PackageInfo info=pm.getPackageInfo(selected,android.content.pm.PackageManager.GET_PERMISSIONS);appObservationActive(token);
        if(!selected.equals(info.packageName)||info.applicationInfo==null)throw new IOException("Package identity unavailable");
        String[] names=info.requestedPermissions;int[] flags=info.requestedPermissionsFlags;if(names!=null&&names.length>512)throw new IOException("Package exceeds512 requested permissions; no partial observation accepted");
        JSONArray permissions=new JSONArray();if(names!=null)for(int index=0;index<names.length;index++){appObservationActive(token);Object granted=flags!=null&&flags.length==names.length?Boolean.valueOf((flags[index]&android.content.pm.PackageInfo.REQUESTED_PERMISSION_GRANTED)!=0):JSONObject.NULL;permissions.put(new JSONObject().put("name",names[index]).put("granted",granted));}
        long code=android.os.Build.VERSION.SDK_INT>=28?info.getLongVersionCode():Integer.toUnsignedLong(info.versionCode);
        return new JSONObject().put("package",info.packageName).put("version",info.versionName==null?"":info.versionName).put("version_code",Long.toString(code)).put("uid",info.applicationInfo.uid).put("system",(info.applicationInfo.flags&android.content.pm.ApplicationInfo.FLAG_SYSTEM)!=0).put("observed_ms",Long.toString(System.currentTimeMillis())).put("permissions",permissions);
    }
    private JSONObject inputs(){JSONObject out=new JSONObject();try{if(draft!=null)out.put("draft",draft.getText().toString());for(Map.Entry<String,View> e:fields.entrySet()){View v=e.getValue();if(v instanceof android.widget.Spinner){Object item=((android.widget.Spinner)v).getSelectedItem();JSONArray choices=(JSONArray)v.getTag();int selected=((android.widget.Spinner)v).getSelectedItemPosition();if(selected>=0&&selected<choices.length())out.put(e.getKey(),choices.getJSONObject(selected).getString("value"));}else if(v instanceof CheckBox)out.put(e.getKey(),((CheckBox)v).isChecked());else if(v instanceof EditText)out.put(e.getKey(),((EditText)v).getText().toString());}}catch(JSONException ignored){throw new IllegalStateException(ignored);}return out;}
    private volatile String mcpPauseOwner="";
    private void dispatch(String action){dispatch(action,new JSONObject());}
    private void dispatch(String action,JSONObject extra){if(closed)return;JSONObject plannedEffect=null;boolean stateCommitted=false,effectStarted=false;
        try{JSONObject in=inputs();if(action.equals("apps_usage_day")||action.equals("apps_usage_week"))in.put("usage_now_ms",Long.toString(System.currentTimeMillis()));android.app.ActivityManager.MemoryInfo memory=new android.app.ActivityManager.MemoryInfo();((android.app.ActivityManager)getSystemService(ACTIVITY_SERVICE)).getMemoryInfo(memory);in.put("device",new JSONObject().put("memory_bytes",memory.totalMem).put("heap_bytes",Runtime.getRuntime().maxMemory()).put("cores",Runtime.getRuntime().availableProcessors()).put("abi",android.os.Build.SUPPORTED_ABIS[0]));in.put("phone_access_granted",phoneAccessGranted());in.put("gateway_status",Gateway.status);if(action.equals("gateway_edit")){byte[] entropy=new byte[32];new java.security.SecureRandom().nextBytes(entropy);in.put("generated_token",Base64.encodeToString(entropy,Base64.NO_WRAP));}if(action.equals("cancel")&&!rateTicket.isEmpty()&&rateTicket.equals(state.optString("model_expected")))in.put("elapsed_ms",android.os.SystemClock.elapsedRealtime()-rateStarted);Iterator<String> keys=extra.keys();while(keys.hasNext()){String k=keys.next();in.put(k,extra.get(k));}
            JSONObject request=new JSONObject().put("op","event").put("action",action).put("state",state==null?JSONObject.NULL:state).put("inputs",in).put("root",root());
            JSONObject result=(JSONObject)Core.call(request);JSONObject next=result.getJSONObject("state");plannedEffect=result.optJSONObject("effect");
            boolean progress=action.equals("model_progress")||action.equals("rate_tick")||action.equals("rea_server_progress");if(!progress)save(next);state=next;appOwner=appOwnerFor(next);stateCommitted=true;if(progress){if(action.equals("rate_tick"))renderRate(result.getJSONObject("ui"));else if(action.equals("rea_server_progress"))renderMcpProgress(result.getJSONObject("ui"));else renderProgress(result.getJSONObject("ui"));}else render(result.getJSONObject("ui"));
            JSONObject effect=plannedEffect;if(effect!=null){effect(effect);effectStarted=true;}
        }catch(Throwable e){if(!effectStarted&&plannedEffect!=null){boolean opening=abortMcpOpening(plannedEffect,stateCommitted);if(opening&&stateCommitted&&!closed)try{dispatch("failure",new JSONObject().put("purpose",plannedEffect.optString("purpose")).put("error",safe(e)));}catch(Exception ignored){}if(stateCommitted&&!closed&&plannedEffect.optString("purpose").equals("web_tool"))try{dispatch("failure",new JSONObject().put("purpose","web_tool").put("web_ticket",plannedEffect.optString("web_ticket")).put("error","Web operation could not finish starting: "+safe(e)).put("sent",true));}catch(Exception ignored){}}new AlertDialog.Builder(this).setTitle("Operation not completed").setMessage(safe(e)).setPositiveButton("Close",null).show();}
    }
    private boolean abortMcpOpening(JSONObject effect,boolean committed){
        boolean mcpEffect=effect.optString("kind").equals("network")&&effect.optString("purpose").startsWith("rea_");JSONArray aborted=effect.optJSONArray("mcp_abort_cleanup");effect.remove("mcp_abort_cleanup");if(!committed&&aborted!=null&&!closed)queueMcpCleanup(aborted,android.os.SystemClock.elapsedRealtime()+2000);
        JSONArray attached=effect.optJSONArray("mcp_cleanup");effect.remove("mcp_cleanup");if(attached!=null&&!closed)queueMcpCleanup(attached,android.os.SystemClock.elapsedRealtime()+2000);
        JSONObject context=effect.optJSONObject("mcp_control");if(context==null||(!context.has("admission_token")&&!context.has("resume_token")))return mcpEffect;
        try{JSONArray cleanup=(JSONArray)Core.call(new JSONObject().put("op","stream_open_cleanup").put("context",context));if(!closed)queueMcpCleanup(cleanup,android.os.SystemClock.elapsedRealtime()+2000);}catch(Exception ignored){}return true;
    }
    private TextView label(String text,int size){TextView v=new TextView(this);v.setText(text);v.setTextSize(size*textScale);v.setTypeface(proseFont);v.setTextColor(ink);v.setPadding(dp(8),dp(3),dp(8),dp(3));return v;}
    private LinearLayout column(){LinearLayout v=new LinearLayout(this);v.setOrientation(LinearLayout.VERTICAL);return v;}
    private LinearLayout row(){LinearLayout v=new LinearLayout(this);v.setOrientation(LinearLayout.HORIZONTAL);v.setGravity(Gravity.CENTER_VERTICAL);return v;}
    private Button button(String text,Runnable action){Button b=new Button(this);b.setFilterTouchesWhenObscured(true);b.setText(text);b.setTextSize(12*textScale);b.setTypeface(proseFont);b.setAllCaps(false);b.setTextColor(accent);b.setMinHeight(dp(48));b.setPadding(dp(8),0,dp(8),0);b.setBackground(new android.graphics.drawable.RippleDrawable(android.content.res.ColorStateList.valueOf((accent&0x00ffffff)|0x33000000),framed(card,10),null));b.setOnClickListener(v->action.run());return b;}
    private void stretch(LinearLayout parent,View child){parent.addView(child,new LinearLayout.LayoutParams(0,dp(48),1));}
    private void showPage(LinearLayout main,ArrayList<String> labels,ArrayList<String> events,String page,int offset,int pageSize,int lines,boolean busy){
        int last=labels.isEmpty()?0:((labels.size()-1)/pageSize)*pageSize;
        int start=Math.max(0,Math.min(last,(offset/pageSize)*pageSize));
        pageOffsets.put(page,start);
        main.removeAllViews();main.setBackgroundColor(surface);
        if(labels.isEmpty()){TextView empty=label(page.equals("chat")?"No messages yet":"No items on this page",14);empty.setGravity(Gravity.CENTER);main.addView(empty,new LinearLayout.LayoutParams(-1,0,1));}
        for(int i=start;i<Math.min(start+pageSize,labels.size());i++){
            final int index=i;String full=labels.get(i);
            TextView item=label(full,14);item.setMaxLines(lines);item.setEllipsize(android.text.TextUtils.TruncateAt.END);
            item.setGravity(Gravity.CENTER_VERTICAL);item.setPadding(dp(12),dp(4),dp(12),dp(4));
            item.setContentDescription(full);item.setClickable(true);item.setFocusable(true);
            item.setBackground(panel((i%2==0)?card:surface,12));
            item.setTooltipText(full);item.setEnabled(!busy||page.equals("chat")||Arrays.asList("configuration","configuration_export","export_review").contains(events.get(index)));item.setAlpha(item.isEnabled()?1f:.45f);
            item.setOnClickListener(v->{if(page.equals("chat"))dispatch("branch:"+index);else if(events.get(index).equals("conversation_export"))export();else dispatch(events.get(index));});
            if(page.equals("chat"))item.setOnLongClickListener(v->{((android.content.ClipboardManager)getSystemService(CLIPBOARD_SERVICE)).setPrimaryClip(android.content.ClipData.newPlainText("Message",full));Toast.makeText(this,"Message copied",Toast.LENGTH_SHORT).show();return true;});
            main.addView(item,new LinearLayout.LayoutParams(-1,0,1));
        }
        LinearLayout pager=row();ImageButton previous=icon("arrow_left","Previous",()->showPage(main,labels,events,page,start-pageSize,pageSize,lines,busy));
        previous.setEnabled(start>0);pager.addView(previous,new LinearLayout.LayoutParams(dp(48),dp(48)));
        TextView counter=label(labels.isEmpty()?"0/0":((start/pageSize)+1)+"/"+((labels.size()+pageSize-1)/pageSize),10);
        counter.setContentDescription(labels.isEmpty()?"No pages":"Page "+((start/pageSize)+1)+" of "+((labels.size()+pageSize-1)/pageSize));counter.setPadding(0,0,0,0);counter.setSingleLine(true);counter.setEllipsize(android.text.TextUtils.TruncateAt.END);
        counter.setGravity(Gravity.CENTER);pager.addView(counter,new LinearLayout.LayoutParams(0,dp(48),1));
        ImageButton next=icon("arrow_right","Next",()->showPage(main,labels,events,page,start+pageSize,pageSize,lines,busy));
        next.setEnabled(start+pageSize<labels.size());pager.addView(next,new LinearLayout.LayoutParams(dp(48),dp(48)));main.addView(pager);
    }
    private ImageButton icon(String resource,String description,Runnable action){
        ImageButton button=new ImageButton(this);button.setFilterTouchesWhenObscured(true);button.setContentDescription(description);button.setTooltipText(description);button.setImageResource(getResources().getIdentifier(resource,"drawable",getPackageName()));button.setImageTintList(android.content.res.ColorStateList.valueOf(accent));button.setBackground(panel(Color.TRANSPARENT,24));button.setScaleType(ImageView.ScaleType.CENTER_INSIDE);button.setPadding(dp(12),dp(12),dp(12),dp(12));button.setMinimumWidth(dp(48));button.setMinimumHeight(dp(48));button.setOnClickListener(v->action.run());return button;
    }
    private int lineHeight(int size){TextView probe=new TextView(this);probe.setText("Ag 中文 ا");probe.setTextSize(size*textScale);probe.setTypeface(proseFont);probe.measure(View.MeasureSpec.makeMeasureSpec(0,View.MeasureSpec.UNSPECIFIED),View.MeasureSpec.makeMeasureSpec(0,View.MeasureSpec.UNSPECIFIED));return (int)Math.ceil(probe.getMeasuredHeight()/getResources().getDisplayMetrics().density);}
    private JSONObject layout(JSONObject ui)throws Exception{
        android.content.res.Configuration configuration=getResources().getConfiguration();
        TextView probe=new TextView(this);probe.setTextSize(12);int tab=(int)Math.ceil(probe.getPaint().measureText("Workspace")/getResources().getDisplayMetrics().density)+16;
        return (JSONObject)Core.call(new JSONObject().put("op","layout").put("width",viewportWidth>0?viewportWidth:Math.max(160,configuration.screenWidthDp-16)).put("height",viewportHeight>0?viewportHeight:Math.max(160,configuration.screenHeightDp-80)).put("small_line",lineHeight(10)).put("body_line",lineHeight(14)).put("composer_line",lineHeight(16)).put("title_line",lineHeight(16)).put("tab_width",tab).put("chat",ui.optString("page").equals("chat")));
    }
    private void queueViewport(){
        if(currentRoot==null||closed)return;View decor=getWindow().getDecorView();if(viewportUpdate!=null)decor.removeCallbacks(viewportUpdate);
        viewportUpdate=()->{if(currentRoot==null||closed)return;float density=getResources().getDisplayMetrics().density;int width=Math.round((currentRoot.getWidth()-currentRoot.getPaddingLeft()-currentRoot.getPaddingRight())/density),height=Math.round((currentRoot.getHeight()-currentRoot.getPaddingTop()-currentRoot.getPaddingBottom())/density);if(width<=0||height<=0)return;if(width==viewportWidth&&(dialog!=null||(draft!=null&&draft.hasFocus()))){viewportHeight=height;if(draft!=null){draft.setMaxLines(height<350?1:3);try{applyChatGeometry(layout(new JSONObject().put("page","chat")));}catch(Exception problem){dispatch("viewport");}}return;}if(width!=viewportWidth||height!=viewportHeight){viewportWidth=width;viewportHeight=height;dispatch("viewport");}};decor.postDelayed(viewportUpdate,120);
    }
    private TextView summary(String text,int size,int lines,String description){TextView view=label(text,size);view.setPadding(dp(4),0,dp(4),0);view.setMaxLines(lines);view.setEllipsize(android.text.TextUtils.TruncateAt.END);view.setContentDescription(description);view.setTooltipText(description);return view;}
    private final Map<String,int[]> conversationScroll=new HashMap<>();
    private ListView conversationList;
    private JSONArray displayedMessages;
    private TextView statusView;
    private View chatBanner;private TextView conversationHeading;private LinearLayout activeComposer,modelFooter;private ImageButton composerMenu;private String composerModel="";private boolean composerBusy;
    private String displayedConversation="";
    private int card=0xFF142535,muted=0xFF9CB0BF,glass=0xC21B293A,border=0x809AB6BE;
    private android.graphics.drawable.GradientDrawable panel(int color,int radius){android.graphics.drawable.GradientDrawable d=new android.graphics.drawable.GradientDrawable();d.setColor(color);d.setCornerRadius(dp(radius));return d;}
    private android.graphics.drawable.GradientDrawable framed(int color,int radius){android.graphics.drawable.GradientDrawable d=panel(color,radius);d.setStroke(Math.max(1,dp(1)),border);return d;}
    private void rememberScroll(){if(conversationList!=null&&conversationList.getChildCount()>0){View first=conversationList.getChildAt(0),last=conversationList.getChildAt(conversationList.getChildCount()-1);boolean bottom=conversationList.getLastVisiblePosition()==conversationList.getCount()-1&&last.getBottom()<=conversationList.getHeight();conversationScroll.put(displayedConversation,new int[]{conversationList.getFirstVisiblePosition(),first.getTop(),bottom?1:0,conversationList.getCount()});}}
    private void renderRate(JSONObject ui)throws Exception{
        JSONObject rate=ui.optJSONObject("local_rate");
        if(rateBadge!=null&&rate!=null){rateBadge.setText(rate.optString("label","— tok/s"));rateBadge.setContentDescription(rate.optString("description"));rateBadge.setTooltipText(rate.optString("description"));}
        JSONObject monitor=ui.optJSONObject("monitor");JSONArray gauges=monitor==null?null:monitor.optJSONArray("gauges");
        if(rateValue!=null&&gauges!=null)for(int i=0;i<gauges.length();i++){JSONObject g=gauges.getJSONObject(i);if(!g.optString("id").equals("local_rate"))continue;rateValue.setText(g.optString("value"));rateCaption.setText(g.optString("caption"));if(rateDiagram!=null)rateDiagram.update(g.getJSONObject("visual"));}
    }
    private void renderMcpProgress(JSONObject ui){
        if(statusView!=null){String value=ui.optString("visible_status");statusView.setText(value);statusView.setContentDescription(value);statusView.setVisibility(value.isEmpty()?View.GONE:View.VISIBLE);}
    }
    private void renderProgress(JSONObject ui)throws Exception{
        renderRate(ui);
        if(statusView!=null){statusView.setText(ui.optString("visible_status"));statusView.setContentDescription(ui.optString("visible_status"));statusView.setVisibility(ui.optString("visible_status").isEmpty()?View.GONE:View.VISIBLE);}
        if(conversationList==null||displayedMessages==null||!ui.optString("page").equals("chat"))return;
        rememberScroll();int[] prior=conversationScroll.get(displayedConversation);JSONArray next=ui.getJSONArray("messages");
        while(displayedMessages.length()>0)displayedMessages.remove(displayedMessages.length()-1);
        for(int i=0;i<next.length();i++)displayedMessages.put(next.get(i));
        ((BaseAdapter)conversationList.getAdapter()).notifyDataSetChanged();
        if(prior==null||prior[2]==1)conversationList.setSelection(Math.max(0,displayedMessages.length()-1));
    }
    private void render(JSONObject ui) throws Exception {
        boolean composerFocus=draft!=null&&draft.hasFocus();int selection=draft==null?0:Math.max(0,draft.getSelectionStart());rememberScroll();conversationList=null;displayedMessages=null;statusView=null;chatBanner=null;conversationHeading=null;activeComposer=null;modelFooter=null;composerMenu=null;rateBadge=null;rateValue=null;rateCaption=null;rateDiagram=null;
        rendering=true;if(dialog!=null){dialog.dismiss();dialog=null;}fields.clear();draft=null;
        JSONObject appearance=ui.optJSONObject("appearance");if(appearance==null)appearance=new JSONObject();textScale=Math.max(1f,Math.min(2f,appearance.optInt("scale",100)/100f));proseFont=Typeface.create(appearance.optString("font","sans-serif"),Typeface.NORMAL);
        canNavigateBack=ui.optBoolean("can_navigate_back");JSONObject geometry=layout(ui);String page=ui.optString("page");boolean busy=ui.optBoolean("busy"),dark=ui.optString("theme").equals("dark");
        JSONObject palette=ui.getJSONObject("palette");ink=Color.parseColor(palette.getString("ink"));surface=Color.parseColor(palette.getString("surface"));card=Color.parseColor(palette.getString("card"));muted=Color.parseColor(palette.getString("muted"));accent=Color.parseColor(palette.getString("accent"));glass=Color.parseColor(palette.getString("glass"));border=Color.parseColor(palette.getString("border"));
        getWindow().setStatusBarColor(surface);getWindow().setNavigationBarColor(surface);
        LinearLayout outer=new LinearLayout(this){@Override protected void onSizeChanged(int w,int h,int oldw,int oldh){super.onSizeChanged(w,h,oldw,oldh);queueViewport();}};
        outer.setOrientation(LinearLayout.VERTICAL);outer.setBackgroundColor(Color.TRANSPARENT);outer.setPadding(dp(8)+insetLeft,insetTop,dp(8)+insetRight,dp(8)+insetBottom);currentRoot=outer;
        setTitle("RLLM · "+ui.optString("title"));
        FrameLayout scene=new FrameLayout(this);scene.setBackgroundColor(surface);ImageView wallpaper=new ImageView(this);int wi=dark?1:0;if(wallpaperCache[wi]==null){android.graphics.BitmapFactory.Options opts=new android.graphics.BitmapFactory.Options();opts.inSampleSize=2;wallpaperCache[wi]=android.graphics.BitmapFactory.decodeResource(getResources(),dark?R.drawable.wallpaper_dark:R.drawable.wallpaper_light,opts);}wallpaper.setImageBitmap(wallpaperCache[wi]);String background=appearance.optString("background","waves");if(background.equals("solid"))wallpaper.setVisibility(View.GONE);else if(background.equals("image")&&!appearance.optString("wallpaper").isEmpty()){String imageUri=appearance.optString("wallpaper");if(imageUri.equals(customWallpaperUri)&&customWallpaper!=null)wallpaper.setImageBitmap(customWallpaper);else loadWallpaper(imageUri,wallpaper);}wallpaper.setScaleType(ImageView.ScaleType.CENTER_CROP);wallpaper.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO);scene.addView(wallpaper,new FrameLayout.LayoutParams(-1,-1));View veil=new View(this);veil.setBackgroundColor(Color.parseColor(palette.getString("veil")));scene.addView(veil,new FrameLayout.LayoutParams(-1,-1));scene.addView(outer,new FrameLayout.LayoutParams(-1,-1));
        FrameLayout banner=new FrameLayout(this);banner.setBackground(framed(glass,12));

        LinearLayout header=row();header.addView(icon("actions","Navigation",()->dispatch("navigation")),new LinearLayout.LayoutParams(dp(48),dp(48)));
        LinearLayout heading=column();heading.setGravity(Gravity.CENTER_VERTICAL);heading.setPadding(dp(4),0,dp(4),0);if(page.equals("chat")){heading.setContentDescription("Change chat model");heading.setClickable(true);heading.setFocusable(true);heading.setOnClickListener(v->dispatch("chat_models"));}TextView title=summary(page.equals("chat")?(ui.optString("connection_label").isEmpty()?"RLLM":ui.optString("connection_label")):page.substring(0,1).toUpperCase(Locale.ROOT)+page.substring(1),16,1,ui.optString("title"));title.setTypeface(proseFont,Typeface.BOLD);heading.addView(title);if(page.equals("chat")&&lineHeight(16)<32&&!ui.optString("server").isEmpty()){TextView server=summary(ui.optString("server"),10,1,ui.optString("server"));server.setTextColor(muted);heading.addView(server);}header.addView(heading,new LinearLayout.LayoutParams(0,-1,1));
        header.addView(icon("history","Chat history",()->dispatch("history")),new LinearLayout.LayoutParams(dp(48),dp(48)));
        ImageButton create=icon("message","Edit connection",()->dispatch("profile_edit"));create.setEnabled(!busy);header.addView(create,new LinearLayout.LayoutParams(dp(48),dp(48)));banner.addView(header,new FrameLayout.LayoutParams(-1,-1));LinearLayout.LayoutParams bannerBounds=new LinearLayout.LayoutParams(-1,dp(geometry.getInt("header_height")));bannerBounds.topMargin=dp(6);bannerBounds.bottomMargin=dp(8);outer.addView(banner,bannerBounds);
        if(page.equals("chat"))chatBanner=banner;
        LinearLayout contentSurface=outer;
        if(page.equals("chat")){contentSurface=column();contentSurface.setBackground(framed(glass,18));outer.addView(contentSurface,new LinearLayout.LayoutParams(-1,0,1));int headingHeight=geometry.optInt("conversation_header_height");if(headingHeight>0){TextView conversation=summary("CONVERSATION",12,1,"Conversation");conversationHeading=conversation;conversation.setTypeface(Typeface.create("sans-serif-medium",Typeface.NORMAL));conversation.setLetterSpacing(.12f);conversation.setTextColor(ink);conversation.setGravity(Gravity.CENTER_VERTICAL);conversation.setPadding(dp(16),dp(4),dp(16),0);contentSurface.addView(conversation,new LinearLayout.LayoutParams(-1,dp(headingHeight)));}}
        LinearLayout main=column();main.setTag("workflow");contentSurface.addView(main,new LinearLayout.LayoutParams(-1,0,1));
        if(page.equals("chat")){
            JSONArray messages=ui.optJSONArray("messages");final JSONArray entries=messages==null?new JSONArray():messages;displayedMessages=entries;displayedConversation=ui.optString("conversation_id");
            if(entries.length()==0){ScrollView welcome=new ScrollView(this);welcome.setFillViewport(true);LinearLayout intro=column();intro.setGravity(Gravity.CENTER_VERTICAL);intro.setPadding(dp(16),dp(20),dp(16),dp(20));TextView brand=label("Start a conversation",22);brand.setGravity(Gravity.CENTER);brand.setTypeface(proseFont,Typeface.BOLD);intro.addView(brand);TextView help=label(ui.optBoolean("connected_profile")?ui.optString("connection_label"):"Connect any LLM with its URL, model name and optional API key. Your conversations and tools stay here.",15);help.setTextColor(muted);help.setGravity(Gravity.CENTER);intro.addView(help);if(!ui.optBoolean("connected_profile"))intro.addView(button("Connect to LLM",()->dispatch("profile_new")));if(!ui.optBoolean("connected_profile"))intro.addView(button("Workspace",()->dispatch("workspace")));welcome.addView(intro);main.addView(welcome,new LinearLayout.LayoutParams(-1,-1));}
            else{ListView list=new ListView(this);conversationList=list;list.setDivider(null);list.setClipToPadding(false);list.setPadding(0,dp(12),0,dp(12));list.setAdapter(new BaseAdapter(){public int getCount(){return entries.length();}public Object getItem(int position){return entries.optJSONObject(position);}public long getItemId(int position){return position;}public View getView(int position,View recycled,android.view.ViewGroup parent){JSONObject message=entries.optJSONObject(position);LinearLayout box=column();box.setPadding(dp(12),dp(5),dp(12),dp(5));box.setMinimumHeight(dp(48));boolean user=message.optString("role").equals("user");box.setBackground(user?framed(card,12):panel(Color.TRANSPARENT,0));TextView role=label(message.optString("speaker",user?"You":"Assistant"),12);role.setTextColor(user?muted:accent);role.setTypeface(proseFont,Typeface.BOLD);role.setPadding(dp(8),0,dp(8),0);box.addView(role);TextView body=label(message.optString("content"),16);body.setLineSpacing(dp(3),1f);body.setPadding(dp(8),dp(2),dp(8),dp(2));if(!message.optBoolean("streaming"))box.setOnClickListener(v->dispatch("branch:"+position));box.setOnLongClickListener(v->{((android.content.ClipboardManager)getSystemService(CLIPBOARD_SERVICE)).setPrimaryClip(android.content.ClipData.newPlainText("Message",message.optString("content")));Toast.makeText(Activity.this,"Message copied",Toast.LENGTH_SHORT).show();return true;});box.addView(body);return box;}});main.addView(list,new LinearLayout.LayoutParams(-1,-1));int[] prior=conversationScroll.get(displayedConversation);if(prior==null||prior[2]==1||(entries.length()>prior[3]&&entries.optJSONObject(entries.length()-1).optString("role").equals("user")))list.setSelection(entries.length()-1);else list.setSelectionFromTop(Math.min(prior[0],entries.length()-1),prior[1]);}
        }else if(page.equals("history")){
            ScrollView scroll=new ScrollView(this);LinearLayout rows=column();JSONArray entries=ui.optJSONArray("rows");if(entries!=null)for(int i=0;i<entries.length();i++){JSONObject entry=entries.getJSONObject(i);String event=entry.getString("event"),name=entry.getString("label");LinearLayout line=row();Button open=button(name,()->dispatch(event));open.setTextColor(ink);open.setGravity(Gravity.START|Gravity.CENTER_VERTICAL);open.setPadding(dp(12),dp(8),dp(12),dp(8));open.setEnabled(!busy);line.addView(open,new LinearLayout.LayoutParams(0,-2,1));if(event.startsWith("history:")){ImageButton options=icon("actions","Conversation options: "+name,()->dispatch("chat_options:"+event.substring(8)));options.setEnabled(!busy);line.addView(options,new LinearLayout.LayoutParams(dp(48),dp(48)));}rows.addView(line,new LinearLayout.LayoutParams(-1,-2));}scroll.addView(rows);main.addView(scroll,new LinearLayout.LayoutParams(-1,-1));
        }else if(page.equals("diagnostics")){
            monitor(main,ui.getJSONObject("monitor"),palette);
        }else{
            ArrayList<String> labels=new ArrayList<>(),events=new ArrayList<>();JSONArray rows=ui.optJSONArray("rows");if(rows!=null)for(int i=0;i<rows.length();i++){JSONObject item=rows.getJSONObject(i);labels.add(item.getString("label"));events.add(item.getString("event"));}
            showPage(main,labels,events,page,pageOffsets.containsKey(page)?pageOffsets.get(page):0,geometry.getInt("rows"),Math.max(1,geometry.getInt("preview_lines")),busy);
        }
        String visibleStatus=ui.optString("visible_status");TextView status=summary(visibleStatus,11,2,visibleStatus);statusView=status;status.setTextColor(muted);status.setOnClickListener(v->dispatch("status"));status.setGravity(Gravity.CENTER_VERTICAL);status.setMinHeight(dp(48));status.setVisibility(visibleStatus.isEmpty()?View.GONE:View.VISIBLE);contentSurface.addView(status,new LinearLayout.LayoutParams(-1,-2));
        if(page.equals("chat")){
            modelFooter=row();modelFooter.setPadding(dp(12),0,dp(12),0);
            Button model=button(ui.optString("connection_label").isEmpty()?"Choose LLM ▾":ui.optString("connection_label")+" ▾",()->dispatch("chat_models"));
            Button agent=button(ui.optString("agent_name","General")+" ▾",()->dispatch("chat_agents"));
            rateBadge=button("— tok/s",()->dispatch("monitor:all"));
            for(Button control:new Button[]{model,agent,rateBadge}){control.setMinWidth(0);control.setMinimumWidth(0);control.setPadding(dp(8),0,dp(8),0);control.setBackgroundColor(Color.TRANSPARENT);modelFooter.addView(control,new LinearLayout.LayoutParams(0,-1,1));}
            model.setGravity(Gravity.START|Gravity.CENTER_VERTICAL);agent.setGravity(Gravity.CENTER);
            for(Button control:new Button[]{model,agent}){control.setSingleLine(true);control.setEllipsize(android.text.TextUtils.TruncateAt.END);}
            rateBadge.setGravity(Gravity.END|Gravity.CENTER_VERTICAL);rateBadge.setMaxLines(2);rateBadge.setTextColor(muted);
            model.setContentDescription("Change connection: "+ui.optString("connection_label"));agent.setContentDescription("Change main agent: "+ui.optString("agent_name"));model.setTooltipText(model.getContentDescription());agent.setTooltipText(agent.getContentDescription());agent.setEnabled(!busy);
            renderRate(ui);contentSurface.addView(modelFooter,new LinearLayout.LayoutParams(-1,dp(Math.max(48,2*lineHeight(12)+12))));
        }
        if(page.equals("chat")){
            LinearLayout composer=row();activeComposer=composer;composer.setGravity(Gravity.BOTTOM);composer.setPadding(dp(4),dp(4),dp(4),dp(4));composer.setBackground(framed(card,12));
            ImageButton menu=icon("addition","Conversation actions",()->dispatch("chat_actions"));composerMenu=menu;composerModel=ui.optString("connection_label");composerBusy=busy;menu.setEnabled(!busy);composer.addView(menu,new LinearLayout.LayoutParams(dp(48),dp(48)));
            draft=new EditText(this);draft.setTextColor(ink);draft.setHintTextColor(muted);draft.setTextSize(16*textScale);draft.setTypeface(proseFont);draft.setHint("Ask anything");draft.setContentDescription("Message or project instruction");draft.setInputType(InputType.TYPE_CLASS_TEXT|InputType.TYPE_TEXT_FLAG_MULTI_LINE|InputType.TYPE_TEXT_FLAG_CAP_SENTENCES);draft.setImeOptions(android.view.inputmethod.EditorInfo.IME_FLAG_NO_FULLSCREEN);draft.setMinLines(1);draft.setMaxLines(viewportHeight>0&&viewportHeight<350?1:3);draft.setMinHeight(dp(48));draft.setBackgroundColor(Color.TRANSPARENT);draft.setPadding(dp(8),dp(10),dp(8),dp(10));draft.setText(ui.optString("draft"));composer.addView(draft,new LinearLayout.LayoutParams(0,-2,1));ImageButton submit=icon(busy?"cancellation":"arrow_up",busy?"Cancel":"Send",()->dispatch(busy?"cancel":"send"));submit.setBackground(framed(accent,10));submit.setImageTintList(android.content.res.ColorStateList.valueOf(surface));composer.addView(submit,new LinearLayout.LayoutParams(dp(48),dp(48)));LinearLayout.LayoutParams composerBounds=new LinearLayout.LayoutParams(-1,-2);composerBounds.setMargins(dp(8),0,dp(8),dp(8));contentSurface.addView(composer,composerBounds);
        }else if(busy)outer.addView(button("Cancel",()->dispatch("cancel")),new LinearLayout.LayoutParams(-1,dp(48)));
        if(busy&&state.optBoolean("rea_resumable")&&state.optString("rea_expected").startsWith("rea_server:"))outer.addView(button("Pause server stream",()->dispatch("rea_pause")),new LinearLayout.LayoutParams(-1,dp(48)));
        if(page.equals("chat"))applyChatGeometry(geometry);
        outer.setOnApplyWindowInsetsListener((view,insets)->{if(android.os.Build.VERSION.SDK_INT>=30){Insets bars=insets.getInsets(WindowInsets.Type.systemBars()),ime=insets.getInsets(WindowInsets.Type.ime());insetLeft=bars.left;insetTop=bars.top;insetRight=bars.right;insetBottom=Math.max(bars.bottom,ime.bottom);}else{insetLeft=insets.getSystemWindowInsetLeft();insetTop=insets.getSystemWindowInsetTop();insetRight=insets.getSystemWindowInsetRight();insetBottom=insets.getSystemWindowInsetBottom();}view.setPadding(dp(8)+insetLeft,insetTop,dp(8)+insetRight,dp(8)+insetBottom);queueViewport();return insets;});
        setContentView(scene);if(android.os.Build.VERSION.SDK_INT>=30){WindowInsetsController controller=getWindow().getInsetsController();if(controller!=null)controller.setSystemBarsAppearance(dark?0:WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS|WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS,WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS|WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS);}outer.requestApplyInsets();JSONObject modal=ui.optJSONObject("modal");if(modal!=null)modal(modal);else if(composerFocus&&draft!=null){draft.requestFocus();draft.setSelection(Math.min(selection,draft.length()));}rendering=false;
    }
    private void applyChatGeometry(JSONObject geometry)throws Exception {
        if(chatBanner==null||activeComposer==null||draft==null)return;
        boolean compact=geometry.optBoolean("short_chat");int gap=geometry.getInt("chrome_gap"),height=geometry.getInt("header_height");
        LinearLayout.LayoutParams header=(LinearLayout.LayoutParams)chatBanner.getLayoutParams();header.height=dp(height);header.topMargin=compact?0:dp(6);header.bottomMargin=dp(gap);chatBanner.setLayoutParams(header);chatBanner.setVisibility(height==0?View.GONE:View.VISIBLE);
        if(conversationHeading!=null){int heading=geometry.getInt("conversation_header_height");conversationHeading.getLayoutParams().height=dp(heading);conversationHeading.setVisibility(heading==0?View.GONE:View.VISIBLE);}
        int status=geometry.getInt("chat_status_height");if(modelFooter!=null){modelFooter.getLayoutParams().height=dp(status==0?0:Math.max(status,2*lineHeight(12)+12));modelFooter.setVisibility(status==0?View.GONE:View.VISIBLE);}if(composerMenu!=null){boolean picker=status==0;composerMenu.setImageResource(picker?R.drawable.transfer:R.drawable.addition);composerMenu.setContentDescription(picker?"Change chat model: "+composerModel:"Conversation actions");composerMenu.setTooltipText(composerMenu.getContentDescription());composerMenu.setEnabled(picker||!composerBusy);composerMenu.setOnClickListener(v->dispatch(picker?"chat_models":"chat_actions"));}
        LinearLayout.LayoutParams bounds=(LinearLayout.LayoutParams)activeComposer.getLayoutParams();bounds.setMargins(dp(gap),0,dp(gap),dp(gap));activeComposer.setLayoutParams(bounds);
        draft.setPadding(dp(8),compact?0:dp(10),dp(8),compact?0:dp(10));draft.setMaxLines(compact?1:3);
    }
    private void monitor(LinearLayout parent,JSONObject model,JSONObject palette)throws Exception{
        ScrollView scroll=new ScrollView(this);scroll.setFillViewport(true);
        LinearLayout body=column();body.setPadding(dp(2),dp(4),dp(2),dp(12));scroll.addView(body);parent.addView(scroll,new LinearLayout.LayoutParams(-1,-1));
        TextView subtitle=label(model.optString("subtitle"),13);subtitle.setTextColor(muted);subtitle.setPadding(dp(8),dp(8),dp(8),dp(8));body.addView(subtitle);
        LinearLayout ranges=row();int selected=model.optInt("window",32);
        for(int value:new int[]{16,32,64,128}){Button b=button(Integer.toString(value),()->dispatch("monitor:window:"+value));b.setContentDescription("Last "+value+" requests");b.setSelected(value==selected);b.setTextColor(value==selected?surface:ink);b.setBackground(framed(value==selected?accent:glass,8));LinearLayout.LayoutParams lp=new LinearLayout.LayoutParams(0,dp(48),1);lp.setMargins(dp(2),0,dp(2),dp(8));ranges.addView(b,lp);}body.addView(ranges);
        JSONArray gauges=model.getJSONArray("gauges");
        boolean stack=getResources().getConfiguration().fontScale*textScale>1.3f||viewportWidth<320;
        for(int i=0;i<gauges.length();){
            LinearLayout pair=row();
            int columns=stack?1:2;
            for(int c=0;c<columns&&i<gauges.length();c++,i++){LinearLayout panel=monitorCard(gauges.getJSONObject(i),palette);LinearLayout.LayoutParams lp=new LinearLayout.LayoutParams(0,-2,1);lp.setMargins(dp(2),0,dp(2),dp(8));pair.addView(panel,lp);}
            body.addView(pair);
        }
        JSONArray cards=model.getJSONArray("cards");
        for(int i=0;i<cards.length();i++){LinearLayout panel=monitorCard(cards.getJSONObject(i),palette);LinearLayout.LayoutParams lp=new LinearLayout.LayoutParams(-1,-2);lp.setMargins(dp(2),0,dp(2),dp(8));body.addView(panel,lp);}
        JSONArray actions=model.getJSONArray("actions");for(int i=0;i<actions.length();i++){JSONObject a=actions.getJSONObject(i);body.addView(button(a.getString("label"),()->dispatch(a.optString("event"))));}
    }
    private LinearLayout monitorCard(JSONObject model,JSONObject palette)throws Exception{
        LinearLayout panel=column();panel.setBackground(framed(glass,12));panel.setPadding(dp(12),dp(12),dp(12),dp(12));
        TextView title=label(model.optString("title"),12);title.setTypeface(proseFont,Typeface.BOLD);title.setLetterSpacing(.06f);panel.addView(title);
        if(model.has("value")){TextView value=label(model.optString("value"),23);value.setTypeface(proseFont,Typeface.BOLD);value.setTextColor(accent);value.setPadding(0,dp(6),0,0);panel.addView(value);if(model.optString("id").equals("local_rate"))rateValue=value;}
        JSONObject drawing=model.optJSONObject("visual");
        if(drawing!=null){Diagram diagram=new Diagram(this,drawing,palette,event->dispatch(event));panel.addView(diagram,new LinearLayout.LayoutParams(-1,-2));if(model.optString("id").equals("local_rate"))rateDiagram=diagram;}
        JSONArray legend=model.optJSONArray("legend");if(legend!=null)for(int i=0;i<legend.length();i++){JSONObject item=legend.getJSONObject(i);TextView text=label("● "+item.optString("label"),12);text.setTextColor(Color.parseColor(palette.optString(item.optString("color"),"#A2BCC5")));panel.addView(text);}
        TextView note=label(model.optString("caption"),12);note.setTextColor(muted);note.setPadding(0,dp(6),0,0);panel.addView(note);if(model.optString("id").equals("local_rate"))rateCaption=note;
        return panel;
    }
    private void modal(JSONObject model) throws Exception {
        LinearLayout content=column();content.setBackgroundColor(surface);content.setPadding(dp(16),dp(12),dp(16),dp(12));JSONArray definitions=model.optJSONArray("fields");
        if(definitions!=null)for(int i=0;i<definitions.length();i++){JSONObject f=definitions.getJSONObject(i);String name=f.getString("key"),kind=f.optString("kind"),value=f.optString("value");
            if(kind.equals("choice")){content.addView(label(f.optString("label"),12));android.widget.Spinner select=new android.widget.Spinner(this);JSONArray choices=f.getJSONArray("options");ArrayList<String> labels=new ArrayList<>();int selected=0;for(int j=0;j<choices.length();j++){JSONObject option=choices.getJSONObject(j);labels.add(option.getString("label"));if(option.getString("value").equals(value))selected=j;}android.widget.ArrayAdapter<String> adapter=new android.widget.ArrayAdapter<String>(this,android.R.layout.simple_spinner_item,labels){public View getView(int p,View v,android.view.ViewGroup g){TextView t=(TextView)super.getView(p,v,g);t.setTextColor(ink);t.setTextSize(14*textScale);t.setTypeface(proseFont);return t;}public View getDropDownView(int p,View v,android.view.ViewGroup g){TextView t=(TextView)super.getDropDownView(p,v,g);t.setTextColor(ink);t.setTypeface(proseFont);t.setTextSize(14*textScale);t.setBackgroundColor(surface);t.setMinHeight(dp(48));return t;}};adapter.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item);select.setAdapter(adapter);select.setSelection(selected);select.setTag(choices);select.setContentDescription(f.optString("label"));select.setMinimumHeight(dp(48));content.addView(select,new LinearLayout.LayoutParams(-1,-2));fields.put(name,select);}
            else if(kind.equals("check")){CheckBox c=new CheckBox(this);c.setText(f.optString("label"));c.setTextColor(ink);c.setTypeface(proseFont);c.setTextSize(14*textScale);c.setChecked(value.equals("true"));c.setMinHeight(dp(48));c.setPadding(dp(8),dp(8),dp(8),dp(8));content.addView(c,new LinearLayout.LayoutParams(-1,-2));fields.put(name,c);}
            else{content.addView(label(f.optString("label"),12));if(kind.equals("readonly")){TextView t=label(value,13);t.setTextIsSelectable(true);t.setTypeface(proseFont);content.addView(t);}
                else{EditText e=new EditText(this);e.setTextSize(15*textScale);e.setTypeface(proseFont);e.setTextColor(ink);e.setText(value);if(name.equals("endpoint"))e.setHint("Local IP → http://; explicit https:// is kept");e.setHintTextColor(muted);e.setBackgroundTintList(new android.content.res.ColorStateList(new int[][]{new int[]{android.R.attr.state_focused},new int[]{}},new int[]{accent,muted}));e.setSingleLine(!kind.equals("multiline"));if(kind.equals("secret")){e.setInputType(InputType.TYPE_CLASS_TEXT|InputType.TYPE_TEXT_VARIATION_PASSWORD);e.setImportantForAutofill(View.IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS);}else if(kind.equals("uri"))e.setInputType(InputType.TYPE_CLASS_TEXT|InputType.TYPE_TEXT_VARIATION_URI);else if(kind.equals("multiline")){e.setInputType(InputType.TYPE_CLASS_TEXT|InputType.TYPE_TEXT_FLAG_MULTI_LINE);e.setMinLines(5);e.setGravity(Gravity.TOP);}content.addView(e,new LinearLayout.LayoutParams(-1,-2));fields.put(name,e);}}
        }
        boolean profileForm=state!=null&&state.optString("dialog").equals("profile");JSONArray actions=model.optJSONArray("buttons");if(actions!=null)for(int i=0;i<actions.length();i++){JSONObject b=actions.getJSONObject(i);String event=b.getString("event");if(profileForm&&event.equals("profile_save"))continue;Button control=button(b.getString("label"),()->{if(event.equals("conversation_export"))export();else dispatch(event);});control.setEnabled(b.optBoolean("enabled",true));control.setAlpha(control.isEnabled()?1f:.45f);if(model.optString("presentation").equals("list")){control.setGravity(Gravity.START|Gravity.CENTER_VERTICAL);control.setTextColor(ink);control.setPadding(dp(12),dp(8),dp(12),dp(8));control.setMinHeight(dp(48));control.setBackground(new android.graphics.drawable.RippleDrawable(android.content.res.ColorStateList.valueOf((accent&0x00ffffff)|0x33000000),new android.graphics.drawable.ColorDrawable(Color.TRANSPARENT),null));content.addView(control,new LinearLayout.LayoutParams(-1,-2));View line=new View(this);line.setBackgroundColor(border);content.addView(line,new LinearLayout.LayoutParams(-1,dp(1)));}else{control.setPadding(dp(12),dp(8),dp(12),dp(8));LinearLayout.LayoutParams actionBounds=new LinearLayout.LayoutParams(-1,-2);actionBounds.setMargins(0,dp(4),0,dp(4));content.addView(control,actionBounds);}}
        ScrollView scroll=new ScrollView(this);scroll.addView(content);
        TextView title=label(model.optString("title"),20);title.setTypeface(proseFont,Typeface.BOLD);title.setBackgroundColor(surface);title.setPadding(dp(24),dp(20),dp(24),dp(12));
        AlertDialog.Builder builder=new AlertDialog.Builder(this).setCustomTitle(title).setView(scroll).setNegativeButton("Close",(d,w)->dispatch("close"));
        if(profileForm){builder.setPositiveButton("Save",(d,w)->dispatch("profile_save"));}
        dialog=builder.create();dialog.setOnCancelListener(d->{if(!rendering)dispatch("close");});dialog.show();
        for(int id:new int[]{AlertDialog.BUTTON_NEGATIVE,AlertDialog.BUTTON_POSITIVE,AlertDialog.BUTTON_NEUTRAL}){Button control=dialog.getButton(id);if(control!=null){control.setTextColor(accent);control.setMinHeight(dp(48));if(control.getParent() instanceof View)((View)control.getParent()).setBackgroundColor(surface);}}
        if(dialog.getWindow()!=null){dialog.getWindow().setBackgroundDrawable(panel(surface,20));dialog.getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);}

    }
    private static byte[] read(InputStream in,int max) throws IOException {ByteArrayOutputStream out=new ByteArrayOutputStream();byte[] buffer=new byte[8192];int n;while((n=in.read(buffer))!=-1){if(out.size()+n>max)throw new IOException("Input exceeds size limit");out.write(buffer,0,n);}return out.toByteArray();}
    private void loadWallpaper(String value,ImageView target){
        workers.execute(()->{try{Uri uri=Uri.parse(value);android.graphics.BitmapFactory.Options options=new android.graphics.BitmapFactory.Options();options.inJustDecodeBounds=true;try(InputStream input=getContentResolver().openInputStream(uri)){android.graphics.BitmapFactory.decodeStream(input,null,options);}if(options.outWidth<=0||options.outHeight<=0||options.outWidth>32768||options.outHeight>32768)throw new IOException("Unsupported background image");options.inSampleSize=1;while(Math.max(options.outWidth,options.outHeight)/options.inSampleSize>2048)options.inSampleSize*=2;options.inJustDecodeBounds=false;android.graphics.Bitmap bitmap;try(InputStream input=getContentResolver().openInputStream(uri)){bitmap=android.graphics.BitmapFactory.decodeStream(input,null,options);}if(bitmap==null)throw new IOException("Cannot read background image");runOnUiThread(()->{if(closed)return;customWallpaper=bitmap;customWallpaperUri=value;target.setImageBitmap(bitmap);});}catch(Exception problem){runOnUiThread(()->{if(!closed)Toast.makeText(this,"Background unavailable: "+safe(problem),Toast.LENGTH_LONG).show();});}});
    }
    private void effect(JSONObject e) throws Exception {
        String kind=e.getString("kind");
        JSONArray cleanup=e.optJSONArray("mcp_cleanup");e.remove("mcp_cleanup");e.remove("mcp_abort_cleanup");if(cleanup!=null)queueMcpCleanup(cleanup,android.os.SystemClock.elapsedRealtime()+2000);
        if(kind.equals("mcp_cleanup"))return;
        if(kind.equals("project_clear")){documentJobs.execute(()->{try{transferStore.clear(key(),e.getString("ticket"));runOnUiThread(()->{if(!closed)try{dispatch("project_cleared",new JSONObject().put("ticket",e.getString("ticket")).put("purpose",e.getString("purpose")));}catch(Exception problem){Toast.makeText(this,safe(problem),Toast.LENGTH_LONG).show();}});}catch(Exception problem){runOnUiThread(()->{if(!closed)try{dispatch("failure",new JSONObject().put("purpose",e.optString("purpose")).put("error",safe(problem)));}catch(Exception ignored){}});}});return;}
        if(kind.equals("document_export")){stageExport(e,e.getString("content").getBytes(StandardCharsets.UTF_8),null);return;}
        if(kind.equals("export_picker")){openSavedExport(e.getString("ticket"));return;}
        if(kind.equals("export_cancel")){cancelledExports.add(e.getString("ticket"));cancelExportDocuments();return;}
        if(kind.equals("export_discard")){String ticket=e.getString("ticket");documentJobs.execute(()->{try{exportStore.clear(key(),ticket);cancelledExports.remove(ticket);exportEvent("export_discarded",ticket,null);}catch(Exception problem){exportEvent("export_failed",ticket,safe(problem));}});return;}
        if(kind.equals("gateway_start")){if(android.os.Build.VERSION.SDK_INT>=33&&checkSelfPermission("android.permission.POST_NOTIFICATIONS")!=android.content.pm.PackageManager.PERMISSION_GRANTED)requestPermissions(new String[]{"android.permission.POST_NOTIFICATIONS"},45);startForegroundService(new Intent(this,Gateway.class).putExtra("config",e.getJSONObject("config").toString()));return;}
        if(kind.equals("gateway_stop")){stopService(new Intent(this,Gateway.class));return;}
        if(kind.equals("mcp_pause")){mcpPauseOwner=e.getString("owner");for(Socket socket:activeConnections)try{socket.close();}catch(IOException ignored){}return;}
        if(kind.equals("cancel")){mcpPauseOwner="";JSONObject pending=state.optJSONObject("export_pending");if(pending!=null)cancelledExports.add(pending.optString("ticket"));stopExecution();generation++;cancelDocuments();for(Socket socket:activeConnections)try{socket.close();}catch(IOException ignored){}Core.call(new JSONObject().put("op","cancel_tool"));return;}
        if(kind.equals("wallpaper_picker")){Intent intent=new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("image/*").addCategory(Intent.CATEGORY_OPENABLE).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);startActivityForResult(intent,47);return;}
        if(kind.equals("app_usage_settings")){
            try{startActivity(new Intent(android.provider.Settings.ACTION_USAGE_ACCESS_SETTINGS));}
            catch(android.content.ActivityNotFoundException|SecurityException unavailable){new AlertDialog.Builder(this).setTitle("Usage settings unavailable").setMessage("This Android device has no handler for Usage access settings. Open system Settings manually; no usage observation was started.").setPositiveButton("Close",null).show();}return;
        }
        if(kind.equals("app_observation")){
            final int token=generation;final JSONObject command=new JSONObject(e.toString());
            final JSONObject result=appBinding(command);final boolean usage=command.optString("operation").equals("usage");
            try{appJobs.execute(()->{
                try{appObservationOwner(command,token);if(usage){usageActive(command,token);result.put("data",Usage.read(this,command,()->appObservationOwner(command,token),()->usageActive(command,token)));}else result.put("data",appObservation(command,token));}
                catch(Exception problem){try{result.remove("data");result.put("error",safe(problem));}catch(JSONException ignored){}}
                runOnUiThread(()->{
                    try{appObservationOwner(command,token);}catch(IOException stale){return;}
                    if(usage)try{usageActive(command,token);}catch(Exception unavailable){try{result.remove("data");result.put("error",safe(unavailable)).put("grant_error",true);}catch(JSONException ignored){}}
                    dispatch("apps_result",result);
                });
            });}catch(java.util.concurrent.RejectedExecutionException unavailable){dispatch("apps_result",result.put("error","App observation worker is unavailable"));}return;
        }
        if(kind.equals("phone_access_request")){requestPhoneAccess();return;}
        if(kind.equals("document_tree")){Intent intent=new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_WRITE_URI_PERMISSION|Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);startActivityForResult(intent,43);return;}
        if(kind.equals("document_zip")){pendingImport=null;Intent intent=new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("application/zip").addCategory(Intent.CATEGORY_OPENABLE);startActivityForResult(intent,45);return;}
        if(kind.equals("conversation_document")){Intent intent=new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("application/json").addCategory(Intent.CATEGORY_OPENABLE);startActivityForResult(intent,46);return;}
        if(kind.equals("archive_clear")){pendingImport=null;return;}
        if(kind.equals("document")){Intent intent=new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").addCategory(Intent.CATEGORY_OPENABLE);startActivityForResult(intent,41);return;}
        if(kind.equals("network")&&e.optString("purpose").equals("project_file"))pendingImport=null;
        final int token=++generation;final long started=android.os.SystemClock.elapsedRealtime();
        if(kind.equals("network")&&e.optString("purpose").equals("model"))startRateClock(token,e.optString("model_ticket"),started);
        workers.execute(()->{final boolean[] sending={false};final boolean[] streaming={false};final long[] metrics={0,-1,-1,-1};long snapshotHandle=0;boolean chunkedSnapshot=false;try{JSONObject work=e;String operation=kind;
            if(kind.equals("project_payload")){JSONObject stored=documentJobs.submit(()->transferStore.load(key())).get();if(stored==null||stored.has("cleared_ticket"))throw new IOException("Pending snapshot is missing or cleared");
                chunkedSnapshot=stored.optInt("version")==2;
                if(chunkedSnapshot)work=(JSONObject)Core.call(new JSONObject().put("op","project_chunks").put("action","open").put("record",e.getJSONObject("record")).put("stored",stored.getJSONObject("record")));
                else work=(JSONObject)Core.call(new JSONObject().put("op","project_payload").put("record",e.getJSONObject("record")).put("stored",stored.getJSONObject("record")).put("archive",stored.getString("payload")));
                snapshotHandle=work.getLong("snapshot_handle");operation=work.getString("kind");}
            if(token!=generation||closed)throw new IOException("Cancelled before transfer");final JSONObject command=work;JSONObject result=new JSONObject();String callback;
            if(operation.equals("network")&&chunkedSnapshot){result=transferNetwork(command,token);snapshotHandle=0;callback="network_result";
            }else if(operation.equals("network")){String host=command.getString("host");int port=command.getInt("port");Socket raw=new Socket();activeConnections.add(raw);ScheduledFuture<?> timeout=deadlines.schedule(()->{try{raw.close();}catch(IOException ignored){}},120,TimeUnit.SECONDS);
                try{raw.connect(new InetSocketAddress(host,port),15000);raw.setSoTimeout(120000);Socket connection=raw;
                    if(command.optBoolean("tls")){SSLSocket tls=(SSLSocket)((SSLSocketFactory)SSLSocketFactory.getDefault()).createSocket(raw,host,port,true);SSLParameters params=tls.getSSLParameters();params.setEndpointIdentificationAlgorithm("HTTPS");tls.setSSLParameters(params);tls.setSoTimeout(120000);tls.startHandshake();connection=tls;}
                    try(Socket managed=connection){if(token!=generation)throw new IOException("Cancelled");sending[0]=true;byte[] requestBytes=command.has("wire_base64")?Base64.decode(command.getString("wire_base64"),Base64.NO_WRAP):command.getString("wire").getBytes(StandardCharsets.UTF_8);managed.getOutputStream().write(requestBytes);managed.getOutputStream().flush();metrics[3]=requestBytes.length;InputStream measured=measuredInput(managed.getInputStream(),started,metrics);streaming[0]=command.optBoolean("stream");byte[] bytes=command.optBoolean("stream")?readStream(measured,token,command.optInt("limit",1024*1024),command.optString("stream_protocol","local"),started,metrics,command.optString("model_ticket"),command.optJSONObject("mcp_control")):read(measured,Math.min(6*1024*1024,command.optInt("limit",1024*1024)));if(command.optString("purpose").equals("project_file")){JSONObject extracted=(JSONObject)Core.call(new JSONObject().put("op","network_binary").put("data",Base64.encodeToString(bytes,Base64.NO_WRAP)));byte[] archive=Base64.decode(extracted.getString("archive"),Base64.NO_WRAP);JSONObject preview=(JSONObject)Core.call(new JSONObject().put("op","archive_preview").put("archive",extracted.getString("archive")));runOnUiThread(()->{if(token!=generation||closed)return;pendingImport=archive;try{dispatch("archive_review",new JSONObject().put("names",preview.getJSONArray("names")).put("bytes",preview.getInt("bytes")));}catch(JSONException error){pendingImport=null;fatal(safe(error));}});return;}result.put("data",Base64.encodeToString(bytes,Base64.NO_WRAP));result.put("purpose",command.getString("purpose"));result.put("model_ticket",command.optString("model_ticket"));result.put("web_ticket",command.optString("web_ticket"));addMeasurements(result,started,metrics);}
                }finally{timeout.cancel(false);try{raw.close();}catch(IOException ignored){}activeConnections.remove(raw);if(!streaming[0]&&command.optJSONObject("mcp_control")!=null&&(token!=generation||command.getJSONObject("mcp_control").optBoolean("resumable")))try{JSONObject context=command.getJSONObject("mcp_control");JSONArray openingCleanup=(JSONArray)Core.call(new JSONObject().put("op",context.has("resume_token")||context.has("admission_token")?"stream_open_cleanup":"mcp_cleanup_plan").put("context",context));if(!closed)queueMcpCleanup(openingCleanup,Math.min(started+120000,android.os.SystemClock.elapsedRealtime()+2000));}catch(Exception ignored){}}callback="network_result";
            }else if(operation.equals("archive")){JSONObject request=new JSONObject().put("op","project_archive").put("root",root());JSONObject packageData=(JSONObject)Core.call(request);byte[] archive=Base64.decode(packageData.getString("archive"),Base64.NO_WRAP);
                if(token!=generation||closed)return;stageExport(command,archive,packageData);return;
            }else if(operation.equals("artifact_file")){stageArtifact(command,token);return;
            }else if(operation.equals("archive_drive")){JSONObject packageData=(JSONObject)Core.call(new JSONObject().put("op","project_archive").put("root",root()));stageTransfer(command,packageData.getString("archive"),false,token);return;
            }else if(operation.equals("archive_import")){if(token!=generation)return;byte[] bytes=pendingImport;if(bytes==null)throw new IOException("Selected project archive is no longer available; select it again");JSONObject report=(JSONObject)Core.call(new JSONObject().put("op","archive_import").put("root",root()).put("archive",Base64.encodeToString(bytes,Base64.NO_WRAP)));pendingImport=null;result.put("files",report.getInt("files")).put("directories",report.optInt("directories"));callback="archive_done";
            }else if(operation.equals("project_phone")){result=projectPhone(command,token);if(chunkedSnapshot)snapshotHandle=0;result.put("purpose",command.getString("purpose"));callback="project_phone_result";
            }else if(operation.equals("phone_revision")){result=phoneRevision(command,token);result.put("phase",command.getString("phase")).put("ticket",command.getString("ticket"));callback="phone_revision_result";
            }else if(operation.equals("tool")){JSONObject request=new JSONObject(command.toString()).put("op","tool").put("root",root());Object output=command.optString("tool").startsWith("phone_")?phoneReadOrDocument(command,token):Core.call(request);if(output instanceof JSONObject && ((JSONObject)output).has("source")){JSONObject program=(JSONObject)output;runOnUiThread(()->{if(token==generation&&!closed)startExecution(program,token);});return;}result.put("output",output.toString());result.put("editor",command.optBoolean("editor"));result.put("editor_dialog",command.optString("editor_dialog"));callback="tool_result";}
            else throw new IOException("Unknown platform operation");
            if(snapshotHandle!=0){JSONObject checked=new JSONObject().put("op","project_readback").put("handle",snapshotHandle);if(result.has("found"))checked.put("found",result.get("found"));if(result.has("data"))checked.put("data",result.get("data"));result=(JSONObject)Core.call(checked);snapshotHandle=0;}
            final JSONObject delivered=result;runOnUiThread(()->{if(token==generation&&!closed)dispatch(callback,delivered);else if(callback.equals("network_result"))mcpDiscardedSuccess(command,token);});
        }catch(Throwable error){runOnUiThread(()->{if(token==generation&&!closed){if(kind.equals("archive_import"))pendingImport=null;if(kind.equals("archive")){exportEvent("export_stage_failed",e.optString("ticket"),safe(error));return;}if(kind.equals("artifact_file")||kind.equals("archive_drive")){exportEvent("artifact_failed",e.optString("ticket"),safe(error));return;}try{dispatch("failure",addMeasurements(new JSONObject(),started,metrics).put("error",safe(error)).put("purpose",e.optString("purpose")).put("model_ticket",e.optString("model_ticket")).put("web_ticket",e.optString("web_ticket")).put("ticket",e.optString("ticket")).put("phase",e.optString("phase")).put("sent",sending[0]));}catch(JSONException ex){fatal(safe(ex));}}});}finally{if(snapshotHandle!=0)try{Core.call(new JSONObject().put("op",chunkedSnapshot?"project_chunks":"project_close").put("action","close").put("handle",snapshotHandle));}catch(Exception ignored){}}});
    }
    private void mcpDiscardedSuccess(JSONObject command,int token){
        if(closed||token==generation||!command.optBoolean("stream")||!command.optString("stream_protocol").equals("mcp"))return;
        JSONObject context=command.optJSONObject("mcp_control");if(context==null)return;
        String request=context.optString("request");if(!request.equals("tools/list")&&!request.equals("tools/call"))return;
        // readStream already consumed its finished handle. The discarded success owns
        // only this old pinned session cleanup, never another result or original call.
        try{queueMcpCleanup((JSONArray)Core.call(new JSONObject().put("op","mcp_cleanup_plan").put("context",context)),android.os.SystemClock.elapsedRealtime()+2000);}catch(Exception ignored){}
    }
    private void transferRecovery(JSONObject recovered)throws Exception {
        JSONObject stored=transferStore.load(key());if(stored==null)return;
        if(stored.has("cleared_ticket"))recovered.put("project_cleared_ticket",stored.getString("cleared_ticket"));else recovered.put("project_record",stored.getJSONObject("record"));
    }
    private void stageTransfer(JSONObject e,String encoded,boolean artifact,int token)throws Exception {
        byte[] bytes=Base64.decode(encoded,Base64.NO_WRAP);JSONObject metadata=(JSONObject)Core.call(new JSONObject().put("op","project_record").put("prepare",e.getJSONObject("prepare")).put("bytes",Integer.toString(bytes.length)));
        try{JSONObject committed=documentJobs.submit(()->{if(token!=generation||closed)throw new IOException("Snapshot preparation cancelled");return transferStore.stage(key(),metadata,bytes);}).get();
            runOnUiThread(()->{if(closed)return;try{if(token==generation)dispatch(artifact?"artifact_built":"archive_built",new JSONObject().put("ticket",e.getString("ticket")).put("external",true).put("record",committed));dispatch("project_recovered",new JSONObject().put("project_record",committed));}catch(Exception problem){Toast.makeText(this,safe(problem),Toast.LENGTH_LONG).show();}});
        }catch(Exception error){JSONObject recovered=documentJobs.submit(()->{JSONObject value=new JSONObject();try{transferRecovery(value);}catch(Exception problem){value.put("project_error",safe(problem));}return value;}).get();runOnUiThread(()->{if(!closed){try{dispatch("artifact_failed",new JSONObject().put("ticket",e.optString("ticket")));dispatch("project_recovered",recovered);}catch(JSONException ignored){}}});throw error;}
    }
    private static JSONObject chunkInput(String action,long handle)throws JSONException{return new JSONObject().put("op","project_chunks").put("action",action).put("handle",handle);}
    private static byte[] chunkExpected(InputStream in,int length)throws IOException {
        byte[] bytes=new byte[length];int total=0;while(total<length){int n=in.read(bytes,total,length-total);if(n<0)break;total+=n;}return total==length?bytes:Arrays.copyOf(bytes,total);
    }
    private final class SourceInput extends InputStream {
        final long handle;final int token;byte[] data=new byte[0];int offset;boolean ended;
        SourceInput(long handle,int token){this.handle=handle;this.token=token;}
        private void active()throws IOException{if(token!=generation||closed)throw new IOException("Snapshot preparation cancelled");}
        public int read(byte[] into,int start,int count)throws IOException{
            if(count==0)return 0;active();try{if(offset==data.length){if(ended)return -1;JSONObject part=(JSONObject)Core.call(chunkInput("source_next",handle));data=Base64.decode(part.getString("data"),Base64.NO_WRAP);offset=0;ended=part.getBoolean("done");if(data.length==0)return -1;}int n=Math.min(count,data.length-offset);System.arraycopy(data,offset,into,start,n);offset+=n;return n;}catch(Exception error){throw new IOException("Source snapshot failed",error);}
        }
        public int read()throws IOException{byte[] b=new byte[1];return read(b,0,1)<0?-1:b[0]&255;}
        void rewind(){try{active();Core.call(chunkInput("source_rewind",handle));data=new byte[0];offset=0;ended=false;}catch(Exception error){throw new IllegalStateException(error);}}
    }
    private void stageArtifact(JSONObject e,int token)throws Exception{
        JSONObject opened=(JSONObject)Core.call(new JSONObject().put("op","project_chunks").put("action","source_open").put("root",root()).put("path",e.getString("path")));long handle=opened.getLong("handle");
        try{JSONObject metadata=(JSONObject)Core.call(new JSONObject().put("op","project_chunks").put("action","record").put("prepare",e.getJSONObject("prepare")).put("bytes",Long.toString(opened.getLong("bytes"))));SourceInput input=new SourceInput(handle,token);
            JSONObject committed=documentJobs.submit(()->{if(token!=generation||closed)throw new IOException("Snapshot preparation cancelled");return transferStore.stageChunks(key(),metadata,input,()->input.rewind());}).get();
            runOnUiThread(()->{if(closed)return;try{if(token==generation)dispatch("artifact_built",new JSONObject().put("ticket",e.getString("ticket")).put("external",true).put("record",committed));dispatch("project_recovered",new JSONObject().put("project_record",committed));}catch(Exception problem){Toast.makeText(this,safe(problem),Toast.LENGTH_LONG).show();}});
        }catch(Exception error){JSONObject recovered=documentJobs.submit(()->{JSONObject value=new JSONObject();try{transferRecovery(value);}catch(Exception problem){value.put("project_error",safe(problem));}return value;}).get();runOnUiThread(()->{if(!closed)try{dispatch("artifact_failed",new JSONObject().put("ticket",e.optString("ticket")));dispatch("project_recovered",recovered);}catch(Exception ignored){}});throw error;
        }finally{Core.call(chunkInput("source_close",handle));}
    }
    private TransferStore.ChunkReader openTransfer(JSONObject e)throws Exception{
        TransferStore.ChunkReader stream=transferStore.openChunks(key());try{Core.call(new JSONObject().put("op","project_chunks").put("action","binding").put("record",e.getJSONObject("record")).put("stored",stream.header.getJSONObject("record")));return stream;}catch(Exception error){stream.close();throw error;}
    }
    private JSONObject transferNetwork(JSONObject e,int token)throws Exception{
        String host=e.getString("host");int port=e.getInt("port");long handle=e.getLong("snapshot_handle");Socket raw=new Socket();activeConnections.add(raw);ScheduledFuture<?> timeout=deadlines.schedule(()->{try{raw.close();}catch(IOException ignored){}},e.getInt("deadline_seconds"),TimeUnit.SECONDS);
        try(TransferStore.ChunkReader expected=documentJobs.submit(()->openTransfer(e)).get()){
            raw.connect(new InetSocketAddress(host,port),15000);raw.setSoTimeout(120000);Socket connection=raw;
            if(e.optBoolean("tls")){SSLSocket tls=(SSLSocket)((SSLSocketFactory)SSLSocketFactory.getDefault()).createSocket(raw,host,port,true);SSLParameters params=tls.getSSLParameters();params.setEndpointIdentificationAlgorithm("HTTPS");tls.setSSLParameters(params);tls.setSoTimeout(120000);tls.startHandshake();connection=tls;}
            try(Socket managed=connection){if(token!=generation||closed)throw new IOException("Cancelled");OutputStream output=managed.getOutputStream();output.write(e.getString("wire").getBytes(StandardCharsets.UTF_8));
                if(e.getString("phase").equals("write")){try(TransferStore.ChunkReader payload=documentJobs.submit(()->openTransfer(e)).get()){byte[] b=new byte[65536];int n;while((n=payload.read(b))!=-1){if(token!=generation||closed)throw new IOException("Cancelled");output.write(b,0,n);}}}output.flush();
                InputStream in=managed.getInputStream();byte[] buffer=new byte[65536];int n,status=0;
                while((n=in.read(buffer))!=-1){if(token!=generation||closed)throw new IOException("Cancelled");JSONObject part=(JSONObject)Core.call(chunkInput("feed",handle).put("data",Base64.encodeToString(Arrays.copyOf(buffer,n),Base64.NO_WRAP)));status=part.getInt("status");byte[] body=Base64.decode(part.getString("data"),Base64.NO_WRAP);byte[] original=status==200?chunkExpected(expected,body.length):new byte[0];Core.call(chunkInput("compare",handle).put("data","").put("expected",Base64.encodeToString(original,Base64.NO_WRAP)));}
                if(status==200&&expected.read()!=-1)throw new IOException("Destination readback ended early");if(token!=generation||closed)throw new IOException("Cancelled");return (JSONObject)Core.call(chunkInput("finish",handle));
            }
        }finally{timeout.cancel(false);try{raw.close();}catch(IOException ignored){}activeConnections.remove(raw);}
    }
    private static JSONObject addMeasurements(JSONObject result,long started,long[] metrics)throws JSONException{
        result.put("elapsed_ms",android.os.SystemClock.elapsedRealtime()-started).put("received_bytes",metrics[0]);if(metrics[3]>=0)result.put("sent_bytes",metrics[3]);
        if(metrics[1]>=0)result.put("first_byte_ms",metrics[1]);if(metrics[2]>=0)result.put("first_text_ms",metrics[2]);return result;
    }
    private static InputStream measuredInput(InputStream input,long started,long[] metrics){return new java.io.FilterInputStream(input){
        private void observed(int count){if(count>0){metrics[0]+=count;if(metrics[1]<0)metrics[1]=android.os.SystemClock.elapsedRealtime()-started;}}
        public int read()throws IOException{int value=in.read();if(value>=0)observed(1);return value;}
        public int read(byte[] b,int offset,int length)throws IOException{int count=in.read(b,offset,length);observed(count);return count;}
    };}
    private void startRateClock(int token,String ticket,long started){
        if(rateTick!=null)rateHandler.removeCallbacks(rateTick);rateTicket=ticket;rateStarted=started;
        rateTick=new Runnable(){public void run(){
            if(closed||token!=generation||!ticket.equals(state.optString("model_expected"))){rateTick=null;return;}
            if(ratePaused)return;
            if(state.optString("page").equals("chat")||state.optString("page").equals("diagnostics"))try{dispatch("rate_tick",new JSONObject().put("model_ticket",ticket).put("elapsed_ms",android.os.SystemClock.elapsedRealtime()-started));}catch(JSONException e){fatal(safe(e));}
            rateHandler.postDelayed(this,500);
        }};if(!ratePaused)rateHandler.postDelayed(rateTick,500);
    }
    private void streamProgress(int token,String text,String ticket,long started){
        if(ticket.isEmpty())return;
        long elapsed=android.os.SystemClock.elapsedRealtime()-started;
        runOnUiThread(()->{if(token==generation&&!closed)try{dispatch("model_progress",new JSONObject().put("stream_text",text).put("elapsed_ms",elapsed).put("model_ticket",ticket));}catch(JSONException error){fatal(safe(error));}});
    }
    private byte[] readStream(InputStream input,int token,int limit,String protocol,long started,long[] metrics,String ticket,JSONObject control)throws Exception{
        JSONObject opening=new JSONObject().put("op","stream_open").put("protocol",protocol);if(control!=null)opening.put("mcp_control",control);
        long id;try{id=((Number)Core.call(opening)).longValue();}catch(Exception problem){
            if(control!=null)try{JSONArray cleanup=(JSONArray)Core.call(new JSONObject().put("op",control.has("resume_token")||control.has("admission_token")?"stream_open_cleanup":"mcp_cleanup_plan").put("context",control));if(!closed)queueMcpCleanup(cleanup,Math.min(started+120000,android.os.SystemClock.elapsedRealtime()+2000));}catch(Exception ignored){}
            throw problem;
        }
        StringBuilder text=new StringBuilder();int published=0,total=0;long previous=0,serverPublished=0;JSONObject serverObservation=null;boolean serverDirty=false,finished=false,cleanupHandled=false,originalRead=false;
        try{
            byte[] buffer=new byte[8192];int count=-2;
            while(true){
                originalRead=true;count=input.read(buffer);originalRead=false;if(count==-1)break;
                if(token!=generation||closed)throw new IOException("Cancelled");
                total+=count;if(total>Math.min(2*1024*1024,limit))throw new IOException("Stream exceeds the current response memory budget");
                JSONObject part=(JSONObject)Core.call(new JSONObject().put("op","stream_feed").put("id",id).put("data",Base64.encodeToString(Arrays.copyOf(buffer,count),Base64.NO_WRAP)));
                text.append(part.optString("delta"));long now=android.os.SystemClock.elapsedRealtime();if(metrics[2]<0&&text.length()>0)metrics[2]=now-started;
                if(protocol.equals("mcp_server")&&part.optJSONObject("observation")!=null){serverObservation=part.getJSONObject("observation");serverDirty=true;if(now-serverPublished>=250){mcpServerProgress(token,serverObservation,control.getString("owner"));serverPublished=now;serverDirty=false;}}
                if(part.has("error")){JSONArray cleanup=part.optJSONArray("cleanup");if(cleanup!=null&&!closed){queueMcpCleanup(cleanup,Math.min(started+120000,android.os.SystemClock.elapsedRealtime()+2000));cleanupHandled=true;}throw new IOException(part.getString("error"));}
                boolean complete=part.optBoolean("complete");JSONArray controls=part.optJSONArray("controls");
                if(controls!=null)for(int i=0;i<controls.length();i++){
                    JSONObject response=controls.getJSONObject(i);JSONObject accepted=mcpControl(response,id,token,started);
                    if(token!=generation||closed)throw new IOException("Cancelled during MCP control response");
                    JSONObject ack=(JSONObject)Core.call(new JSONObject().put("op","stream_control_ack").put("id",id).put("ticket",response.get("ticket")).put("owner",response.getString("owner")).put("data",accepted.getString("data")).put("eof",accepted.getBoolean("eof")));
                    complete=ack.optBoolean("complete");
                }
                if(text.length()!=published&&now-previous>=250){streamProgress(token,text.toString(),ticket,started);published=text.length();previous=now;}
                if(complete)break;
            }
            if(count==-1&&control!=null&&control.optBoolean("resumable")&&mcpPauseOwner.equals(control.optString("owner"))&&token==generation&&!closed){
                try{JSONObject paused=(JSONObject)Core.call(new JSONObject().put("op","stream_suspend").put("id",id).put("owner",control.getString("owner")));finished=true;return Base64.decode(paused.getString("data"),Base64.NO_WRAP);}finally{mcpPauseOwner="";}
            }
            JSONObject complete=(JSONObject)Core.call(new JSONObject().put("op","stream_finish").put("id",id).put("eof",count==-1));finished=true;return Base64.decode(complete.getString("data"),Base64.NO_WRAP);
        }catch(Exception problem){
            if(originalRead&&control!=null&&control.optBoolean("resumable")&&mcpPauseOwner.equals(control.optString("owner"))&&token==generation&&!closed){
                try{JSONObject paused=(JSONObject)Core.call(new JSONObject().put("op","stream_suspend").put("id",id).put("owner",control.getString("owner")));finished=true;return Base64.decode(paused.getString("data"),Base64.NO_WRAP);}catch(Exception unavailable){throw new IOException("Stream could not retain a resume cursor: "+safe(unavailable),problem);}finally{mcpPauseOwner="";}
            }
            throw problem;
        }finally{
            if(serverDirty&&serverObservation!=null&&control!=null)mcpServerProgress(token,serverObservation,control.getString("owner"));
            if(text.length()!=published)streamProgress(token,text.toString(),ticket,started);
            if(control!=null&&!closed&&!finished&&!cleanupHandled){long ending=Math.min(started+120000,android.os.SystemClock.elapsedRealtime()+2000);try{queueMcpCleanup((JSONArray)Core.call(new JSONObject().put("op","stream_abandon").put("id",id).put("owner",control.getString("owner")).put("cancel",token!=generation)),ending);}catch(Exception ignored){try{queueMcpCleanup((JSONArray)Core.call(new JSONObject().put("op","mcp_cleanup_plan").put("context",control)),ending);}catch(Exception ignoredAgain){}}}
            try{Core.call(new JSONObject().put("op","stream_close").put("id",id));}catch(Exception ignored){}
            if(control!=null&&mcpPauseOwner.equals(control.optString("owner")))mcpPauseOwner="";
        }
    }
    private void mcpServerProgress(int token,JSONObject counts,String owner){
        runOnUiThread(()->{if(token==generation&&!closed)try{dispatch("rea_server_progress",new JSONObject().put("purpose",owner).put("counts",counts));}catch(JSONException error){fatal(safe(error));}});
    }
    // Platform transport only: Rust owns the destination, message and one-use acknowledgment.
    private JSONObject mcpControl(JSONObject command,long stream,int token,long started)throws Exception{
        long remaining=120000-(android.os.SystemClock.elapsedRealtime()-started);
        if(token!=generation||closed||remaining<=0)throw new IOException("MCP request cancelled or timed out");
        String host=command.getString("host");int port=command.getInt("port");Socket raw=new Socket();activeConnections.add(raw);
        ScheduledFuture<?> timeout=deadlines.schedule(()->{try{raw.close();}catch(IOException ignored){}},remaining,TimeUnit.MILLISECONDS);
        try{
            if(token!=generation||closed)throw new IOException("Cancelled before MCP control connection");
            raw.connect(new InetSocketAddress(host,port),(int)Math.min(15000,remaining));raw.setSoTimeout((int)remaining);Socket connection=raw;
            if(command.optBoolean("tls")){SSLSocket tls=(SSLSocket)((SSLSocketFactory)SSLSocketFactory.getDefault()).createSocket(raw,host,port,true);SSLParameters params=tls.getSSLParameters();params.setEndpointIdentificationAlgorithm("HTTPS");tls.setSSLParameters(params);tls.setSoTimeout((int)remaining);tls.startHandshake();connection=tls;}
            try(Socket managed=connection){
                if(token!=generation||closed||android.os.SystemClock.elapsedRealtime()-started>=120000)throw new IOException("Cancelled before MCP control write");
                managed.getOutputStream().write(command.getString("wire").getBytes(StandardCharsets.UTF_8));managed.getOutputStream().flush();
                ByteArrayOutputStream accepted=new ByteArrayOutputStream();byte[] buffer=new byte[1024];int count;InputStream input=managed.getInputStream();
                while((count=input.read(buffer))!=-1){
                    if(token!=generation||closed||android.os.SystemClock.elapsedRealtime()-started>=120000)throw new IOException("MCP control response cancelled or timed out");
                    if(accepted.size()+count>16384)throw new IOException("MCP control acknowledgment exceeds 16 KiB");accepted.write(buffer,0,count);
                    JSONObject framing=(JSONObject)Core.call(new JSONObject().put("op","stream_control_probe").put("id",stream).put("ticket",command.get("ticket")).put("owner",command.getString("owner")).put("data",Base64.encodeToString(accepted.toByteArray(),Base64.NO_WRAP)));
                    if(framing.optBoolean("ready"))break;
                }
                byte[] result=accepted.toByteArray();
                if(token!=generation||closed||android.os.SystemClock.elapsedRealtime()-started>=120000)throw new IOException("Cancelled during MCP control response");return new JSONObject().put("data",Base64.encodeToString(result,Base64.NO_WRAP)).put("eof",count==-1);
            }
        }finally{timeout.cancel(false);try{raw.close();}catch(IOException ignored){}activeConnections.remove(raw);}
    }
    // Best-effort teardown has no callback and cannot complete or fail a newer operation.
    // The one worker, four queued plans, two requests/plan and original-or-2s deadline
    // bound detached work. Destruction refuses queued jobs and closes every cleanup socket.
    private void queueMcpCleanup(JSONArray plan,long ending){
        if(closed||plan.length()==0||plan.length()>2)return;
        try{cleanupWorkers.allowCoreThreadTimeOut(true);cleanupWorkers.execute(()->{
            for(int i=0;i<plan.length();i++){
                if(closed||Thread.currentThread().isInterrupted()||android.os.SystemClock.elapsedRealtime()>=ending)return;
                try{mcpCleanup(plan.getJSONObject(i),ending);}catch(Exception ignored){}
            }
        });}catch(RejectedExecutionException ignored){}
    }
    private void mcpCleanup(JSONObject command,long ending)throws Exception{
        long remaining=ending-android.os.SystemClock.elapsedRealtime();if(closed||remaining<=0)return;
        byte[] wire=command.getString("wire").getBytes(StandardCharsets.UTF_8);if(wire.length>24576)throw new IOException("MCP cleanup request exceeds bound");
        String host=command.getString("host");int port=command.getInt("port");Socket raw=new Socket();cleanupConnections.add(raw);
        ScheduledFuture<?> timeout=null;
        try{
            timeout=deadlines.schedule(()->{try{raw.close();}catch(IOException ignored){}},remaining,TimeUnit.MILLISECONDS);
            if(closed)throw new IOException("Activity closed before MCP cleanup");
            raw.connect(new InetSocketAddress(host,port),(int)remaining);raw.setSoTimeout((int)remaining);Socket connection=raw;
            if(command.optBoolean("tls")){SSLSocket tls=(SSLSocket)((SSLSocketFactory)SSLSocketFactory.getDefault()).createSocket(raw,host,port,true);SSLParameters params=tls.getSSLParameters();params.setEndpointIdentificationAlgorithm("HTTPS");tls.setSSLParameters(params);tls.setSoTimeout((int)remaining);tls.startHandshake();connection=tls;}
            try(Socket managed=connection){
                if(closed||android.os.SystemClock.elapsedRealtime()>=ending)throw new IOException("MCP cleanup expired before write");
                managed.getOutputStream().write(wire);managed.getOutputStream().flush();
                ByteArrayOutputStream response=new ByteArrayOutputStream();InputStream input=managed.getInputStream();byte[] buffer=new byte[1024];int count;
                while((count=input.read(buffer))!=-1){
                    if(closed||android.os.SystemClock.elapsedRealtime()>=ending)throw new IOException("MCP cleanup expired");
                    if(response.size()+count>16384)throw new IOException("MCP cleanup acknowledgment exceeds bound");response.write(buffer,0,count);
                    JSONObject framed=(JSONObject)Core.call(new JSONObject().put("op","mcp_cleanup_response").put("cleanup",command.getString("cleanup")).put("data",Base64.encodeToString(response.toByteArray(),Base64.NO_WRAP)).put("eof",false));
                    if(framed.optBoolean("ready"))return;
                }
                Core.call(new JSONObject().put("op","mcp_cleanup_response").put("cleanup",command.getString("cleanup")).put("data",Base64.encodeToString(response.toByteArray(),Base64.NO_WRAP)).put("eof",true));
            }
        }finally{if(timeout!=null)timeout.cancel(false);try{raw.close();}catch(IOException ignored){}cleanupConnections.remove(raw);}
    }
    private void export(){dispatch("conversation_export");}
    private void exportEvent(String action,String ticket,String error){runOnUiThread(()->{if(closed)return;try{JSONObject in=new JSONObject().put("ticket",ticket);if(error!=null)in.put("error",error);dispatch(action,in);}catch(Exception problem){Toast.makeText(this,safe(problem),Toast.LENGTH_LONG).show();}});}
    private void stageExport(JSONObject metadata,byte[] bytes,JSONObject archiveInfo){
        documentJobs.execute(()->{String ticket=metadata.optString("ticket");try{
            exportStore.stage(key(),metadata,bytes);
            runOnUiThread(()->{if(closed)return;try{
                if(archiveInfo!=null)dispatch("archive_ready",new JSONObject().put("ticket",ticket).put("files",archiveInfo.getInt("files")).put("directories",archiveInfo.optInt("directories")).put("bytes",bytes.length));
                dispatch("export_staged",new JSONObject().put("ticket",ticket).put("bytes",bytes.length));
            }catch(Exception problem){exportEvent("export_failed",ticket,safe(problem));}});
        }catch(Exception problem){exportEvent("export_stage_failed",ticket,safe(problem));}});
    }
    private void openSavedExport(String ticket){documentJobs.execute(()->{try{
        JSONObject record=exportStore.load(key());if(record==null||!ticket.equals(record.getString("ticket")))throw new IOException("Saved snapshot is unavailable or changed");
        runOnUiThread(()->{if(closed)return;try{
            JSONObject current=state.optJSONObject("export_pending");if(current==null||!ticket.equals(current.optString("ticket"))||!(current.optString("phase").equals("ready")||current.optString("phase").equals("selecting")))return;
            cancelledExports.remove(ticket);pendingExportTicket=ticket;
            Intent intent=new Intent(Intent.ACTION_CREATE_DOCUMENT).setType(record.getString("mime")).addCategory(Intent.CATEGORY_OPENABLE).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_WRITE_URI_PERMISSION).putExtra(Intent.EXTRA_TITLE,record.getString("name"));
            startActivityForResult(intent,42);
        }catch(Exception problem){pendingExportTicket=null;exportEvent("export_failed",ticket,safe(problem));}});
    }catch(Exception problem){exportEvent("export_failed",ticket,safe(problem));}});}
    private void finishSavedExport(int result,Intent data){
        final String ticket=pendingExportTicket;pendingExportTicket=null;
        if(ticket==null){Toast.makeText(this,"Export result has no saved ticket. Use Review saved export to choose a destination again.",Toast.LENGTH_LONG).show();return;}
        if(result!=RESULT_OK||data==null||data.getData()==null){exportEvent("export_failed",ticket,"Destination selection cancelled");return;}
        final Uri uri=data.getData();if(!"content".equals(uri.getScheme())||uri.getAuthority()==null){exportEvent("export_failed",ticket,"Unsupported destination URI");return;}
        try{dispatch("export_started",new JSONObject().put("ticket",ticket));JSONObject current=state.optJSONObject("export_pending");if(current==null||!ticket.equals(current.optString("ticket"))||!current.optString("phase").equals("writing"))return;}catch(Exception problem){exportEvent("export_failed",ticket,safe(problem));return;}
        documentJobs.execute(()->{try{
            exportActive(ticket,null);
            JSONObject record=exportStore.load(key());if(record==null||!ticket.equals(record.getString("ticket")))throw new IOException("Saved snapshot changed; no bytes were written");
            writeDocument(uri,ExportStore.payload(record),ticket);
            exportActive(ticket,null);
            exportStore.clear(key(),ticket);exportEvent("export_done",ticket,null);
        }catch(Exception problem){exportEvent("export_failed",ticket,safe(problem));}finally{cancelledExports.remove(ticket);}});
    }
    private void deliverPickedDocument(int token,Runnable result){runOnUiThread(()->{if(token!=generation||closed)return;result.run();});}
    private byte[] pickedDocumentBytes(Uri uri,int limit,int token)throws Exception {
        android.os.CancellationSignal signal=new android.os.CancellationSignal();documentSignals.add(signal);
        java.util.concurrent.atomic.AtomicReference<android.os.ParcelFileDescriptor> opened=new java.util.concurrent.atomic.AtomicReference<>();
        java.util.concurrent.atomic.AtomicBoolean expired=new java.util.concurrent.atomic.AtomicBoolean();ScheduledFuture<?> timeout=null;
        try{
            documentActive(token,signal);
            timeout=deadlines.schedule(()->{expired.set(true);signal.cancel();android.os.ParcelFileDescriptor file=opened.get();if(file!=null)try{file.close();}catch(IOException ignored){}},120,TimeUnit.SECONDS);
            try(android.content.res.AssetFileDescriptor asset=getContentResolver().openAssetFileDescriptor(uri,"r",signal)){
                if(asset==null)throw new IOException("Selected document is unavailable");android.os.ParcelFileDescriptor file=asset.getParcelFileDescriptor();opened.set(file);documentFiles.add(file);documentActive(token,signal);
                try(InputStream input=asset.createInputStream()){byte[] bytes=read(input,limit);documentActive(token,signal);return bytes;}
            }
        }catch(Exception problem){if(expired.get())throw new IOException("Selected document read timed out",problem);throw problem;}
        finally{if(timeout!=null)timeout.cancel(false);documentSignals.remove(signal);android.os.ParcelFileDescriptor file=opened.get();if(file!=null)documentFiles.remove(file);}
    }
    @Override protected void onActivityResult(int request,int result,Intent data){
        super.onActivityResult(request,result,data);
        if(request==48){dispatch("phone_access_status");return;}
        if(request==42||request==44){finishSavedExport(result,data);return;}
        if(result!=RESULT_OK||data==null||data.getData()==null){if(request==45){pendingImport=null;dispatch("archive_reject");}return;}
        Uri uri=data.getData();
        if(!"content".equals(uri.getScheme())||uri.getAuthority()==null){Toast.makeText(this,"Choose a document through Android storage; this URI is unsupported",Toast.LENGTH_LONG).show();return;}
        if(request==47){try{getContentResolver().takePersistableUriPermission(uri,Intent.FLAG_GRANT_READ_URI_PERMISSION);dispatch("wallpaper_result",new JSONObject().put("uri",uri.toString()));}catch(Exception e){Toast.makeText(this,safe(e),Toast.LENGTH_LONG).show();}return;}
        if(request==43){try{getContentResolver().takePersistableUriPermission(uri,data.getFlags()&(Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_WRITE_URI_PERMISSION));dispatch("folder_result",new JSONObject().put("uri",uri.toString()));}catch(Exception e){Toast.makeText(this,safe(e),Toast.LENGTH_LONG).show();}return;}
        if(request!=41&&request!=45&&request!=46)return;
        final int token=generation;
        workers.execute(()->{try{
            byte[] bytes=pickedDocumentBytes(uri,request==45?5*1024*1024:1024*1024,token);
            if(request==46){
                String content=StandardCharsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(bytes)).toString();
                deliverPickedDocument(token,()->{try{dispatch("conversation_review",new JSONObject().put("content",content));}catch(JSONException e){fatal(safe(e));}});
            }else if(request==45){
                JSONObject preview=(JSONObject)Core.call(new JSONObject().put("op","archive_preview").put("archive",Base64.encodeToString(bytes,Base64.NO_WRAP)));
                deliverPickedDocument(token,()->{pendingImport=bytes;try{dispatch("archive_review",new JSONObject().put("names",preview.getJSONArray("names")).put("bytes",preview.getInt("bytes")));}catch(JSONException e){pendingImport=null;fatal(safe(e));}});
            }else{
                String content=StandardCharsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(bytes)).toString();if(content.indexOf('\0')>=0)throw new IOException("Select a UTF-8 text or source-code document");
                deliverPickedDocument(token,()->{try{dispatch("document_result",new JSONObject().put("content",content));}catch(JSONException e){fatal(safe(e));}});
            }
        }catch(Exception e){deliverPickedDocument(token,()->{if(request==45||request==46){if(request==45)pendingImport=null;try{dispatch("failure",new JSONObject().put("error",safe(e)));}catch(JSONException ignored){Toast.makeText(this,safe(e),Toast.LENGTH_LONG).show();}}else Toast.makeText(this,safe(e),Toast.LENGTH_LONG).show();});}});
    }
    private void exportActive(String ticket,android.os.CancellationSignal signal)throws IOException{if(closed||cancelledExports.contains(ticket))throw new IOException("Export cancelled or interrupted; inspect destination before retrying");if(signal!=null)signal.throwIfCanceled();}
    private void writeDocument(Uri uri,byte[] bytes,String ticket)throws IOException{
        android.os.CancellationSignal signal=new android.os.CancellationSignal();exportSignals.add(signal);
        try{
            exportActive(ticket,signal);
            android.os.ParcelFileDescriptor file=getContentResolver().openFileDescriptor(uri,"wt",signal);if(file==null)throw new IOException("Unable to open selected document");exportFiles.add(file);
            try(OutputStream out=new android.os.ParcelFileDescriptor.AutoCloseOutputStream(file)){exportActive(ticket,signal);out.write(bytes);out.flush();}finally{exportFiles.remove(file);}
            exportActive(ticket,signal);file=getContentResolver().openFileDescriptor(uri,"r",signal);if(file==null)throw new IOException("Unable to read back selected document");exportFiles.add(file);
            try(InputStream in=new android.os.ParcelFileDescriptor.AutoCloseInputStream(file)){if(!Arrays.equals(bytes,read(in,bytes.length)))throw new IOException("Document readback differs from requested bytes");}finally{exportFiles.remove(file);}
            exportActive(ticket,signal);
        }catch(Exception error){throw new IOException("Selected document may remain incomplete or unverified. "+safe(error),error);}finally{exportSignals.remove(signal);}
    }
    @Override protected void onSaveInstanceState(Bundle bundle){super.onSaveInstanceState(bundle);if(pendingExportTicket!=null)bundle.putString("export_ticket",pendingExportTicket);}
    private void stopExecution(){if(executionMessenger!=null){try{executionMessenger.send(Message.obtain(null,2));}catch(Exception ignored){}executionMessenger=null;}if(executionConnection!=null){try{unbindService(executionConnection);}catch(Exception ignored){}executionConnection=null;}}
    private void startExecution(JSONObject program,int token){
        final Messenger callback=new Messenger(new Handler(Looper.getMainLooper()){@Override public void handleMessage(Message msg){if(token!=generation||closed)return;stopExecution();try{Bundle b=msg.getData();if(b.containsKey("error"))dispatch("failure",new JSONObject().put("error",b.getString("error")));else dispatch("tool_result",new JSONObject().put("output",b.getString("output")));}catch(Exception e){fatal(safe(e));}}});
        executionConnection=new ServiceConnection(){@Override public void onServiceConnected(ComponentName name,IBinder service){if(token!=generation||closed){try{unbindService(this);}catch(Exception ignored){}return;}executionMessenger=new Messenger(service);try{Message msg=Message.obtain(null,1);Bundle data=new Bundle();data.putString("source",program.getString("source"));data.putBoolean("check",program.getBoolean("check"));msg.setData(data);msg.replyTo=callback;executionMessenger.send(msg);}catch(Exception e){stopExecution();try{dispatch("failure",new JSONObject().put("error",safe(e)));}catch(Exception failure){fatal(safe(failure));}}}
            @Override public void onServiceDisconnected(ComponentName name){if(token!=generation||closed)return;stopExecution();try{dispatch("failure",new JSONObject().put("error","Isolated execution process disconnected"));}catch(Exception e){fatal(safe(e));}}};
        if(!bindService(new Intent(this,Execution.class),executionConnection,BIND_AUTO_CREATE)){stopExecution();try{dispatch("failure",new JSONObject().put("error","Could not start isolated execution"));}catch(Exception e){fatal(safe(e));}}
    }
    private void cancelExportDocuments(){for(android.os.CancellationSignal signal:exportSignals)signal.cancel();for(android.os.ParcelFileDescriptor file:exportFiles)try{file.close();}catch(IOException ignored){}}
    private void cancelDocuments(){cancelExportDocuments();for(android.os.CancellationSignal signal:documentSignals)signal.cancel();for(android.os.ParcelFileDescriptor file:documentFiles)try{file.close();}catch(IOException ignored){}}
    private void documentActive(int token,android.os.CancellationSignal signal)throws IOException{signal.throwIfCanceled();if(closed||token!=generation||Thread.currentThread().isInterrupted())throw new IOException("Phone operation cancelled");}
    private byte[] phoneBytes(Uri file,int token,android.os.CancellationSignal signal)throws Exception {
        documentActive(token,signal);android.os.ParcelFileDescriptor descriptor=getContentResolver().openFileDescriptor(file,"r",signal);
        if(descriptor==null)throw new IOException("Phone document unavailable");documentFiles.add(descriptor);
        try(InputStream input=new android.os.ParcelFileDescriptor.AutoCloseInputStream(descriptor)){byte[] bytes=read(input,1024*1024);documentActive(token,signal);return bytes;}finally{documentFiles.remove(descriptor);}
    }
    private String[] phoneIdentity(Uri tree,String path,int token,android.os.CancellationSignal signal)throws Exception {
        String id=DocumentsContract.getTreeDocumentId(tree),parent=id;
        for(String segment:path.split("/",-1)){
            documentActive(token,signal);String next=null;parent=id;
            try(Cursor cursor=getContentResolver().query(DocumentsContract.buildChildDocumentsUriUsingTree(tree,parent),new String[]{DocumentsContract.Document.COLUMN_DOCUMENT_ID,DocumentsContract.Document.COLUMN_DISPLAY_NAME},null,null,null,signal)){
                if(cursor==null)throw new IOException("Phone folder unavailable");int count=0;
                while(cursor.moveToNext()){if(++count>5000)throw new IOException("Phone folder lookup limit exceeded");if(segment.equals(cursor.getString(1))){if(next!=null)throw new IOException("Ambiguous phone filename");next=cursor.getString(0);}}
            }
            if(next==null)throw new IOException("Phone document not found: "+segment);id=next;
        }
        Uri file=DocumentsContract.buildDocumentUriUsingTree(tree,id);
        try(Cursor cursor=getContentResolver().query(file,new String[]{DocumentsContract.Document.COLUMN_MIME_TYPE,DocumentsContract.Document.COLUMN_FLAGS},null,null,null,signal)){
            if(cursor==null||!cursor.moveToFirst())throw new IOException("Phone document metadata unavailable");
            if(DocumentsContract.Document.MIME_TYPE_DIR.equals(cursor.getString(0))||(cursor.getInt(1)&DocumentsContract.Document.FLAG_SUPPORTS_WRITE)==0)throw new IOException("Phone provider does not permit updating this file");
        }
        return new String[]{id,parent};
    }
    private JSONObject phoneRevision(JSONObject e,int token)throws Exception {
        android.os.CancellationSignal signal=new android.os.CancellationSignal();documentSignals.add(signal);
        try{
            documentActive(token,signal);Uri tree=Uri.parse(e.getString("tree"));boolean granted=false;
            for(android.content.UriPermission grant:getContentResolver().getPersistedUriPermissions())if(grant.getUri().equals(tree)&&grant.isReadPermission()&&grant.isWritePermission())granted=true;
            if(!granted)throw new IOException("Grant read and write permission to this phone folder through Android");
            String phase=e.getString("phase");JSONObject result=new JSONObject();
            if(phase.equals("read")){
                String[] identity=phoneIdentity(tree,e.getString("path"),token,signal);
                result.put("document_id",identity[0]).put("parent_id",identity[1]);
                result.put("data",Base64.encodeToString(phoneBytes(DocumentsContract.buildDocumentUriUsingTree(tree,identity[0]),token,signal),Base64.NO_WRAP));
            }else if(phase.equals("backup")){
                String parent=e.getString("parent_id"),name=e.getString("revision_name");Set<String> ids=new HashSet<>();
                try(Cursor cursor=getContentResolver().query(DocumentsContract.buildChildDocumentsUriUsingTree(tree,parent),new String[]{DocumentsContract.Document.COLUMN_DOCUMENT_ID,DocumentsContract.Document.COLUMN_DISPLAY_NAME},null,null,null,signal)){
                    if(cursor==null)throw new IOException("Phone folder unavailable");int count=0;
                    while(cursor.moveToNext()){if(++count>5000)throw new IOException("Phone folder lookup limit exceeded");ids.add(cursor.getString(0));if(name.equals(cursor.getString(1)))throw new IOException("Revision name already exists; no existing document was replaced");}
                }
                documentActive(token,signal);Uri created=DocumentsContract.createDocument(getContentResolver(),DocumentsContract.buildDocumentUriUsingTree(tree,parent),"text/plain",name);
                if(created==null||!tree.getAuthority().equals(created.getAuthority())||ids.contains(DocumentsContract.getDocumentId(created))||e.getString("document_id").equals(DocumentsContract.getDocumentId(created)))throw new IOException("Provider did not create a separate revision; no bytes were written");
                try(Cursor cursor=getContentResolver().query(created,new String[]{DocumentsContract.Document.COLUMN_DISPLAY_NAME},null,null,null,signal)){if(cursor==null||!cursor.moveToFirst())throw new IOException("Revision metadata unavailable");name=cursor.getString(0);}
                documentActive(token,signal);android.os.ParcelFileDescriptor descriptor=getContentResolver().openFileDescriptor(created,"w",signal);
                if(descriptor==null)throw new IOException("Could not open the new phone revision");documentFiles.add(descriptor);
                try(OutputStream out=new android.os.ParcelFileDescriptor.AutoCloseOutputStream(descriptor)){documentActive(token,signal);out.write(e.getString("previous").getBytes(StandardCharsets.UTF_8));out.flush();descriptor.getFileDescriptor().sync();}finally{documentFiles.remove(descriptor);}
                result.put("document_id",DocumentsContract.getDocumentId(created)).put("name",name).put("data",Base64.encodeToString(phoneBytes(created,token,signal),Base64.NO_WRAP));
            }else if(phase.equals("write")){
                String[] identity=phoneIdentity(tree,e.getString("path"),token,signal);Uri target=DocumentsContract.buildDocumentUriUsingTree(tree,identity[0]);
                documentActive(token,signal);android.os.ParcelFileDescriptor descriptor=getContentResolver().openFileDescriptor(target,"rw",signal);
                if(descriptor==null)throw new IOException("Provider cannot open this phone file for a checked update");documentFiles.add(descriptor);
                try{
                    // A duplicated descriptor gives the lock its own close ownership. The
                    // same original descriptor is read, checked, truncated and written.
                    try(android.os.ParcelFileDescriptor lockDescriptor=android.os.ParcelFileDescriptor.dup(descriptor.getFileDescriptor());FileOutputStream lockStream=new android.os.ParcelFileDescriptor.AutoCloseOutputStream(lockDescriptor);java.nio.channels.FileLock lock=lockStream.getChannel().tryLock()){
                        if(lock==null)throw new IOException("Phone file is locked by another writer; target unchanged");
                        FileDescriptor fd=descriptor.getFileDescriptor();android.system.Os.lseek(fd,0,android.system.OsConstants.SEEK_SET);
                        ByteArrayOutputStream before=new ByteArrayOutputStream();byte[] buffer=new byte[8192];int count;
                        while((count=android.system.Os.read(fd,buffer,0,buffer.length))>0){documentActive(token,signal);if(before.size()+count>1024*1024)throw new IOException("Phone original exceeds 1 MiB");before.write(buffer,0,count);}
                        Core.call(new JSONObject().put("op","phone_revision_check").put("record",e).put("document_id",identity[0]).put("data",Base64.encodeToString(before.toByteArray(),Base64.NO_WRAP)));
                        documentActive(token,signal);android.system.Os.lseek(fd,0,android.system.OsConstants.SEEK_SET);android.system.Os.ftruncate(fd,0);
                        byte[] replacement=e.getString("content").getBytes(StandardCharsets.UTF_8);int offset=0;
                        while(offset<replacement.length){documentActive(token,signal);count=android.system.Os.write(fd,replacement,offset,replacement.length-offset);if(count<=0)throw new IOException("Phone update stopped before all bytes were written");offset+=count;}
                        android.system.Os.fsync(fd);documentActive(token,signal);
                    }
                }catch(Exception error){try{descriptor.closeWithError(safe(error));}catch(IOException ignored){}throw error;}finally{documentFiles.remove(descriptor);descriptor.close();}
                result.put("data",Base64.encodeToString(phoneBytes(target,token,signal),Base64.NO_WRAP));
            }else throw new IOException("Unexpected phone revision phase");
            documentActive(token,signal);return result;
        }finally{documentSignals.remove(signal);}
    }
    // Platform document-provider binding. Rust owns destination/bytes/retry policy.
    private JSONObject projectPhone(JSONObject e,int token)throws Exception {
        android.os.CancellationSignal signal=new android.os.CancellationSignal();documentSignals.add(signal);
        java.util.concurrent.atomic.AtomicReference<android.os.ParcelFileDescriptor> opened=new java.util.concurrent.atomic.AtomicReference<>();
        java.util.concurrent.atomic.AtomicBoolean expired=new java.util.concurrent.atomic.AtomicBoolean();
        ScheduledFuture<?> timeout=deadlines.schedule(()->{expired.set(true);signal.cancel();android.os.ParcelFileDescriptor file=opened.get();if(file!=null)try{file.close();}catch(IOException ignored){}},e.optBoolean("chunked")?e.getInt("deadline_seconds"):120,TimeUnit.SECONDS);
        String createdName=null;TransferStore.ChunkReader stream=null;final boolean chunked=e.optBoolean("chunked");
        try{
            if(chunked)stream=documentJobs.submit(()->openTransfer(e)).get();
            documentActive(token,signal);Uri tree=Uri.parse(e.getString("tree"));String name=e.getString("name"),phase=e.getString("phase");
            if(!"content".equals(tree.getScheme())||tree.getAuthority()==null)throw new IOException("Unsupported phone folder");
            if(name.isEmpty()||name.equals(".")||name.equals("..")||name.contains("/")||name.contains("\\")||name.indexOf('%')>=0||name.indexOf(':')>=0||name.indexOf('\0')>=0||name.getBytes(StandardCharsets.UTF_8).length>255)throw new IOException("Invalid project filename");
            String mime=e.optString("mime","application/zip");if(!mime.equals("application/zip")&&!mime.equals("application/octet-stream"))throw new IOException("Unsupported snapshot content type");
            boolean write=phase.equals("write");if(!write&&!phase.equals("probe")&&!phase.equals("verify"))throw new IOException("Invalid phone transfer phase");
            boolean granted=false;for(android.content.UriPermission grant:getContentResolver().getPersistedUriPermissions())if(grant.getUri().equals(tree)&&grant.isReadPermission()&&(!write||grant.isWritePermission()))granted=true;
            if(!granted)throw new IOException("The original phone folder grant is unavailable; pending snapshot retained");
            String rootId=DocumentsContract.getTreeDocumentId(tree),match=null;Set<String> ids=new HashSet<>();
            try(Cursor c=getContentResolver().query(DocumentsContract.buildChildDocumentsUriUsingTree(tree,rootId),new String[]{DocumentsContract.Document.COLUMN_DOCUMENT_ID,DocumentsContract.Document.COLUMN_DISPLAY_NAME},null,null,null,signal)){
                if(c==null)throw new IOException("Phone folder unavailable");int count=0;
                while(c.moveToNext()){documentActive(token,signal);if(++count>5000)throw new IOException("Phone folder exceeds lookup bound");String id=c.getString(0);if(id==null||!ids.add(id))throw new IOException("Ambiguous phone document identity");if(name.equals(c.getString(1))){if(match!=null)throw new IOException("Ambiguous project filename");match=id;}}
            }
            documentActive(token,signal);Uri file;
            if(write){
                if(match!=null)throw new IOException("Project document already exists; it was not replaced. Retry will verify existing bytes");
                byte[] bytes=chunked?null:Base64.decode(e.getString("archive"),Base64.NO_WRAP);if(bytes!=null&&bytes.length>5*1024*1024)throw new IOException("Project archive exceeds 5 MiB");
                file=DocumentsContract.createDocument(getContentResolver(),DocumentsContract.buildDocumentUriUsingTree(tree,rootId),mime,name);
                if(file==null||!tree.getAuthority().equals(file.getAuthority())||ids.contains(DocumentsContract.getDocumentId(file)))throw new IOException("Provider did not return a new document; no bytes were written");
                createdName=name;
                try(Cursor c=getContentResolver().query(file,new String[]{DocumentsContract.Document.COLUMN_DISPLAY_NAME},null,null,null,signal)){if(c==null||!c.moveToFirst()||!name.equals(c.getString(0)))throw new IOException("Provider changed project filename; no bytes were written");}
                documentActive(token,signal);android.os.ParcelFileDescriptor descriptor=getContentResolver().openFileDescriptor(file,"w",signal);if(descriptor==null)throw new IOException("New project document unavailable");documentFiles.add(descriptor);opened.set(descriptor);documentActive(token,signal);
                try(OutputStream out=new android.os.ParcelFileDescriptor.AutoCloseOutputStream(descriptor)){documentActive(token,signal);if(chunked){byte[] buffer=new byte[65536];int n;while((n=stream.read(buffer))!=-1){documentActive(token,signal);out.write(buffer,0,n);}}else out.write(bytes);out.flush();}finally{opened.compareAndSet(descriptor,null);documentFiles.remove(descriptor);}
            }else{
                if(match==null)return chunked?(JSONObject)Core.call(chunkInput("finish",e.getLong("snapshot_handle")).put("found",false)):new JSONObject().put("found",false);
                file=DocumentsContract.buildDocumentUriUsingTree(tree,match);
            }
            documentActive(token,signal);android.os.ParcelFileDescriptor descriptor=getContentResolver().openFileDescriptor(file,"r",signal);if(descriptor==null)throw new IOException("Project readback unavailable");documentFiles.add(descriptor);opened.set(descriptor);documentActive(token,signal);byte[] actual;
            try(InputStream in=new android.os.ParcelFileDescriptor.AutoCloseInputStream(descriptor)){
                if(chunked){if(stream!=null)stream.close();stream=documentJobs.submit(()->openTransfer(e)).get();byte[] buffer=new byte[65536];int n;while((n=in.read(buffer))!=-1){documentActive(token,signal);byte[] expected=chunkExpected(stream,n);Core.call(chunkInput("compare",e.getLong("snapshot_handle")).put("data",Base64.encodeToString(Arrays.copyOf(buffer,n),Base64.NO_WRAP)).put("expected",Base64.encodeToString(expected,Base64.NO_WRAP)));}if(stream.read()!=-1)throw new IOException("Destination readback ended early");documentActive(token,signal);return (JSONObject)Core.call(chunkInput("finish",e.getLong("snapshot_handle")).put("found",true));}
                actual=read(in,5*1024*1024);documentActive(token,signal);
            }finally{opened.compareAndSet(descriptor,null);documentFiles.remove(descriptor);}
            return new JSONObject().put("found",true).put("data",Base64.encodeToString(actual,Base64.NO_WRAP));
        }catch(Exception error){if(expired.get())error=new IOException("Phone project transfer timed out; pending snapshot retained",error);if(createdName!=null)throw new IOException("New project document "+createdName+" may remain incomplete or unverified; existing files were not replaced. "+safe(error),error);throw error;}
        finally{if(stream!=null)try{stream.close();}catch(IOException ignored){}timeout.cancel(false);documentSignals.remove(signal);android.os.ParcelFileDescriptor file=opened.getAndSet(null);if(file!=null){documentFiles.remove(file);try{file.close();}catch(IOException ignored){}}}
    }
    private String phoneTool(JSONObject e,int token) throws Exception {
        android.os.CancellationSignal signal=new android.os.CancellationSignal();documentSignals.add(signal);String createdName=null;
        try{
            documentActive(token,signal);Uri tree=Uri.parse(e.getString("tree"));String id=DocumentsContract.getTreeDocumentId(tree);String path=e.getJSONObject("args").getString("path");
            if(path.startsWith("/")||path.contains("\\")||path.indexOf('\0')>=0)throw new IOException("Invalid phone path");
            boolean creating=e.getString("tool").equals("phone_create");String[] segments=path.split("/",-1);String fileName=creating?segments[segments.length-1]:null;
            if(creating&&(fileName.isEmpty()||fileName.equals(".")||fileName.equals("..")||fileName.getBytes(StandardCharsets.UTF_8).length>255||fileName.indexOf('%')>=0||fileName.indexOf(':')>=0))throw new IOException("Choose a plain new filename of at most 255 UTF-8 bytes");
            if(creating){boolean writable=false;for(android.content.UriPermission grant:getContentResolver().getPersistedUriPermissions())if(grant.getUri().equals(tree)&&grant.isWritePermission())writable=true;if(!writable)throw new IOException("Grant write permission to this phone folder through Android");}
            for(int index=0;index<segments.length-(creating?1:0);index++){String segment=segments[index];if(segment.isEmpty()||segment.equals("."))continue;if(segment.equals(".."))throw new IOException("Path leaves permitted folder");String next=null;Uri children=DocumentsContract.buildChildDocumentsUriUsingTree(tree,id);
                documentActive(token,signal);try(Cursor c=getContentResolver().query(children,new String[]{DocumentsContract.Document.COLUMN_DOCUMENT_ID,DocumentsContract.Document.COLUMN_DISPLAY_NAME},null,null,null,signal)){if(c==null)throw new IOException("Folder unavailable");int count=0;while(c.moveToNext()){if(++count>5000)throw new IOException("Folder traversal limit exceeded");if(segment.equals(c.getString(1))){if(next!=null)throw new IOException("Ambiguous document name");next=c.getString(0);}}}
                if(next==null)throw new IOException("Document not found: "+segment);id=next;
            }
            documentActive(token,signal);
            if(creating){String content=e.getJSONObject("args").getString("content");byte[] bytes=content.getBytes(StandardCharsets.UTF_8);if(bytes.length>1024*1024||content.indexOf('\0')>=0)throw new IOException("Phone text file must be UTF-8 and at most 1 MiB");
                Uri children=DocumentsContract.buildChildDocumentsUriUsingTree(tree,id);Set<String> existingIds=new HashSet<>();
                try(Cursor c=getContentResolver().query(children,new String[]{DocumentsContract.Document.COLUMN_DOCUMENT_ID,DocumentsContract.Document.COLUMN_DISPLAY_NAME},null,null,null,signal)){if(c==null)throw new IOException("Folder unavailable");int count=0;while(c.moveToNext()){if(++count>5000)throw new IOException("Folder exceeds lookup limit");existingIds.add(c.getString(0));if(fileName.equals(c.getString(1)))throw new IOException("File already exists; it was not replaced");}}
                documentActive(token,signal);Uri created=DocumentsContract.createDocument(getContentResolver(),DocumentsContract.buildDocumentUriUsingTree(tree,id),"text/plain",fileName);if(created==null)throw new IOException("Folder provider refused to create the file");
                if(!tree.getAuthority().equals(created.getAuthority())||existingIds.contains(DocumentsContract.getDocumentId(created)))throw new IOException("Provider did not return a new document; no bytes were written");
                createdName=fileName;documentActive(token,signal);try(Cursor c=getContentResolver().query(created,new String[]{DocumentsContract.Document.COLUMN_DISPLAY_NAME},null,null,null,signal)){if(c==null||!c.moveToFirst())throw new IOException("New document metadata unavailable");createdName=c.getString(0);}
                documentActive(token,signal);android.os.ParcelFileDescriptor descriptor=getContentResolver().openFileDescriptor(created,"w",signal);if(descriptor==null)throw new IOException("Could not open the new phone file");documentFiles.add(descriptor);try(OutputStream out=new android.os.ParcelFileDescriptor.AutoCloseOutputStream(descriptor)){documentActive(token,signal);out.write(bytes);out.flush();}finally{documentFiles.remove(descriptor);}
                documentActive(token,signal);descriptor=getContentResolver().openFileDescriptor(created,"r",signal);if(descriptor==null)throw new IOException("Could not verify the new phone file");documentFiles.add(descriptor);byte[] actual;try(InputStream in=new android.os.ParcelFileDescriptor.AutoCloseInputStream(descriptor)){documentActive(token,signal);actual=read(in,1024*1024);}finally{documentFiles.remove(descriptor);}documentActive(token,signal);
                if(!Arrays.equals(bytes,actual))throw new IOException("Phone file readback differs from the requested contents");return "Created and verified phone file "+createdName+" ("+bytes.length+" bytes).";
            }
            if(e.getString("tool").equals("phone_list")){JSONArray rows=new JSONArray();Uri children=DocumentsContract.buildChildDocumentsUriUsingTree(tree,id);try(Cursor c=getContentResolver().query(children,new String[]{DocumentsContract.Document.COLUMN_DISPLAY_NAME,DocumentsContract.Document.COLUMN_MIME_TYPE},null,null,null,signal)){if(c==null)throw new IOException("Folder unavailable");while(c.moveToNext()){if(rows.length()>=500)throw new IOException("Folder exceeds 500 entries");rows.put(new JSONObject().put("name",c.getString(0)).put("directory",DocumentsContract.Document.MIME_TYPE_DIR.equals(c.getString(1))));}}documentActive(token,signal);return rows.toString(2);}
            Uri file=DocumentsContract.buildDocumentUriUsingTree(tree,id);android.os.ParcelFileDescriptor descriptor=getContentResolver().openFileDescriptor(file,"r",signal);if(descriptor==null)throw new IOException("Document unavailable");documentFiles.add(descriptor);try(InputStream in=new android.os.ParcelFileDescriptor.AutoCloseInputStream(descriptor)){documentActive(token,signal);byte[] bytes=read(in,1024*1024);documentActive(token,signal);return StandardCharsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(bytes)).toString();}finally{documentFiles.remove(descriptor);}
        }catch(Exception error){if(createdName!=null)throw new IOException("New phone document "+createdName+" may remain incomplete or unverified. Existing documents were not replaced. "+safe(error),error);throw error;}finally{documentSignals.remove(signal);}
    }
    private void preserveInputs(){if(closed||state==null)return;try{JSONObject result=(JSONObject)Core.call(new JSONObject().put("op","event").put("action","capture").put("state",state).put("inputs",inputs()).put("root",root()));JSONObject next=result.getJSONObject("state");save(next);state=next;}catch(Throwable problem){Toast.makeText(this,"Input could not be saved: "+safe(problem),Toast.LENGTH_LONG).show();}}
    @Override public void onBackPressed(){if(canNavigateBack)dispatch("back");else super.onBackPressed();}
    @Override protected void onPause(){ratePaused=true;if(rateTick!=null)rateHandler.removeCallbacks(rateTick);preserveInputs();super.onPause();}
    @Override protected void onResume(){super.onResume();if(state!=null&&!closed)dispatch("phone_access_status");ratePaused=false;if(rateTick!=null){rateHandler.removeCallbacks(rateTick);rateHandler.post(rateTick);}}
    @Override public void onConfigurationChanged(android.content.res.Configuration configuration){super.onConfigurationChanged(configuration);viewportWidth=0;viewportHeight=0;queueViewport();}
    @Override protected void onDestroy(){closed=true;try{if(state!=null){Core.call(new JSONObject().put("op","stream_owner_cancel").put("owner",state.optString("rea_expected")));}}catch(Exception ignored){}try{if(state!=null&&state.has("rea_resume_token")&&!state.isNull("rea_resume_token"))Core.call(new JSONObject().put("op","stream_resume_close").put("token",state.get("rea_resume_token")));}catch(Exception ignored){}cleanupWorkers.shutdownNow();for(Socket socket:cleanupConnections)try{socket.close();}catch(IOException ignored){}rateHandler.removeCallbacksAndMessages(null);stopExecution();generation++;cancelDocuments();for(Socket socket:activeConnections)try{socket.close();}catch(IOException ignored){}try{Core.call(new JSONObject().put("op","cancel_tool"));}catch(Throwable ignored){}deadlines.shutdownNow();workers.shutdownNow();super.onDestroy();}
}
