#!/usr/bin/env python3
import http.server
import socketserver

class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-type", "text/plain")
        self.end_headers()
        self.wfile.write(b"Hello World from test server!\n")
        self.wfile.write(f"Path: {self.path}\n".encode())
        self.wfile.write(f"Headers: {dict(self.headers)}\n".encode())

if __name__ == "__main__":
    with socketserver.TCPServer(("", 8080), Handler) as httpd:
        print("Test server listening on port 8080")
        httpd.serve_forever()

