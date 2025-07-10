#!/usr/bin/env bash
TMP_FILE=$(mktemp)
cat /dev/fb0 > $TMP_FILE
sudo chvt 10
cat $TMP_FILE > /dev/fb0
rm $TMP_FILE
/home/pi/life-framebuffer/a.out &
sleep 5
kill -9 $!
sudo chvt 2
# TODO get chrome to refresh itself
xrefresh
