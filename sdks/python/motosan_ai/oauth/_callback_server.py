from __future__ import annotations

import asyncio
import threading
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

_SUCCESS_PAGE = b"""<!doctype html>
<html><body>
<h1>Authentication complete</h1>
<p>You can close this window and return to the terminal.</p>
</body></html>
"""


@dataclass
class BoundServer:
    port: int
    _server: HTTPServer
    _thread: threading.Thread
    _result: asyncio.Future[tuple[str, str]]

    @property
    def result(self) -> asyncio.Future[tuple[str, str]]:
        return self._result

    def close(self) -> None:
        self._server.shutdown()
        self._server.server_close()
        if self._thread.is_alive():
            self._thread.join(timeout=2.0)


async def bind(port: int | None, callback_path: str) -> BoundServer:
    loop = asyncio.get_running_loop()
    result: asyncio.Future[tuple[str, str]] = loop.create_future()

    class _Handler(BaseHTTPRequestHandler):
        def log_message(self, format: str, *args: object) -> None:
            pass

        def do_GET(self) -> None:
            parsed = urlparse(self.path)
            if parsed.path != callback_path:
                self.send_response(404)
                self.end_headers()
                return
            qs = parse_qs(parsed.query)
            code = qs.get("code", [""])[0]
            state = qs.get("state", [""])[0]
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(_SUCCESS_PAGE)
            if not result.done():
                loop.call_soon_threadsafe(result.set_result, (code, state))

    server = HTTPServer(("127.0.0.1", port or 0), _Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return BoundServer(
        port=server.server_address[1], _server=server, _thread=thread, _result=result
    )


async def wait_for_callback(server: BoundServer) -> tuple[str, str]:
    try:
        return await server.result
    finally:
        server.close()
