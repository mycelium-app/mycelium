# syntax=docker/dockerfile:1
FROM python:3.13-slim
RUN pip install --no-cache-dir --index-url https://download.pytorch.org/whl/cpu torch==2.15.0+cpu \
 && pip install --no-cache-dir transformers==5.22.2 numpy==2.5.4 \
 && useradd -u 1000 -m mycelium
COPY runner.py /usr/local/share/mycelium/runner.py
USER mycelium
ENV MYCELIUM_SPEECH_MODELS=/var/lib/mycelium/speech-models \
    MYCELIUM_SPEECH_PORT=8090 \
    HF_HUB_OFFLINE=1
EXPOSE 8090
CMD ["python", "/usr/local/share/mycelium/runner.py"]
