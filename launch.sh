#!/bin/bash
LAUNCHER=term.sh

($LAUNCHER nats-server -js -sd ~/tmp/nats) &
sleep 1
# ($LAUNCHER bash -c "bash -i <<< 'cd location-service && direnv exec . cargo run;exec </dev/tty'") &
# ($LAUNCHER bash -c "bash -i <<< 'cd forum-service && direnv exec . cargo run;exec </dev/tty'") &
# ($LAUNCHER bash -c "bash -i <<< 'cd log-service && direnv exec . cargo run;exec </dev/tty'") &
# ($LAUNCHER bash -c "bash -i <<< 'cd user-service && direnv exec . cargo run;exec </dev/tty'") &
# ($LAUNCHER bash -c "bash -i <<< 'cd frontend && direnv exec . cargo run;exec </dev/tty'") &

($LAUNCHER fish -C "cd location-service && direnv exec . cargo run") &
($LAUNCHER fish -C "cd forum-service && direnv exec . cargo run") &
($LAUNCHER fish -C "cd log-service && direnv exec . cargo run") &
($LAUNCHER fish -C "cd user-service && direnv exec . cargo run") &
($LAUNCHER fish -C "cd frontend && direnv exec . cargo run") &
