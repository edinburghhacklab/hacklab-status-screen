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
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::{fmt, thread};

use anyhow::Error;
use evdev::{EventType, InputEvent};
use log::{debug, error, info, warn};

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
	run: Arc<Mutex<()>>,
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
	run: Arc<Mutex<()>>,
}

fn execute(run: Arc<Mutex<()>>, command: &str) {
	let command = command.to_owned();

	thread::spawn(move || {
		/* Don't run multiple commands concurrently */
		let _lock = run.lock().unwrap();

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
		let run = Arc::new(Mutex::new(()));

		Ok(Self {
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
				config
					.keyboard_device("tabs")
					.map(PathBuf::from),
				Handlers::from(Tabs::new(browser.clone(), config.clone(), run)),
			),
			timers: Device::new_optional(
				"timers",
				config
					.keyboard_device("timers")
					.map(PathBuf::from),
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

	pub fn new(config: Arc<Config>, run: Arc<Mutex<()>>) -> Self {
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
	fn new(browser: Arc<Browser>, config: Arc<Config>, run: Arc<Mutex<()>>) -> Self {
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
				9 => self.browser.toggle_pause(),      /* start */
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
	fn new(browser: Arc<Browser>, config: Arc<Config>, run: Arc<Mutex<()>>) -> Self {
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
