"""A plain TCP forwarder: listen on 0.0.0.0:LISTEN_PORT, forward to 127.0.0.1:TARGET_PORT.

Run it inside WSL so a Windows process can reach the test server on the WSL VM's own address and
not through the WSL localhost relay, which stalls on large two-way transfers (see README.md,
"Windows"). Test-only; it has no access control and must not be left running.

    python3 forward.py LISTEN_PORT TARGET_PORT
"""
import socket,threading,sys
lp,tp=int(sys.argv[1]),int(sys.argv[2])
s=socket.socket(); s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1); s.bind(('0.0.0.0',lp)); s.listen(50)
def pipe(a,b):
    try:
        while True:
            d=a.recv(65536)
            if not d: break
            b.sendall(d)
    except Exception: pass
    for x in (a,b):
        try: x.shutdown(socket.SHUT_RDWR)
        except Exception: pass
def h(c):
    u=socket.create_connection(('127.0.0.1',tp))
    threading.Thread(target=pipe,args=(c,u),daemon=True).start(); pipe(u,c)
while True:
    c,_=s.accept(); threading.Thread(target=h,args=(c,),daemon=True).start()
