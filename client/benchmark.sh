#!/usr/bin/env bash

net-set-delay() {
  sudo tc qdisc add dev lo root netem delay "$@" #10ms 25'%'
}

net-clear() {
  sudo tc qdisc delete dev lo root
}

net-show() {
  sudo tc -s -p qdisc show dev lo
}

set-large-buffers() {
  echo '4096 16384 41943040' | sudo tee /proc/sys/net/ipv4/tcp_wmem
  echo '4096 87380 62914560' | sudo tee /proc/sys/net/ipv4/tcp_rmem
}
set-normal-buffers() {
  echo '4096 16384 4194304' | sudo tee /proc/sys/net/ipv4/tcp_wmem
  echo '4096 87380 6291456' | sudo tee /proc/sys/net/ipv4/tcp_rmem
}

root=.
binDir="$root"/target/debug

set -e
cargo build
cargo build --release
set +e
net-clear

net-set-delay 1ms
#set-large-buffers
set-normal-buffers

net-show

RUST_BACKTRACE=1 "$binDir"/benchmark "$@"
net-clear 2>/dev/null
set-normal-buffers
