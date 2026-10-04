# A JSON-RPC front that forwards eth_sendRawTransaction to the node twice, as a load balancer that
# retries on a second backend does, and returns the second answer: an error such as "already
# imported" or "nonce too low" for a transaction the node has taken. Every other request passes once.
# Usage: python3 retrying_rpc_front.py <listen port> <upstream RPC URL>
import http.server, json, sys, urllib.request

PORT, UPSTREAM = int(sys.argv[1]), sys.argv[2]


def forward(body):
    request = urllib.request.Request(UPSTREAM, data=body, headers={"Content-Type": "application/json"})
    return urllib.request.urlopen(request, timeout=30).read()


class Front(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers["Content-Length"]))
        answer = forward(body)
        try:
            if json.loads(body).get("method") == "eth_sendRawTransaction":
                sys.stderr.write("first answer: %s\n" % answer.decode())
                answer = forward(body)
                sys.stderr.write("second answer, returned: %s\n" % answer.decode())
                sys.stderr.flush()
        except ValueError:
            pass
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(answer)))
        self.end_headers()
        self.wfile.write(answer)

    def log_message(self, *args):
        pass


http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Front).serve_forever()
