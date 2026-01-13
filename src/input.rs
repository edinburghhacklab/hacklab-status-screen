/*
 * Copyright 2025  Simon Arlott
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

use core::time;
use enum_dispatch::enum_dispatch;
use rumqttc::{Event, Incoming, MqttOptions, QoS};
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{fmt, thread};

use anyhow::Error;
use evdev::{EventType, InputEvent};
use log::{debug, error, info, trace, warn};

use crate::config::Config;
use crate::output::{Browser, TimeSinceLast};

#[derive(Debug)]
struct Device {
	name: String,
	path: PathBuf,
	handler: Handlers,
}

#[derive(Debug)]
pub struct Input {
	main: Arc<Device>,
	tabs: Option<Arc<Device>>,
	timers: Option<Arc<Device>>,
	_idle: Arc<Idle>,
}

#[derive(Debug)]
enum Direction {
	Up,
	Down,
	Left,
	Right,
}

#[enum_dispatch]
trait Handler {
	fn button_press(&self, id: u16);
	fn dpad_press(&self, dir: Direction);
}

#[enum_dispatch(Handler)]
#[derive(strum::AsRefStr)]
enum Handlers {
	Navigation,
	Tabs,
	Timers,
}

#[derive(Debug)]
struct Navigation {
	browser: Arc<Browser>,
	konami: Konami,
}

#[derive(Debug)]
struct Tabs {
	browser: Arc<Browser>,
	config: Arc<Config>,
	run: Arc<Mutex<Arc<Browser>>>,
}

#[derive(Debug)]
struct Timers {
	browser: Arc<Browser>,
	config: Arc<Config>,
	time_since_last: Arc<TimeSinceLast>,
}

#[derive(Debug)]
struct Konami {
	config: Arc<Config>,
	history: Mutex<VecDeque<char>>,
	run: Arc<Mutex<Arc<Browser>>>,
}

#[derive(Debug)]
struct Clip {
    //vid_dir: String,
    //playing: bool,
}

#[derive(derive_more::Debug)]
struct Idle {
	#[debug("{:?}", _client.is_some())]
	_client: Option<rumqttc::Client>,
}

fn execute(run: Arc<Mutex<Arc<Browser>>>, command: &str) {
	let command = command.to_owned();

	run.lock().unwrap().user_activity();

	thread::spawn(move || {
		/* Don't run multiple commands concurrently */
		let run = run.lock().unwrap();

		run.user_activity();

		info!("Execute: {command}");

		/* Wait for command to finish */
		if let Err(err) = Command::new("sh").arg("-c").arg(&command).output() {
			error!("Error executing command {command:?}: {err}");
		}
	});
}


impl Input {
	pub fn new(
		config: Arc<Config>,
		browser: Arc<Browser>,
		time_since_last: Arc<TimeSinceLast>,
	) -> Result<Self, Error> {
		let run = Arc::new(Mutex::new(browser.clone()));

		Ok(Self {
			_idle: Idle::new(&config, browser.clone(), run.clone()),
			main: Device::new(
				"main",
				PathBuf::from(config.keyboard_device("main")?),
				Handlers::from(Navigation::new(
					browser.clone(),
					config.clone(),
					run.clone(),
				)),
			),
			tabs: Device::new_optional(
				"tabs",
				config.keyboard_device("tabs").map(PathBuf::from),
				Handlers::from(Tabs::new(browser.clone(), config.clone(), run)),
			),
			timers: Device::new_optional(
				"timers",
				config.keyboard_device("timers").map(PathBuf::from),
				Handlers::from(Timers::new(browser, config, time_since_last.clone())),
			),
		})
	}

	pub fn start(&self) {
		self.main.start();
		if let Some(tabs) = &self.tabs {
			tabs.start();
		}
		if let Some(timers) = &self.timers {
			timers.start();
		}
	}
}

impl Device {
	pub fn new<P: AsRef<Path>>(name: &str, path: P, handler: Handlers) -> Arc<Self> {
		Arc::new(Self {
			name: name.to_owned(),
			path: path.as_ref().to_path_buf(),
			handler,
		})
	}

	pub fn new_optional<P: AsRef<Path>>(
		name: &str,
		path: Result<P, Error>,
		handler: Handlers,
	) -> Option<Arc<Self>> {
		match path {
			Ok(path) => Some(Self::new(name, path, handler)),
			Err(err) => {
				warn!("Keyboard {name} not found: {err}");
				None
			}
		}
	}

