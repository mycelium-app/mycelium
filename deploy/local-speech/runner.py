import io
import json
import os
import sys
import wave
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

import numpy
from transformers import pipeline

MODELS = os.environ.get("MYCELIUM_SPEECH_MODELS", "/var/lib/mycelium/speech-models")
LISTEN = int(os.environ.get("MYCELIUM_SPEECH_PORT", "8090"))
loaded = {}


def recogniser(folder):
    if folder not in loaded:
        loaded[folder] = pipeline("automatic-speech-recognition", model=os.path.join(MODELS, folder), trust_remote_code=True, device="cpu")

    return loaded[folder]


def samples(audio, rate):
    with wave.open(io.BytesIO(audio)) as recording:
        frames = recording.readframes(recording.getnframes())
        width = recording.getsampwidth()
        channels = recording.getnchannels()
        source_rate = recording.getframerate()

    kinds = {1: numpy.uint8, 2: numpy.int16, 4: numpy.int32}
    data = numpy.frombuffer(frames, dtype=kinds[width]).astype(numpy.float32)
    data = data.reshape(-1, channels).mean(axis=1)
    data = (data - 128.0) / 128.0 if width == 1 else data / float(2 ** (8 * width - 1))

    if source_rate != rate and len(data) > 0:
        length = int(len(data) * rate / source_rate)
        data = numpy.interp(numpy.linspace(0, len(data) - 1, length), numpy.arange(len(data)), data).astype(numpy.float32)

    return data


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.answer(200, {"status": "ok"})

    def do_POST(self):
        address = urlparse(self.path)
        folder = parse_qs(address.query).get("model", [""])[0]

        if address.path != "/transcribe" or not folder or "/" in folder or folder.startswith(".") or not os.path.isdir(os.path.join(MODELS, folder)):
            self.answer(404, {"error": "model_not_found"})

            return

        audio = self.rfile.read(int(self.headers.get("content-length", "0")))

        try:
            listen = recogniser(folder)
            rate = listen.feature_extractor.sampling_rate if listen.feature_extractor is not None else 16000
            heard = listen({"raw": samples(audio, rate), "sampling_rate": rate})
        except Exception as failure:
            print(f"transcription failed: {failure}", file=sys.stderr, flush=True)
            self.answer(500, {"error": "transcription_failed"})

            return

        self.answer(200, {"text": heard.get("text", "")})

    def answer(self, status, body):
        encoded = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


ThreadingHTTPServer(("0.0.0.0", LISTEN), Handler).serve_forever()
