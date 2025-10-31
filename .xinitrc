. "$HOME/.cargo/env"

cd /home/pi/bus-timetables || exit 1

./timetable-server-proxy &

sleep 1

# Start xkcd webserver
(cd xkcd-status && python3 main.py) &

sleep 1

(. /home/pi/.bashrc; cd /home/pi/hacklab-timers/hacklab-timers-main; /home/pi/.nvm/versions/node/v22.14.0/bin/node index.js ../timers ../logs >diagnostics 2>&1) &

sleep 1

# Edit config.toml
cargo run -r -- -k -vvvvvv
