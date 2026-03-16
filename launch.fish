#!/bin/fish
set LAUNCHER term.sh

open_in_workspace "$LAUNCHER nats-server -V -js -sd ~/tmp/nats" 10
sleep 2
open_in_workspace "$LAUNCHER -T 'emailer-service' fish -C 'cd emailer-service && direnv exec . cargo run'" 7
sleep 2
open_in_workspace "$LAUNCHER -T 'location-service' fish -C 'cd location-service && direnv exec . cargo run'" 6
sleep 2
open_in_workspace "$LAUNCHER -T 'forum-service' fish -C 'cd forum-service && direnv exec . cargo run'" 5
sleep 2
open_in_workspace "$LAUNCHER -T 'log-service' fish -C 'cd log-service && direnv exec . cargo run'" 4
sleep 2
open_in_workspace "$LAUNCHER -T 'user-service' fish -C 'cd user-service && direnv exec . cargo run'" 3
sleep 2
open_in_workspace "$LAUNCHER -T 'frontend' fish -C 'cd frontend && direnv exec . shenzi && direnv exec . cargo run'" 2




