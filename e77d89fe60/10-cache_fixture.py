"""Local-only fixture: prime legacy asset URLs, then proxy the fixed server.
Do not use against production. No private data or external writes.
"""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
LEGACY = Path('/home/luluco/code/DabneyMoeWebsite-community-activity/target/site/pkg')

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        data = self.rfile.read(int(self.headers.get('Content-Length', '0')))
        request = Request('http://127.0.0.1:3049' + self.path, data=data, headers={'Content-Type': self.headers.get('Content-Type', '')}, method='POST')
        with urlopen(request) as response:
            body = response.read()
            self.send_response(response.status)
            for name, value in response.headers.items():
                if name.lower() not in ['transfer-encoding', 'connection', 'content-length']:
                    self.send_header(name,value)
        self.send_header('Content-Length',str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == '/prime':
            body = b'''<!doctype html><title>Legacy asset cache fixture</title><h1>Prime legacy hydration assets</h1><script>
Promise.all(['/pkg/dabney.js','/pkg/dabney.wasm'].map(async url => {
  const response = await fetch(url); const data = await response.arrayBuffer();
  return {url, bytes:data.byteLength};
})).then(result => {window.primed = result; document.body.append(JSON.stringify(result));});
</script>'''
            self.send_response(200)
            self.send_header('Content-Type','text/html')
            self.send_header('Cache-Control','no-store')
        elif self.path in ['/pkg/dabney.js','/pkg/dabney.wasm']:
            body = (LEGACY / self.path.split('/')[-1]).read_bytes()
            self.send_response(200)
            self.send_header('Content-Type','application/wasm' if self.path.endswith('.wasm') else 'text/javascript')
            self.send_header('Cache-Control','public, max-age=31536000, immutable')
            self.send_header('Last-Modified','Thu, 01 Jan 1970 00:00:01 GMT')
        else:
            with urlopen('http://127.0.0.1:3049' + self.path) as response:
                body = response.read()
                self.send_response(response.status)
                for name, value in response.headers.items():
                    if name.lower() not in ['transfer-encoding','connection','content-length']:
                        self.send_header(name,value)
        self.send_header('Content-Length',str(len(body)))
        self.end_headers()
        self.wfile.write(body)

ThreadingHTTPServer(('127.0.0.1',3050), Handler).serve_forever()
