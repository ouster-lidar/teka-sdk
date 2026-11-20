import argparse
from http.server import HTTPServer, BaseHTTPRequestHandler

_BODY = (
    b"<!DOCTYPE html><html><head>"
    b'<meta charset="utf-8">'
    b"<title>Hello HTTP</title>"
    b"</head>"
    b"<body><main>"
    b"<h1>Hello world</h1>"
    b"<p>Python HTTP server reached on the app's direct TCP port.</p>"
    b"</main></body></html>\n"
)

class Handler(BaseHTTPRequestHandler):
    def _log_headers(self):
        for line in str(self.headers).strip().splitlines():
            print(line, flush=True)

    def do_GET(self):
        self._log_headers()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(_BODY)))
        self.end_headers()
        self.wfile.write(_BODY)

    def do_HEAD(self):
        self._log_headers()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(_BODY)))
        self.end_headers()

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Run the Hello HTTP demo server.")
    parser.add_argument("port", type=int, help="TCP port to listen on")
    args = parser.parse_args()
    HTTPServer(("0.0.0.0", args.port), Handler).serve_forever()
