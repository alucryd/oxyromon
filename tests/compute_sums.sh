#!/bin/sh

for f in "$*"; do
    ls -l "$f"
    rhash -C --simple "$f"
    md5sum "$f"
    sha1sum "$f"
done
