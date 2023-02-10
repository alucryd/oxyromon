#!/bin/bash

version=$(tomlq .package.version Cargo.toml)
sudo docker build -t alucryd/oxyromon:"${version//\"/}" -t alucryd/oxyromon:latest -f Dockerfile.archlinux .
sudo docker build -t alucryd/oxyromon:"${version//\"/}"-alpine -t alucryd/oxyromon:alpine -f Dockerfile.alpine .
sudo docker push alucryd/oxyromon:"${version//\"/}" alucryd/oxyromon:latest alucryd/oxyromon:"${version//\"/}"-alpine alucryd/oxyromon:alpine