	pub fn start(self: &Arc<Self>) {
		let self_copy = self.clone();

		thread::spawn(move || self_copy.run());
	}

	fn run(&self) {
		loop {
			match evdev::Device::open(&self.path) {
				Ok(mut device) => {
					info!("[{}] Opened device {:?}", self.name, self.path.display());
					if let Err(err) = self.read_events(&mut device) {
						error!("[{}] Error reading events: {err}", self.name);
					}
				}
				Err(err) => error!(
					"[{}] Unable to open device {:?}: {err}",
					self.name,
					self.path.display()
				),
			}
			thread::sleep(time::Duration::from_secs(1));
		}
	}

	fn read_events(&self, device: &mut evdev::Device) -> Result<(), Error> {
		loop {
			for event in device.fetch_events()? {
				self.handle_event(&event);
			}
		}
	}

	fn handle_event(&self, event: &InputEvent) {
		match event.event_type() {
			EventType::KEY => {
				if event.value() == 1 {
					match event.code() {
						code @ 288..=303 => self.button_press(code - 288),
						code @ 704..=712 => self.button_press(code - 704 + 16),
						_ => {}
					}
				}
			}
			EventType::ABSOLUTE => match event.code() {
				0 => {
					/* X axis */
					match event.value().cmp(&127) {
						Ordering::Less => {
							self.dpad_press(Direction::Left);
						}
						Ordering::Greater => {
							self.dpad_press(Direction::Right);
						}
						_ => {}
					}
				}
				1 => {
					/* Y axis */
					match event.value().cmp(&127) {
						Ordering::Less => self.dpad_press(Direction::Up),
						Ordering::Greater => {
							self.dpad_press(Direction::Down);
						}
						_ => {}
					}
				}
				_ => {}
			},
			_ => {}
		}
	}

	fn button_press(&self, id: u16) {
		debug!("[{}] Button pressed: {id}", self.name);
		self.handler.button_press(id);
	}

	fn dpad_press(&self, dir: Direction) {
		debug!("[{}] D-pad pressed: {dir:?}", self.name);
		self.handler.dpad_press(dir);
	}
}

impl fmt::Debug for Handlers {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(self.as_ref())
	}
}

impl Konami {
	const KONAMI_CODE: &str = "UUDDLRLRBAS";

	pub fn new(config: Arc<Config>, run: Arc<Mutex<Arc<Browser>>>) -> Self {
		Self {
			config,
			history: Mutex::new(VecDeque::new()),
			run,
		}
	}

	fn add(&self, key: char) -> Option<bool> {
		let mut history = self.history.lock().unwrap();

		history.push_back(key);

		while history.len() > Self::KONAMI_CODE.len() {
			history.pop_front();
		}

		if history.iter().collect::<String>() == Self::KONAMI_CODE[0..history.len()] {
			debug!("Konami code {}/{}", history.len(), Self::KONAMI_CODE.len());

			if history.len() == Self::KONAMI_CODE.len() {
				history.clear();
				self.entered();
				Some(true)
			} else {
				None
			}
		} else {
			while history.len() > 1 {
				history.pop_front();
			}
			Some(false)
		}
	}

	fn entered(&self) {
		info!("Konami code entered");

		if let Ok(command) = self.config.konami_command() {
			execute(self.run.clone(), &command);
		}
	}
}

impl Navigation {
	fn new(browser: Arc<Browser>, config: Arc<Config>, run: Arc<Mutex<Arc<Browser>>>) -> Self {
		Self {
			browser,
			konami: Konami::new(config, run),
		}
	}
}

impl Handler for Navigation {
	fn button_press(&self, id: u16) {
		match self.konami.add(match id {
			0 => 'X',
			1 => 'A',
			2 => 'B',
			3 => 'Y',
			9 => 'S',
			_ => '*',
		}) {
			Some(false) => match id {
				0 => self.browser.user_press("x"),     /* X */
				1 => self.browser.user_press("a"),     /* A */
				2 => self.browser.user_press("b"),     /* B */
				3 => self.browser.user_press("y"),     /* Y */
				4 => self.browser.goto_previous_tab(), /* left bumper */
				5 => self.browser.goto_next_tab(),     /* right bumper */
				8 => self.browser.reload_tab(),        /* select */
				9 => self.browser.pause(),             /* start */
				_ => {}
			},
			None => { /* Ignore button presses when konami code is being entered */ }
			Some(true) => { /* Konami code entered */ }
		};
	}

