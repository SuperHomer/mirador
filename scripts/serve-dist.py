"""Serves `dist` on :1420 for a debug build, which loads the frontend from
there (see CLAUDE.md, *Verifying a change*). Unlike `python3 -m http.server`
it forbids caching: WebKit otherwise keeps `index.html` from the first
launch and keeps loading that bundle after every rebuild, without asking
the server again, so a sandbox silently runs stale frontend code.

    python3 scripts/serve-dist.py dist
"""
import http.server, functools, sys
class H(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()
    def send_head(self):
        # Never answer 304: the dev bundle changes under the same names.
        for h in ("If-Modified-Since", "If-None-Match"):
            if h in self.headers: del self.headers[h]
        return super().send_head()
http.server.ThreadingHTTPServer(("", 1420), functools.partial(H, directory=sys.argv[1])).serve_forever()
