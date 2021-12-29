FROM node:17-buster-slim AS webui

WORKDIR /webui

COPY package.json .
COPY yarn.lock .
COPY rollup.config.js .
RUN yarn install

COPY public public
COPY src/*.svelte src/
COPY src/*.js src/
RUN yarn run build

FROM rust:1.57-buster AS build

# making a new package is a hack to only build deps
RUN cargo new --bin oxyromon
WORKDIR /oxyromon
COPY Cargo.lock .
COPY Cargo.toml Cargo.toml
RUN cargo build --release

# build our app for release
COPY sqlx-data.json .
COPY migrations migrations
COPY src src
COPY data data
COPY --from=webui /webui/public public

RUN rm -r ./target/release/deps/oxyromon*
RUN cargo build --release --all-features

FROM debian:buster-slim as release
COPY --from=build /oxyromon/target/release/oxyromon .
# TODO: add chdman, maxcso
RUN apt-get update -y && apt-get install -y openssl p7zip && apt-get clean -y

ARG ROM_DIRECTORY=/roms
ARG TMP_DIRECTORY
ARG DISCARD_FLAGS
ARG DISCARD_RELEASES
ARG REGIONS_ALL
ARG REGIONS_ONE

ENTRYPOINT ["./oxyromon"]