	fn dpad_press(&self, dir: Direction) {
		self.konami.add(match dir {
			Direction::Up => 'U',
			Direction::Down => 'D',
			Direction::Left => 'L',
			Direction::Right => 'R',
		});

		match dir {
			Direction::Up => self.browser.user_press("Up"),
			Direction::Down => self.browser.user_press("Down"),
			Direction::Left => self.browser.goto_previous_tab(),
			Direction::Right => self.browser.goto_next_tab(),
		};
	}
}

impl Tabs {
	fn new(browser: Arc<Browser>, config: Arc<Config>, run: Arc<Mutex<Arc<Browser>>>) -> Self {
		Self {
			browser,
			config: config.clone(),
			run,
		}
	}
}

impl Handler for Tabs {
	fn button_press(&self, id: u16) {
		if let Ok(name) = self.config.tabs_key(id) {
			if let Some(command) = name.strip_prefix("!") {
				execute(self.run.clone(), command);
			} else {
				info!("Goto tab {name}");
				self.browser.goto_by_name(&name, false);
			}
		}
	}

	fn dpad_press(&self, _dir: Direction) {}
}

impl Timers {
	fn new(
		browser: Arc<Browser>,
		config: Arc<Config>,
		time_since_last: Arc<TimeSinceLast>,
	) -> Self {
		Self {
			browser,
			config,
			time_since_last,
		}
	}
}

impl Handler for Timers {
	fn button_press(&self, id: u16) {
		if let Ok(name) = self.config.timers_key(id) {
			self.browser.goto_by_name("timers", true);
			self.time_since_last.reset(&name);
		}
	}

	fn dpad_press(&self, _dir: Direction) {}
}

const CLIPS_DIR : &str = "~/clips";

impl Clip {
    pub fn show(run: Arc<Mutex<Arc<Browser>>>, mqtt_topic: String, mqtt_msg: String) {
        match mqtt_topic.as_str() {
            "clip/play" => {
                let path = format!("{}/{}", CLIPS_DIR, mqtt_msg);
                info!("[CLIP] Searching for {}...", path);
                execute(run, format!("DISPLAY=:0 mpv {}", path).as_str());
                // if std::fs::exists(&mqtt_msg).unwrap() {
                //     let md = std::fs::metadata(&mqtt_msg).unwrap();
                //     if md.is_file() {
                //         info!("[CLIP] Found file at {}, playing!", path);
                //         // Execute as soon as given

                //     }
                // }
            }
            _ => {
                error!("[CLIP] Unrecognized clip mqtt topic: {}", &mqtt_topic);
            }
        }
    }
}

impl Idle {
	pub fn new(config: &Config, browser: Arc<Browser>, run: Arc<Mutex<Arc<Browser>>>) -> Arc<Self> {
		let client = match config.mqtt_hostname() {
			Ok(hostname) => {
				let mut options = MqttOptions::new("status-screen-idle", hostname, 1883);

				options.set_keep_alive(Duration::from_secs(60));

				let (client, mut connection) = rumqttc::Client::new(options, 10);
				client
					.subscribe("sensor/global/presence".to_string(), QoS::ExactlyOnce)
					.unwrap();
                client
                    .subscribe("clip/#".to_string(), QoS::AtLeastOnce)
                    .unwrap();

				thread::spawn(move || {
					for notification in connection.iter() {
						trace!("idle MQTT received: {notification:?}");

						if notification.is_err() {
							thread::sleep(Duration::from_secs(1));
							continue;
						}

						let Event::Incoming(Incoming::Publish(msg)) = notification.unwrap() else {
							continue;
						};

                        if msg.topic.as_str().starts_with("clip/") {
                            info!("[CLIP] Attempting to play clip from MQTT...");
                            Clip::show(run.clone(), String::from(msg.topic.as_str()), String::from_utf8(msg.payload.to_vec()).unwrap());
                            continue;
                        }
						if msg.topic.as_str() != "sensor/global/presence" {
							continue;
						}

						let payload = String::from_utf8(msg.payload.to_vec()).unwrap();
						if payload == "empty" {
							info!("sending display to sleep");
							browser.display_sleep();
						} else {
							info!("resuming display");
							browser.display_resume();
						}
					}
				});

				Some(client)
			}
			Err(err) => {
				warn!("MQTT not configured: {err}");
				None
			}
		};

		Arc::new(Self { _client: client })
	}
}
