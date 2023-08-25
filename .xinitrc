# Add all scripts to path
PATH="/home/pi/bus-timetables:$PATH"
timetable-server-proxy &
# We are in sh, must use test builtin
if [ -e /home/pi/kiosk-debug ]; then
  KIOSK_FLAG=""
else
  KIOSK_FLAG="--kiosk"
  autoscroll &
  xbindkeys
fi
/usr/bin/chromium-browser $KIOSK_FLAG --disable-web-security --temp-profile \
    http://localhost:5000 \
    file:///home/pi/tab3.html
