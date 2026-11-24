"""Local servers for the network tests of toa (docs/lines/tools.md 9.5).

  serve.py git --root DIR --cert CERT --key KEY --port N
      git's smart HTTP (`git http-backend`) over HTTPS, for the repositories under DIR
  serve.py proxy --port N --log FILE
      an HTTP proxy that only does CONNECT, writing one line per connection to FILE

Only the standard library; for tests on the machine itself, never for anything else.
"""

import argparse
import http.server
import os
import select
import socket
import socketserver
import ssl
import subprocess
import sys
import threading


class Git(http.server.BaseHTTPRequestHandler):
    root = "."

    def log_message(self, fmt, *args):
        sys.stderr.write("git-http: " + (fmt % args) + "\n")

    def body(self):
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            out = b""
            while True:
                size = int(self.rfile.readline().split(b";")[0].strip() or b"0", 16)
                if size == 0:
                    self.rfile.readline()
                    return out
                out += self.rfile.read(size)
                self.rfile.readline()
        n = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(n) if n else b""

    def do_GET(self):
        self.cgi()

    def do_POST(self):
        self.cgi()

    def cgi(self):
        path, _, query = self.path.partition("?")
        body = self.body()
        env = dict(os.environ)
        env.update(
            GIT_PROJECT_ROOT=self.root,
            GIT_HTTP_EXPORT_ALL="1",
            PATH_INFO=path,
            QUERY_STRING=query,
            REQUEST_METHOD=self.command,
            CONTENT_TYPE=self.headers.get("Content-Type", ""),
            CONTENT_LENGTH=str(len(body)),
            HTTP_CONTENT_ENCODING=self.headers.get("Content-Encoding", ""),
            HTTP_GIT_PROTOCOL=self.headers.get("Git-Protocol", ""),
            REMOTE_ADDR=self.client_address[0],
        )
        p = subprocess.run(["git", "http-backend"], input=body, env=env, capture_output=True)
        out = p.stdout
        sep = b"\r\n\r\n" if b"\r\n\r\n" in out else b"\n\n"
        head, _, rest = out.partition(sep)
        status, headers = 200, []
        for line in head.splitlines():
            k, _, v = line.decode("latin-1").partition(":")
            if k.lower() == "status":
                status = int(v.strip().split()[0])
            elif k:
                headers.append((k, v.strip()))
        self.send_response(status)
        for k, v in headers:
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(rest)))
        self.end_headers()
        self.wfile.write(rest)


class Proxy(socketserver.StreamRequestHandler):
    log = None

    def handle(self):
        line = self.rfile.readline().decode("latin-1").strip()
        while self.rfile.readline() not in (b"\r\n", b"\n", b""):
            pass
        method, target, _ = (line.split(" ") + ["", "", ""])[:3]
        if method != "CONNECT":
            self.wfile.write(b"HTTP/1.1 405 Only CONNECT\r\n\r\n")
            return
        with open(self.log, "a") as f:
            f.write(f"CONNECT {target}\n")
        host, _, port = target.rpartition(":")
        upstream = socket.create_connection((host, int(port)))
        self.wfile.write(b"HTTP/1.1 200 Connection established\r\n\r\n")
        self.wfile.flush()
        a, b = self.connection, upstream
        try:
            while True:
                r, _, _ = select.select([a, b], [], [], 60)
                if not r:
                    return
                for s in r:
                    data = s.recv(65536)
                    if not data:
                        return
                    (b if s is a else a).sendall(data)
        finally:
            upstream.close()


class ThreadingTCP(socketserver.ThreadingMixIn, socketserver.TCPServer):
    allow_reuse_address = True
    daemon_threads = True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("what", choices=["git", "proxy"])
    ap.add_argument("--port", type=int, required=True)
    ap.add_argument("--root")
    ap.add_argument("--cert")
    ap.add_argument("--key")
    ap.add_argument("--log")
    a = ap.parse_args()
    if a.what == "git":
        Git.root = os.path.abspath(a.root)
        server = http.server.ThreadingHTTPServer(("127.0.0.1", a.port), Git)
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(a.cert, a.key)
        server.socket = ctx.wrap_socket(server.socket, server_side=True)
    else:
        Proxy.log = os.path.abspath(a.log)
        open(Proxy.log, "a").close()
        server = ThreadingTCP(("127.0.0.1", a.port), Proxy)
    print(f"{a.what} listening on {a.port}", flush=True)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    threading.Event().wait()


if __name__ == "__main__":
    main()
