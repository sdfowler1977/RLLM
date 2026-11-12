"""Turn lifetime when the app leaves the foreground: promotion, wake lock, held reply."""
import json,os,pathlib,re,subprocess,tempfile,zipfile
PROJECT=pathlib.Path(__file__).resolve().parents[1]
passed=[]
def check(name,value):assert value,name;passed.append(name)
def read(path):return (PROJECT/path).read_text()
manifest=read('AndroidManifest.xml');service=read('platform/com/local/client/TurnService.java');activity=read('platform/com/local/client/Activity.java')
check('a declared foreground service is what promotes an ordinary model turn','android:name=".TurnService"'in manifest and 'android:foregroundServiceType="dataSync"'in manifest and 'android.permission.FOREGROUND_SERVICE_DATA_SYNC'in manifest)
check('the promotion is started from the model-effect path and asks once for reply permission','turnActive=true;startRateClock(token,e.optString("model_ticket"),started);TurnService.begin(this,e.optString("model_ticket"),started,turnLease(e),"Replying");askReplyNotifications();'in activity)
check('the promotion holds a partial wake lock for the turn window and releases it at the deadline','PowerManager.PARTIAL_WAKE_LOCK,"rllm:turn"'in service and 'wake.setReferenceCounted(false);wake.acquire(lease);'in service and 'if(elapsed>=lease){stopSelf();return;}'in service)
check('the promotion names the dataSync type on the API level that requires it','startForeground(PROGRESS_ID,notification,ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC);'in service)
check('the promotion is released when the task is removed or the service is destroyed','public void onTaskRemoved(Intent root){stopSelf();}'in service and service.count('stopForeground(STOP_FOREGROUND_REMOVE)')==2)
check('the held window is clamped to the turn deadline in both the fresh and extended paths',service.count('Math.max(MIN_LEASE,Math.min(MAX_LEASE,window))')==2 and 'MIN_LEASE=30000L'in service and 'MAX_LEASE=600000L'in service)
check('the promotion window comes from the turn request timeout rather than a fixed guess','private long turnLease(JSONObject effect){return Math.max(30000L,Math.min(600000L,effect.optLong("timeout_ms",120000L)));}'in activity)
lease_line=next((line for line in activity.splitlines()if'private long turnLease'in line),'')
transport_line=next((line for line in activity.splitlines()if'transportDeadline=command.optLong'in line),'')
check('the promotion window and the request transport window share one default and one cap so the wake lock can neither outlive nor strand the turn','120000'in lease_line and '600000'in lease_line and '120000'in transport_line and '600000'in transport_line)
core=[path.read_text()for path in sorted((PROJECT/'core').glob('*.rs'))]
carriers=[line.strip()for text in core for line in text.splitlines()if'timeout_ms'in line]
service_pair='"purpose",s(&ticket)'
scaled='(timeout*1000)'
model_pair='put(&mut effect, "purpose", s("model"));'
joined='\n'.join(core)
check('only the service path carries a per-request timeout, so a model turn is promoted on the shared default rather than a guess',len(carriers)==1 and service_pair in carriers[0] and scaled in carriers[0] and model_pair in joined)
check('a chained model effect re-extends the live promotion instead of starting a second one','if(service!=null&&service.active){service.extend(ticket,started,lease,label);return;}'in service and 'if(active){extend(value,from,window,text);return START_NOT_STICKY;}'in service)
check('the rate clock re-arms itself while the app is backgrounded so elapsed time keeps advancing',re.search(r'\n\s*rateHandler\.postDelayed\(this,ratePaused\?1000:500\);\n\s*\}\};',activity)and 'if(!ratePaused&&(state.optString("page").equals("chat")||state.optString("page").equals("diagnostics")))try{dispatch("rate_tick"'in activity)
check('the first frame after a resume re-reads the true elapsed time instead of a stale snapshot','private void refreshRateClock(){'in activity and 'if(!rateTicket.equals(stateText(state,"model_expected")))return;'in activity and 'ratePaused=false;media.resume();super.onResume();'in activity)
check('a turn is only settled when the core reports nothing else pending','if(!stateText(next,"model_expected").isEmpty())return;'in activity and 'if(planned!=null)return;'in activity and 'if(next.optJSONObject("pending")!=null)return;'in activity and 'JSONArray stack=next.optJSONArray("delegation_stack");if(stack!=null&&stack.length()>0)return;'in activity)
check('the promotion is released on every turn-ending path',activity.count('TurnService.finish()')>=4)
check('a reply that lands while backgrounded is held and then drained when notifications are allowed','if(!ratePaused)return;\n        pendingReplyTitle=title;pendingReplyBody=body.trim();pendingReplyChat=conversation;\n        publishPendingReply();'in activity and 'if(!TurnService.notificationsAllowed(this))return;'in activity and 'TurnService.post(this,title,body,chat);'in activity)
check('the held reply is drained on resume and after the permission result arrives','if(code==56)publishPendingReply();'in activity and 'refreshRateClock();openRequestedConversation();'in activity)
check('a reply tap names the conversation it belongs to and reopening waits for the turn to end','static final String EXTRA_CHAT="rllm.conversation";'in service and 'String id=intent.getStringExtra(TurnService.EXTRA_CHAT);'in activity and 'if(requestedConversation.isEmpty()||closed||state==null||ratePaused)return;'in activity and 'if(turnActive)return;'in activity and 'requestConversation(intent);'in activity)
check('a refused background start or refused notification cannot fail the committed turn',service.count('catch(RuntimeException refused)')+service.count('catch(RuntimeException retired)')>=5 and 'catch(RuntimeException unavailable){/* A refused notification never blocks the committed reply. */}'in service)
def subject():
    configured=os.environ.get('RLLM_CHECK_APK')
    if configured:return pathlib.Path(configured).resolve()
    record=json.loads((PROJECT/'PUBLICATION-MANIFEST.json').read_text())['signed_apk']
    name=pathlib.PurePosixPath(record['path']).name
    if record.get('location')!='companion':return (PROJECT/'release/RLLM.apk').resolve()
    for candidate in [PROJECT/name,*sorted((PROJECT/'output').glob('*/'+name))]:
        if candidate.exists():return candidate.resolve()
    raise SystemExit(f'refusing to invent an artifact: PUBLICATION-MANIFEST.json delivers {name} as a companion file that is not on this machine; set RLLM_CHECK_APK to the APK under test')
