#!/bin/bash

version=$(tomlq .package.version Cargo.toml | sed 's/"//g')
sudo docker build -t alucryd/oxyromon:${version} -t alucryd/oxyromon:latest .
sudo docker push alucryd/oxyromon:${version}
sudo docker push alucryd/oxyromon:latest
