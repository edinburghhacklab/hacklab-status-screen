# Add all scripts to path
PATH="/home/pi/bus-timetables:$PATH"
timetable-server-proxy &
(/home/pi/read-gamepad/a.out | tee /home/pi/read-gamepad-output | prune-erroneous-output 2>/home/pi/hello2 | bash) &
# We are in sh, must use test builtin
if [ -e /home/pi/kiosk-debug ]; then
  KIOSK_FLAG=""
else
  KIOSK_FLAG="--kiosk"
  autoscroll &
  xbindkeys
fi

# Start xkcd webserver
python3 /home/pi/bus-timetables/xkcd-status/main.py &

/usr/bin/chromium-browser $KIOSK_FLAG --disable-web-security --temp-profile \
    http://localhost:5000 \
    file:///home/pi/bus-timetables/voron-status/index.html \
    http://trains.hacklab \
    http://localhost:2578 \
    http://carbon.hacklab:8448
#   file:///home/pi/tab3.html
#   file:///home/pi/voron-status/index.html
