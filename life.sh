#!/usr/bin/env bash
TMP_FILE=$(mktemp)
cat /dev/fb0 > $TMP_FILE
/home/pi/life-framebuffer/a.out &
sleep 5
kill $!
cat $TMP_FILE > /dev/fb0
rm $TMP_FILE
