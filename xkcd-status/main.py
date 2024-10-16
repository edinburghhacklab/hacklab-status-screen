from http.server import BaseHTTPRequestHandler, HTTPServer
from requests import get
import io

hostName = "localhost"
serverPort = 2578

class WebServer(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path in "/":
            site: str
            with open("index.html") as f:
                site = f.read()

            self.send_response(200)
            self.send_header("Content-type", "text/html")
            self.end_headers()
            self.wfile.write(bytes(site, "utf-8"))
        elif self.path == "/api":
            obj = get("https://xkcd.com/info.0.json").text

            self.send_response(200)
            self.send_header("Content-type", "application/json")
            self.end_headers()
            self.wfile.write(bytes(obj, "utf-8"))

if __name__ == "__main__":
    webServer = HTTPServer((hostName, serverPort), WebServer)
    print("Server started http://%s:%s" % (hostName, serverPort))

    try:
        webServer.serve_forever()
    except KeyboardInterrupt:
        pass

    webServer.server_close()
    print("Server stopped.")
