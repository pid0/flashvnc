#!/usr/bin/env bash

#TODO rewrite in Rust

vncserver -list | sed -r -n "/:[0-9]+/p" | \
    sort -k1h | tail -1 | cut -f 1 | sed -r 's/://'