apk=subject()
tools=pathlib.Path(os.environ['CLIENT_TOOLS'])
with tempfile.TemporaryDirectory(prefix='turn-background-',dir=PROJECT/'output')as tmp:
    root=pathlib.Path(tmp)
    tree=subprocess.run([str(tools/'aapt2'),'dump','xmltree',str(apk),'--file','AndroidManifest.xml'],capture_output=True,text=True)
    check('the real linker reads the packaged application manifest',tree.returncode==0)
    xml=tree.stdout
    at=xml.find('name(0x01010003)="com.local.client.TurnService"')
    check('the packaged application declares the turn service that outlives focus',at>=0)
    block='\n'.join(xml[at:].splitlines()[:5])if at>=0 else ''
    promoted=re.search(r'foregroundServiceType\(0x01010599\)=0x([0-9a-f]+)',block)
    check('the packaged turn service is promoted as dataSync',bool(promoted)and promoted.group(1)=='00000001')
    check('the packaged permission set carries the dataSync foreground permission','android.permission.FOREGROUND_SERVICE_DATA_SYNC'in xml and xml.count('E: uses-permission')==15)
    check('the packaged service set keeps the three prior services beside the turn service',xml.count('E: service ')==4 and all(name in xml for name in ('com.local.client.VoiceSession','com.local.client.Execution','com.local.client.Gateway')))
    with zipfile.ZipFile(apk)as archive:names=[entry for entry in archive.namelist()if entry.startswith('classes')and entry.endswith('.dex')]
    check('the packaged application carries its dex payloads',names!=[])
    blob=b'';dump=''
    for name in names:
        with zipfile.ZipFile(apk)as archive:payload=archive.read(name)
        blob+=payload;(root/name).write_bytes(payload)
        read_out=subprocess.run([str(tools/'dexdump'),'-f',str(root/name)],capture_output=True,text=True)
        check('dexdump reads '+name,read_out.returncode==0);dump+=read_out.stdout
    def methods(cls):
        found=set()
        for section in dump.split('Class descriptor'):
            head=re.match(r"\s*:\s*'L([^';]+);'",section)
            if head and head.group(1)=='com/local/client/'+cls:found|=set(re.findall(r"name\s*:\s*'([^']+)'",section))
        return found
    check('the packaged dex carries the promotion surface including its lifetime callbacks',{'begin','finish','post','notificationsAllowed','onStartCommand','onTaskRemoved','onDestroy','extend','publish','progress'}<=methods('TurnService'))
    check('the packaged dex carries the turn-continuity surface on the activity',{'turnSettled','publishReply','publishPendingReply','turnLease','askReplyNotifications','refreshRateClock','requestConversation','openRequestedConversation','startRateClock','replyText'}<=methods('Activity'))
    check('the wake-lock tag and reply conversation key survive into the packaged dex',b'rllm:turn'in blob and b'rllm.conversation'in blob)
report=dict(passed=len(passed),checks=passed,scope='The turn-promotion contract read from the real manifest, the real Activity and service sources, and read back out of the packaged APK with the real linker and dexdump: declared promotion type, permission count, wake-lock window, held-reply publication and the tap-to-conversation route. No device is attached, so the platform behaviour of a refused background start, the per-app foreground budget and a real notification delivery are asserted as source contract only, not observed on hardware.')
(PROJECT/'output/checks').mkdir(parents=True,exist_ok=True);(PROJECT/'output/checks/turn_background.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
