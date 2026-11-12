package com.local.client;

import android.content.ContentResolver;
import android.content.Intent;
import android.content.UriPermission;
import android.net.Uri;
import android.provider.DocumentsContract;
import org.json.JSONArray;
import java.io.IOException;
import java.util.List;

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
        // These Android authorities expose local phone files. Every other content
        // source uses the provider policy, including providers unknown to this app.
        return !"com.android.externalstorage.documents".equals(authority)
            &&!"com.android.providers.downloads.documents".equals(authority)
            &&!"com.android.providers.media.documents".equals(authority)
            &&!"media".equals(authority);
    }

    static void check(Uri uri,boolean phone,boolean providers,boolean selected)throws IOException {
        if(!valid(uri))throw new IOException("Select a valid document from Android file locations");
        if((selected||provider(uri))&&!providers)throw new IOException("Document provider access is off in Setup");
        if(!provider(uri)&&!phone)throw new IOException("Phone storage access is off in Setup");
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
