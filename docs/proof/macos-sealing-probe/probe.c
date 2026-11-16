// Throwaway measurement probe (not product code). macOS.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <sys/event.h>
#include <sys/sysctl.h>
#include <sys/ptrace.h>
#include <sys/stat.h>
#include <util.h>
#include <termios.h>
#include <mach/mach.h>
#include <mach/mach_vm.h>
#include <libproc.h>
#include <bsm/libbsm.h>
extern int csops(pid_t pid, unsigned int ops, void *useraddr, size_t usersize);
extern int csr_check(uint32_t mask);
#ifndef LOCAL_PEERTOKEN
#define LOCAL_PEERTOKEN 0x006
#endif
#ifndef LOCAL_PEERPID
#define LOCAL_PEERPID 0x002
#endif

static void flags(pid_t pid){
  uint32_t f=0; int r=csops(pid,0,&f,sizeof f);
  printf("  csops status pid=%d r=%d flags=0x%08x%s%s%s%s%s%s%s\n",pid,r,f,
    f&0x00000001?" VALID":"", f&0x00000100?" HARD":"", f&0x00000200?" KILL":"",
    f&0x00010000?" RUNTIME":"", f&0x04000000?" PLATFORM_BINARY":"", f&0x10000000?" DEBUGGED":"", f&0x00002000?" REQUIRE_LV":"");
  char buf[8192]={0}; r=csops(pid,7,buf,sizeof buf-1);
  int has=0; if(r==0){ for(int i=0;i<(int)sizeof buf-14;i++) if(!memcmp(buf+i,"get-task-allow",14)){has=1;break;} }
  printf("  entitlements: csops r=%d errno=%d get-task-allow-present=%d\n",r,r?errno:0,has);
  char tid[256]={0}; r=csops(pid,14,tid,sizeof tid); // CS_OPS_TEAMID
  printf("  teamid: r=%d '%s'\n",r,r==0?tid:"");
}
static void attack(pid_t pid){
  printf("attack pid=%d (uid=%d)\n",pid,getuid());
  mach_port_t t=MACH_PORT_NULL; kern_return_t kr=task_for_pid(mach_task_self(),pid,&t);
  printf("  task_for_pid: kr=%d (%s)\n",kr,mach_error_string(kr));
  if(kr==KERN_SUCCESS){
    mach_vm_address_t a=0; mach_vm_size_t sz=0; vm_region_basic_info_data_64_t info; mach_msg_type_number_t c=VM_REGION_BASIC_INFO_COUNT_64; mach_port_t on;
    kern_return_t k2=mach_vm_region(t,&a,&sz,VM_REGION_BASIC_INFO_64,(vm_region_info_t)&info,&c,&on);
    printf("  mach_vm_region: kr=%d addr=0x%llx\n",k2,(unsigned long long)a);
    vm_offset_t data; mach_msg_type_number_t cnt; k2=mach_vm_read(t,a,16,&data,&cnt);
    printf("  mach_vm_read: kr=%d (%s)\n",k2,mach_error_string(k2));
  }
  errno=0; int r=ptrace(PT_ATTACHEXC,pid,0,0);
  printf("  ptrace(PT_ATTACHEXC): r=%d errno=%d (%s)\n",r,errno,strerror(errno));
  if(r==0){ ptrace(PT_DETACH,pid,0,0);}
  flags(pid);
}
static void sockmode(void){
  char dir[128]; snprintf(dir,sizeof dir,"/tmp/lum100-%d",getpid()); mkdir(dir,0700);
  struct sockaddr_un sa={0}; sa.sun_family=AF_UNIX; snprintf(sa.sun_path,sizeof sa.sun_path,"%s/s",dir);
  int ls=socket(AF_UNIX,SOCK_STREAM,0); bind(ls,(void*)&sa,sizeof sa); listen(ls,4);
  pid_t parent=getpid();
  pid_t c1=fork();
  if(c1==0){ // "shell": connects, then forks a "child program" that inherits the fd
    int cs=socket(AF_UNIX,SOCK_STREAM,0); connect(cs,(void*)&sa,sizeof sa);
    char m[64]; snprintf(m,sizeof m,"shell-writes pid=%d\n",getpid()); write(cs,m,strlen(m));
    pid_t g=fork(); if(g==0){ snprintf(m,sizeof m,"child-writes pid=%d\n",getpid()); write(cs,m,strlen(m)); _exit(0);}
    waitpid(g,0,0); // fresh connection by the child process by path
    pid_t g2=fork(); if(g2==0){ int s2=socket(AF_UNIX,SOCK_STREAM,0); connect(s2,(void*)&sa,sizeof sa); snprintf(m,sizeof m,"child-new-connection pid=%d\n",getpid()); write(s2,m,strlen(m)); usleep(1500000); _exit(0);}
    usleep(600000); waitpid(g2,0,0); _exit(0);
  }
  printf("sock: server pid=%d shell pid=%d\n",parent,c1);
  for(int i=0;i<2;i++){
    int a=accept(ls,0,0); pid_t pp=-1; socklen_t l=sizeof pp; getsockopt(a,0,LOCAL_PEERPID,&pp,&l);
    audit_token_t tok; socklen_t tl=sizeof tok; int tr=getsockopt(a,0,LOCAL_PEERTOKEN,&tok,&tl);
    printf("  accepted conn %d: LOCAL_PEERPID=%d  LOCAL_PEERTOKEN r=%d pid=%d pidversion=%d euid=%d\n",i,pp,tr,tr==0?audit_token_to_pid(tok):-1,tr==0?audit_token_to_pidversion(tok):-1,tr==0?audit_token_to_euid(tok):-1);
    char buf[256]; usleep(100000); ssize_t n=read(a,buf,sizeof buf-1); if(n>0){buf[n]=0; printf("  data on that connection: %s",buf);}
  }
  waitpid(c1,0,0);
  int sv[2]; socketpair(AF_UNIX,SOCK_DGRAM,0,sv); pid_t pp=-1; socklen_t l=sizeof pp;
  getsockopt(sv[0],0,LOCAL_PEERPID,&pp,&l); printf("  socketpair end0 LOCAL_PEERPID=%d (creator=%d)\n",pp,getpid());
  pid_t k=fork(); if(k==0){ pid_t q=-1; socklen_t l2=sizeof q; getsockopt(sv[1],0,LOCAL_PEERPID,&q,&l2); dprintf(1,"  in forked child (pid %d): end1 LOCAL_PEERPID=%d\n",getpid(),q); _exit(0);} waitpid(k,0,0);
  unlink(sa.sun_path); rmdir(dir);
}
static void audtok(const char*tag){ audit_token_t t; mach_msg_type_number_t c=TASK_AUDIT_TOKEN_COUNT; task_info(mach_task_self(),TASK_AUDIT_TOKEN,(task_info_t)&t,&c);
  printf("  %s: pid=%d pidversion=%d\n",tag,getpid(),audit_token_to_pidversion(t)); }
