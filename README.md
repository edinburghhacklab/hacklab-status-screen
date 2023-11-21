# Setup

Clone this repo into the home directory of the pi, s.t. there is a directory
called /home/pi/bus-timetables.

Create symlinks to `.xinitrc` and `.xbindkeysrc`:

```
$ ln -s ~/bus-timetables/.xinitrc ~/.xinitrc
$ ln -s ~/bus-timetables/.xbindkeysrc ~/.xbindkeysrc
```

Add the following line to `/etc/profile` to launch the X server on startup, but
not when logging in from SSH.

```
if [[ -z $SSH_CONNECTION ]]; then xinit -- -nocursor; fi
```

# How It Works

In essence, we launch an instance of chromium with the tab bar hidden. Each tab
acts as a "screen", which renders a webpage that takes up the whole monitor when
displayed. We define macros which automatically scroll through the tabs, giving
the impression of cycling through screens, which are actually just webpages.

This way, each screen is a webpage on a tab in chrome - the system is built to
make this not too obvious.

## Step by step

At startup, the X server is run with xinit. This runs the commands in .xinitrc
from our home directory when the X server is done initializing. Because we
symlink'd ~/.xinitrc to ~/bus-timetables/.xinitrc in the setup, the .xinitrc in
this repo gets run.

The .xinitrc starts by adding the bus-timetables repository to its path, so all
of the scripts in the repository's top level are available to run.

Then it launches and forks off the Python server via `timetable-server-proxy &`
that serves the bus timetable, and if we aren't in debug mode launches
`xbindkeys` and `autoscroll`.

xbindkeys is a regular program that lets us bind mouse clicks and keyboard
events to scripts via the ~/.xbindkeysrc file, which again is symlink'd to the
file in our repo. In our case, the .xbindkeysrc file overrides the three mouse
clicks (left, right, middle) to run the three scripts we've defined in the repo:
left-click, right-click, and middle-click. These three buttons are rebound to
run the next-tab and prev-tab scripts, which tells Chrome to cycle to the next
tab or previous tab by sending it the Ctrl+tab or Ctrl+shift+tab keys
respectively.

autoscroll is a script which cycles to the next-tab once every 10 seconds, again
using the next-tab script. This keeps chrome cycling through screens.

Finally, the chrome browser is run. It is run in kiosk mode (unless debug mode
is on), which means that it is full screened and has no tab list. We also
disable web security (we assume we will only run our own websites with hand
written code and inputs) in order to access the filesystem easily.

The chrome browser is handed a list of URLs (these can be localhost, or file
urls, or anything else that it can open). Each URL will open in a separate tab,
which will act as a "screen" that we can flick through with right or left
clicking.

# Development

## Adding a new screen

To add a new screen, add a new URL to the list of URLs that chromium opens at
the end of `.xinitrc`, and make sure that URL resolves to the thing you'd like
to show.

## Debug Mode

It can be annoying when debugging a new screen to have to contend with the
automatic screen switcher, a missing cursor, and overridden right and left
click. In order to disable these, you can create the file ~/kiosk-debug, which
.xinitrc will check for.

Make sure to remove the file once you're done debugging!

# TODO

- We still need to make middle-click pause the autoscroll.
- Showing some indicator of the time to next autoscroll would be nice.
- The autoscroll should back off when the next/prev screen buttons have recently
  been clicked.
