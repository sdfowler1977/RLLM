#include <jni.h>
#include <stdlib.h>
#include <string.h>
extern char *client_process(const unsigned char *, size_t);
extern void client_free(char *);
JNIEXPORT jbyteArray JNICALL Java_com_local_client_Core_process(JNIEnv *env, jclass clazz, jbyteArray input) {
    (void)clazz;
    if (!input) return NULL;
    jsize n=(*env)->GetArrayLength(env,input);
    if (n>8*1024*1024) return NULL;
    unsigned char *bytes=malloc(n ? (size_t)n : 1);
    if (!bytes) return NULL;
    (*env)->GetByteArrayRegion(env,input,0,n,(jbyte*)bytes);
    if ((*env)->ExceptionCheck(env)) {free(bytes);return NULL;}
    char *result=client_process(bytes,(size_t)n);
    free(bytes);
    if (!result) return NULL;
    size_t size=strlen(result);
    jbyteArray output=(*env)->NewByteArray(env,(jsize)size);
    if(output) (*env)->SetByteArrayRegion(env,output,0,(jsize)size,(jbyte*)result);
    client_free(result);
    return output;
}
