#!/bin/fish
set LAUNCHER term.sh

open_in_workspace "$LAUNCHER nats-server -js -sd ~/tmp/nats" 10
sleep 1
open_in_workspace "$LAUNCHER fish -C 'cd frontend && direnv exec . shenzi && direnv exec . cargo run'" 2
open_in_workspace "$LAUNCHER fish -C 'cd user-service && direnv exec . cargo run'" 3
open_in_workspace "$LAUNCHER fish -C 'cd log-service && direnv exec . cargo run'" 4
open_in_workspace "$LAUNCHER fish -C 'cd forum-service && direnv exec . cargo run'" 5
open_in_workspace "$LAUNCHER fish -C 'cd location-service && direnv exec . cargo run'" 6
