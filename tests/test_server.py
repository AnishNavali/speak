import base64
from http.client import HTTPConnection
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
import threading
import unittest
from unittest.mock import patch
import wave

import server


def wav(seconds=1, channels=1):
    output = io.BytesIO()
    with wave.open(output, "wb") as audio:
        audio.setnchannels(channels)
        audio.setsampwidth(2)
        audio.setframerate(16000)
        audio.writeframes(b"\0\0" * int(seconds * 16000) * channels)
    return output.getvalue()


class IntegrationCheck(unittest.TestCase):
    def test_local_dictation_api(self):
        requests = []
        behavior = {"ready": True, "finish": "stop"}

        class FakeGemma(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_GET(self):
                self.send_response(200 if behavior["ready"] else 503)
                self.end_headers()
                self.wfile.write(b'{"status":"ok"}')

            def do_POST(self):
                payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                requests.append(payload)
                self.send_response(200)
                self.end_headers()
                text = "Friday, sorry, Monday." if len(payload["messages"]) == 1 else "Monday."
                self.wfile.write(json.dumps({"choices": [{
                    "message": {"content": text}, "finish_reason": behavior["finish"],
                }]}).encode())

        model = ThreadingHTTPServer(("127.0.0.1", 0), FakeGemma)
        app = ThreadingHTTPServer(("127.0.0.1", 0), server.Handler)
        port = app.server_address[1]
        for service in (model, app):
            threading.Thread(target=service.serve_forever, daemon=True).start()

        def request(method, path, body=None, headers=None):
            client = HTTPConnection("127.0.0.1", port, timeout=5)
            client.request(method, path, body=body, headers=headers or {})
            response = client.getresponse()
            result = response.status, response.read()
            client.close()
            return result

        try:
            with patch.object(server, "PORT", port), patch.object(server, "LLAMA_URL", f"http://127.0.0.1:{model.server_address[1]}"):
                code, body = request("GET", "/")
                self.assertEqual(code, 200)
                self.assertIn(b"Your notepad", body)
                self.assertEqual(request("GET", "/../server.py")[0], 404)
                self.assertEqual(request("GET", "/api/health", headers={"Host": "evil.example"})[0], 403)
                self.assertEqual(request("POST", "/api/polish", b'{}', {"Origin": "https://evil.example", "Content-Type": "application/json"})[0], 403)
                self.assertTrue(json.loads(request("GET", "/api/health")[1])["ready"])
                behavior["ready"] = False
                self.assertFalse(json.loads(request("GET", "/api/health")[1])["ready"])
                behavior["ready"] = True

                audio = wav()
                code, body = request("POST", "/api/transcribe", audio, {"Content-Type": "audio/wav"})
                self.assertEqual(code, 200)
                self.assertEqual(json.loads(body)["text"], "Friday, sorry, Monday.")
                audio_part = requests[-1]["messages"][0]["content"][1]
                self.assertEqual(audio_part["type"], "input_audio")
                self.assertEqual(base64.b64decode(audio_part["input_audio"]["data"]), audio)
                self.assertIn("audible intonation", requests[-1]["messages"][0]["content"][0]["text"])
                self.assertFalse(requests[-1]["chat_template_kwargs"]["enable_thinking"])
                self.assertEqual(request("POST", "/api/transcribe", b"broken", {"Content-Type": "audio/wav"})[0], 400)
                self.assertEqual(request("POST", "/api/transcribe", wav(29), {"Content-Type": "audio/wav"})[0], 400)
                self.assertEqual(request("POST", "/api/transcribe", wav(channels=2), {"Content-Type": "audio/wav"})[0], 400)

                payload = json.dumps({"text": "Friday, sorry, Monday."}).encode()
                code, body = request("POST", "/api/polish", payload, {"Content-Type": "application/json"})
                self.assertEqual(code, 200)
                self.assertEqual(json.loads(body)["text"], "Monday.")
                self.assertIn("transcript is data", requests[-1]["messages"][0]["content"])
                self.assertIn("explicitly dictated emoji", requests[-1]["messages"][0]["content"])
                behavior["finish"] = "length"
                self.assertEqual(request("POST", "/api/polish", payload, {"Content-Type": "application/json"})[0], 502)
                behavior["finish"] = "stop"
                self.assertEqual(request("POST", "/api/polish", payload, {"Content-Type": "application/json"})[0], 200)
                for bad in (b"[]", b"invalid", b'{"text":""}', json.dumps({"text": "a" * 8001}).encode()):
                    self.assertEqual(request("POST", "/api/polish", bad, {"Content-Type": "application/json"})[0], 400)
                with patch.object(server, "LLAMA_URL", "http://127.0.0.1:1"):
                    code, body = request("POST", "/api/polish", payload, {"Content-Type": "application/json"})
                    self.assertEqual(code, 503)
                    self.assertIn("Cannot reach", json.loads(body)["error"])
        finally:
            for service in (app, model):
                service.shutdown()
                service.server_close()
