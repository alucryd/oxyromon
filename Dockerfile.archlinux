FROM archlinux

WORKDIR /usr/src/oxyromon
COPY . .
RUN pacman -Sy --noconfirm base-devel cdrtools dolphin-emu mame-tools maxcso p7zip rustup yarn && \
    rustup toolchain install stable && \
    cargo install \
    --features benchmark,server \
    --root /usr \
    --path . && \
    cargo clean && \
    yarn cache clean --all && \
    rm -rf /root/.cargo /root/.rustup /tmp/* && \
    pacman -Rns --noconfirm base-devel rustup yarn && \
    pacman -Sc --noconfirm

WORKDIR /
RUN rm -rf /usr/src/oxyromon

ENV OXYROMON_DATA_DIRECTORY=/data \
    OXYROMON_ROM_DIRECTORY=/roms

VOLUME [ "/data", "/roms" ]

CMD ["oxyromon"]
