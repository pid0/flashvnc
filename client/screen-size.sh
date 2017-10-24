#!/usr/bin/env bash

xwininfo -root | grep "$1" | sed -r -e 's/[^0-9]*([0-9]+).*/\1/'
