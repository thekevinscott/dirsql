FROM botbox-claude:latest

USER root

RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential curl pkg-config time \
    && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
    && chmod -R a+rwX /usr/local/rustup /usr/local/cargo
ENV PATH=/usr/local/cargo/bin:$PATH

COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /usr/local/bin/
ENV UV_PYTHON_INSTALL_DIR=/usr/local/share/uv/python \
    UV_PYTHON_BIN_DIR=/usr/local/bin \
    UV_TOOL_DIR=/usr/local/share/uv/tools \
    UV_TOOL_BIN_DIR=/usr/local/bin
RUN uv python install --default 3.13
RUN uv tool install rust-just

RUN npm install -g pnpm@11

USER user