static int LOGFD=1;
static void tiocsti(void){ LOGFD=dup(1);
  int m,s; int orr=openpty(&m,&s,0,0,0); dprintf(LOGFD,"openpty r=%d\n",orr); pid_t sh=fork();
  if(sh==0){ setsid(); {int rr=ioctl(s,TIOCSCTTY,0); dprintf(LOGFD,"TIOCSCTTY r=%d errno=%d\n",rr,errno);} dup2(s,0); dup2(s,1); dup2(s,2); close(m);
    pid_t g=fork(); if(g==0){ char c='X'; errno=0; int r=ioctl(0,TIOCSTI,&c); dprintf(LOGFD,"grandchild TIOCSTI r=%d errno=%d (%s)\n",r,errno,strerror(errno)); _exit(0);}
    waitpid(g,0,0); struct termios t; tcgetattr(0,&t); t.c_lflag&=~(ICANON|ECHO); tcsetattr(0,TCSANOW,&t);
    fcntl(0,F_SETFL,O_NONBLOCK); usleep(200000); char b[8]; ssize_t n=read(0,b,sizeof b); dprintf(LOGFD,"shell read %zd byte(s) from its input: '%.*s'\n",n,n>0?(int)n:0,b); _exit(0);}
  close(s); char buf[256]; usleep(1200000); fcntl(m,F_SETFL,O_NONBLOCK); ssize_t n; while((n=read(m,buf,sizeof buf))>0) fwrite(buf,1,n,stdout); waitpid(sh,0,0);
}
static void kq(void){
  pid_t c=fork(); if(c==0){ sleep(1); execl("/bin/sleep","sleep","1",(char*)0); _exit(1);}
  int k=kqueue(); struct kevent ev; EV_SET(&ev,c,EVFILT_PROC,EV_ADD|EV_ENABLE,NOTE_EXEC|NOTE_EXIT|NOTE_FORK,0,0); kevent(k,&ev,1,0,0,0);
  for(int i=0;i<3;i++){ struct kevent out; struct timespec to={4,0}; int n=kevent(k,0,0,&out,1,&to); if(n<=0){printf("  kevent timeout\n");break;} printf("  kevent pid=%d fflags:%s%s%s\n",(int)out.ident,out.fflags&NOTE_EXEC?" EXEC":"",out.fflags&NOTE_EXIT?" EXIT":"",out.fflags&NOTE_FORK?" FORK":""); if(out.fflags&NOTE_EXIT)break; }
  int st; waitpid(c,&st,0);
}
static void listenmode(const char*path){
  struct sockaddr_un sa={0}; sa.sun_family=AF_UNIX; snprintf(sa.sun_path,sizeof sa.sun_path,"%s",path); unlink(path);
  int ls=socket(AF_UNIX,SOCK_STREAM,0); bind(ls,(void*)&sa,sizeof sa); listen(ls,2);
  alarm(20); int a=accept(ls,0,0); pid_t pp=-1; socklen_t l=sizeof pp; getsockopt(a,0,LOCAL_PEERPID,&pp,&l);
  audit_token_t tok; socklen_t tl=sizeof tok; int tr=getsockopt(a,0,LOCAL_PEERTOKEN,&tok,&tl);
  char buf[128]={0}; usleep(300000); read(a,buf,sizeof buf-1);
  printf("  listen: LOCAL_PEERPID=%d token pid=%d pidversion=%d data='%s'\n",pp,tr==0?audit_token_to_pid(tok):-1,tr==0?audit_token_to_pidversion(tok):-1,buf); unlink(path);
}
int main(int argc,char**argv){
  if(argc<2){return 2;} const char*m=argv[1];
  if(!strcmp(m,"sleeper")){ sleep(900); return 0; }
  if(!strcmp(m,"info")){ printf("info: uid=%d\n",getuid()); printf("  csr_check(ALLOW_TASK_FOR_PID=0x4)=%d (0 means the weakening is ON)\n",csr_check(0x4)); printf("  csr_check(ALLOW_UNRESTRICTED_FS=0x2)=%d\n",csr_check(0x2)); int v=-1; size_t l=sizeof v; int r=sysctlbyname("kern.tfp.policy",&v,&l,0,0); printf("  kern.tfp.policy r=%d value=%d\n",r,v); char b[512]={0}; l=sizeof b; sysctlbyname("kern.bootargs",b,&l,0,0); printf("  kern.bootargs='%s'\n",b); return 0; }
  if(!strcmp(m,"attack")) { attack(atoi(argv[2])); return 0; }
  if(!strcmp(m,"flags")) { flags(atoi(argv[2])); return 0; }
  if(!strcmp(m,"sock")) { sockmode(); return 0; }
  if(!strcmp(m,"exec")) { audtok("before exec"); fflush(stdout); execl(argv[0],argv[0],"exec2",(char*)0); return 1; }
  if(!strcmp(m,"exec2")) { audtok("after exec"); return 0; }
  if(!strcmp(m,"tiocsti")) { tiocsti(); return 0; }
  if(!strcmp(m,"listen")) { listenmode(argv[2]); return 0; }
  if(!strcmp(m,"kq")) { kq(); return 0; }
  return 2;
}
