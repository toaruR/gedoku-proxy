#!/usr/bin/env python3
"""実Claude CodeセッションからHARフィクスチャを採取するための開発専用キャプチャプロキシ。

gedoku-proxy本体(src/gedoku-server)とは別物。P1-T7完了ハーネスが要求する
「実セッションのHARキャプチャ」を自作するための一時ツールで、本番のセキュリティ
設計(平文をディスクに書かない)には従わない。生キャプチャは docs/fixtures/har-raw/
(.gitignore対象)に出力し、フィクスチャ化する際は手動でスクラビングしてから
src/gedoku-jsonwalk/tests/fixtures/ 等にコピーすること。

使い方:
    CAPTURE_UPSTREAM=http://localhost:8787 python capture_proxy.py --port 18790
"""

import argparse
import json
import os
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import requests

OUT_DIR = os.path.join(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
    "docs",
    "fixtures",
    "har-raw",
)

HOP_BY_HOP = {
    "connection",
    "keep-alive",
    "transfer-encoding",
    "upgrade",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "content-length",
}


class CaptureHandler(BaseHTTPRequestHandler):
    upstream_base = "http://localhost:8787"

    def log_message(self, fmt, *args):
        print(f"[capture] {self.address_string()} - {fmt % args}")

    def _handle(self, method: str) -> None:
        content_length = int(self.headers.get("Content-Length", 0) or 0)
        req_body = self.rfile.read(content_length) if content_length else b""

        fwd_headers = {
            k: v for k, v in self.headers.items() if k.lower() not in HOP_BY_HOP and k.lower() != "host"
        }

        url = self.upstream_base.rstrip("/") + self.path
        try:
            upstream_resp = requests.request(
                method,
                url,
                headers=fwd_headers,
                data=req_body,
                stream=True,
                timeout=120,
            )
        except requests.RequestException as exc:
            self.send_response(502)
            self.end_headers()
            self.wfile.write(f"capture proxy upstream error: {exc}".encode())
            return

        self.send_response(upstream_resp.status_code)
        for k, v in upstream_resp.headers.items():
            if k.lower() in HOP_BY_HOP:
                continue
            self.send_header(k, v)
        self.end_headers()

        resp_chunks = []
        try:
            for chunk in upstream_resp.iter_content(chunk_size=4096):
                if not chunk:
                    continue
                resp_chunks.append(chunk)
                try:
                    self.wfile.write(chunk)
                except (BrokenPipeError, ConnectionAbortedError):
                    break
        finally:
            upstream_resp.close()

        self._dump(method, req_body, upstream_resp.status_code, dict(upstream_resp.headers), b"".join(resp_chunks))

    def _dump(self, method, req_body, status, resp_headers, resp_body) -> None:
        os.makedirs(OUT_DIR, exist_ok=True)
        record = {
            "captured_at": time.time(),
            "request": {
                "method": method,
                "path": self.path,
                "headers": dict(self.headers.items()),
                "body_text": _try_decode(req_body),
            },
            "response": {
                "status": status,
                "headers": resp_headers,
                "body_text": _try_decode(resp_body),
            },
        }
        fname = f"{int(time.time() * 1000)}-{method.lower()}.json"
        with open(os.path.join(OUT_DIR, fname), "w", encoding="utf-8") as f:
            json.dump(record, f, ensure_ascii=False, indent=2)
        print(f"[capture] wrote {fname} (req={len(req_body)}B resp={len(resp_body)}B status={status})")

    def do_GET(self):
        self._handle("GET")

    def do_POST(self):
        self._handle("POST")

    def do_PUT(self):
        self._handle("PUT")

    def do_DELETE(self):
        self._handle("DELETE")


def _try_decode(raw: bytes) -> str:
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError:
        return raw.decode("latin-1")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=18790)
    parser.add_argument("--upstream", default=os.environ.get("CAPTURE_UPSTREAM", "http://localhost:8787"))
    args = parser.parse_args()

    CaptureHandler.upstream_base = args.upstream
    server = ThreadingHTTPServer((args.host, args.port), CaptureHandler)
    print(f"[capture] listening on {args.host}:{args.port} -> {args.upstream}")
    print(f"[capture] output dir: {OUT_DIR}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
