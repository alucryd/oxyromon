FROM alpine

WORKDIR /usr/src/oxyromon
COPY . .
# NOTE: Trunk's musl build runs natively here, and the wasm-bindgen/wasm-opt
# tools it downloads run without gcompat. GNU `tar` is needed by the web UI's
# fetch-webawesome.sh hook, which extracts one directory by name, something
# BusyBox tar cannot do.
RUN apk add --no-cache build-base curl musl-dev rustup tar && \
    rustup-init -y --target wasm32-unknown-unknown && \
    source /root/.cargo/env && \
    curl -fsSL https://github.com/trunk-rs/trunk/releases/download/v0.21.14/trunk-x86_64-unknown-linux-musl.tar.gz | tar -xzf- -C /usr/local/bin && \
    cargo install \
    --features server \
    --root /usr \
    --path . && \
    cargo clean && \
    rm -rf /root/.cargo /root/.rustup /root/.cache /usr/local/bin/trunk /tmp/* && \
    apk del --no-cache build-base curl musl-dev rustup tar

WORKDIR /
RUN rm -rf /usr/src/oxyromon

ENV OXYROMON_DATA_DIRECTORY=/data \
    OXYROMON_ROM_DIRECTORY=/roms

VOLUME [ "/data", "/roms" ]

CMD ["oxyromon"]
