# syntax=docker/dockerfile:1
ARG NODE_VERSION=22
ARG CLAUDE_AGENT_ACP_VERSION=0.99.0
ARG CODEX_ACP_VERSION=2.3.0

FROM node:${NODE_VERSION}-bookworm-slim
ARG CLAUDE_AGENT_ACP_VERSION
ARG CODEX_ACP_VERSION
RUN apt-get update \
 && apt-get install -y --no-install-recommends git ca-certificates openssh-client \
 && rm -rf /var/lib/apt/lists/* \
 && npm install --global --omit=dev \
    @agentclientprotocol/claude-agent-acp@${CLAUDE_AGENT_ACP_VERSION} \
    @agentclientprotocol/codex-acp@${CODEX_ACP_VERSION} \
 && npm cache clean --force \
 && usermod -l agent -d /home/agent -m node \
 && groupmod -n agent node \
 && mkdir -p /workspace \
 && chown agent:agent /workspace
USER agent
ENV HOME=/home/agent
WORKDIR /workspace
