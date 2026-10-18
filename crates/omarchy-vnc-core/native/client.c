/* Narrow, worker-thread-only adapter. Protocol and authentication are LibVNCClient's. */
#include <rfb/rfbclient.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <stdint.h>

typedef struct {
    rfbClient *client;
    const char *username, *password;
    uint64_t generation;
    char *clipboard;
    int clipboard_length;
    uint8_t *cursor;
    int cursor_width,cursor_height,hot_x,hot_y;
    uint64_t cursor_generation;
} Ovnc;
static int tag;
static Ovnc *context(rfbClient *c) { return rfbClientGetClientData(c, &tag); }
static rfbBool allocate(rfbClient *c) {
    if(c->width<=0 || c->height<=0 || c->width>8192 || c->height>8192 ||
       (uint64_t)c->width*c->height>8294400 || c->format.bitsPerPixel!=32) return FALSE;
    size_t n=(size_t)c->width*c->height*4;
    uint8_t *next=calloc(1,n); if(!next) return FALSE;
    free(c->frameBuffer); c->frameBuffer=next;
    context(c)->generation++;
    return TRUE;
}
static void updated(rfbClient *c,int x,int y,int w,int h) {
    (void)x;(void)y;(void)w;(void)h; context(c)->generation++;
}
static char *password(rfbClient *c) {const char *s=context(c)->password; return s?strdup(s):NULL;}
static rfbCredential *credential(rfbClient *c,int type) {
    Ovnc *o=context(c);
    if(type!=rfbCredentialTypeUser || !o->username || !o->password) return NULL;
    rfbCredential *v=calloc(1,sizeof(*v));if(!v)return NULL;
    v->userCredential.username=strdup(o->username);v->userCredential.password=strdup(o->password);
    if(!v->userCredential.username || !v->userCredential.password) {free(v->userCredential.username);free(v->userCredential.password);free(v);return NULL;}
    return v;
}
static void clipboard(rfbClient *c,const char *text,int length) {
    Ovnc *o=context(c);if(length<0 || length>65536)return;
    char *s=malloc((size_t)length+1);if(!s)return;
    memcpy(s,text,(size_t)length);s[length]=0;free(o->clipboard);o->clipboard=s;o->clipboard_length=length;
}
static void cursor_shape(rfbClient *c,int x,int y,int w,int h,int bpp) {
    if(w<=0 || h<=0 || w>512 || h>512 || x<0 || y<0 || x>=w || y>=h || bpp!=4 || !c->rcSource || !c->rcMask)return;
    Ovnc *o=context(c);size_t size=(size_t)w*h*4;
    uint8_t *rgba=malloc(size);if(!rgba)return;
    for(size_t i=0;i<(size_t)w*h;i++) {rgba[i*4]=c->rcSource[i*4+2];rgba[i*4+1]=c->rcSource[i*4+1];rgba[i*4+2]=c->rcSource[i*4];rgba[i*4+3]=c->rcMask[i]?255:0;}
    free(o->cursor);o->cursor=rgba;o->cursor_width=w;o->cursor_height=h;o->hot_x=x;o->hot_y=y;o->cursor_generation++;
}
const uint8_t *ovnc_cursor(Ovnc *o,int *w,int *h,int *x,int *y,uint64_t *generation) {*w=o->cursor_width;*h=o->cursor_height;*x=o->hot_x;*y=o->hot_y;*generation=o->cursor_generation;return o->cursor;}
Ovnc *ovnc_new(int fd,const char *username,const char *pass) {
    Ovnc *o=calloc(1,sizeof(*o));if(!o)return NULL;
    rfbClient *c=rfbGetClient(8,3,4);if(!c){free(o);return NULL;}
    o->client=c;o->username=username;o->password=pass;c->sock=dup(fd);
    if(c->sock<0){rfbClientCleanup(c);free(o);return NULL;}
    rfbClientSetClientData(c,&tag,o);
    c->GetPassword=password;c->GetCredential=credential;c->MallocFrameBuffer=allocate;
    c->GotFrameBufferUpdate=updated;c->GotXCutText=clipboard;
    c->connectTimeout=10;c->readTimeout=2;
    c->format.bitsPerPixel=32;c->format.depth=24;c->format.bigEndian=FALSE;c->format.trueColour=TRUE;
    c->format.redMax=c->format.greenMax=c->format.blueMax=255;
    c->format.redShift=16;c->format.greenShift=8;c->format.blueShift=0;
    c->appData.shareDesktop=TRUE;c->appData.encodingsString="zrle copyrect raw";
    c->appData.useRemoteCursor=TRUE;c->GotCursorShape=cursor_shape;
    const uint32_t apple[]={rfbARD};
    const uint32_t standard[]={rfbVncAuth,rfbNoAuth};
    if(username && username[0])SetClientAuthSchemes(c,apple,1);
    else SetClientAuthSchemes(c,standard,2);
    return o;
}
int ovnc_init(Ovnc *o) {
    rfbClient *c=o->client;
    int result=InitialiseRFBConnection(c);
    o->username=o->password=NULL;
    if(!result)return 0;
    c->width=c->si.framebufferWidth;c->height=c->si.framebufferHeight;
    if(!allocate(c) || !SetFormatAndEncodings(c))return 0;
    c->updateRect.x=c->updateRect.y=0;c->updateRect.w=c->width;c->updateRect.h=c->height;
    return SendFramebufferUpdateRequest(c,0,0,c->width,c->height,FALSE);
}
int ovnc_step(Ovnc *o) {int n=WaitForMessage(o->client,10000);if(n<0)return 0;if(n>0)return HandleRFBServerMessage(o->client);return 1;}
const uint8_t *ovnc_frame(Ovnc *o,int *width,int *height,uint64_t *generation) {*width=o->client->width;*height=o->client->height;*generation=o->generation;return o->client->frameBuffer;}
int ovnc_key(Ovnc *o,uint32_t key,int down) {return SendKeyEvent(o->client,key,down);}
int ovnc_pointer(Ovnc *o,int x,int y,int buttons) {return SendPointerEvent(o->client,x,y,buttons);}
int ovnc_refresh(Ovnc *o) {rfbClient *c=o->client;return SendFramebufferUpdateRequest(c,0,0,c->width,c->height,FALSE);}
int ovnc_clipboard(Ovnc *o,char *text,int length) {return SendClientCutText(o->client,text,length);}
const char *ovnc_take_clipboard(Ovnc *o,int *length) {*length=o->clipboard_length;return o->clipboard;}
void ovnc_clear_clipboard(Ovnc *o) {free(o->clipboard);o->clipboard=NULL;o->clipboard_length=0;}
void ovnc_free(Ovnc *o) {if(o){free(o->client->frameBuffer);o->client->frameBuffer=NULL;rfbClientCleanup(o->client);free(o->clipboard);free(o->cursor);free(o);}}
static void quiet(const char *format,...) {(void)format;}
void ovnc_global_init(void) {rfbClientLog=quiet;rfbClientErr=quiet;}
