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
use std::{
	collections::HashMap,
	process::Command,
	sync::{Arc, Condvar, Mutex, MutexGuard},
	thread,
	time::{Duration, Instant},
};

use anyhow::{Error, anyhow};
use libxdo::{Search, Window, XDo};
use log::{debug, error, info, trace, warn};
use rumqttc::MqttOptions;

use crate::config::{CommandLineArgs, Config};

#[derive(Debug)]
pub struct Browser {
	kiosk: bool,
	urls: Vec<String>,
	tabs: HashMap<String, usize>,
	state: Mutex<BrowserState>,
	sleep: Condvar,
	config: Arc<Config>,
	hands: Mutex<Hands>,
}

#[derive(Debug)]
struct BrowserState {
	tab: usize,
	changed: Instant,
	paused: bool,
}

#[derive(derive_more::Debug)]
struct Hands {
	no_search: bool,
	use_xdotool: bool,
	script: String,
	#[debug("XDo")]
	xdo: XDo,
	window: Option<Window>,
}

#[derive(derive_more::Debug)]
pub struct TimeSinceLast {
	#[debug("{:?}", client.is_some())]
	client: Option<rumqttc::Client>,
}

impl Browser {
	const FIRST_TAB: usize = 1;

	pub fn new(args: &CommandLineArgs, config: Arc<Config>) -> Arc<Self> {
		let mut open_urls = Vec::<String>::new();
		let mut tabs = HashMap::new();

		if let Ok(config_urls) = config.browser_urls() {
			for (name, url) in config_urls {
				if tabs.insert(name.clone(), tabs.len() + 1).is_none() {
					open_urls.push(url);
				} else {
					warn!("Duplicate url {name} ignored");
				}
			}
		}

		Arc::new(Self {
			kiosk: args.kiosk,
			urls: open_urls,
			tabs,
			state: Mutex::new(BrowserState::default()),
			sleep: Condvar::new(),
			config,
			hands: Mutex::new(Hands::new(args.xdotool, args.no_search)),
		})
	}

	pub fn run(self: &Arc<Browser>) {
		let mut command = Command::new("chromium-browser");
		if self.kiosk {
			command.arg("--kiosk");
		}
		command.arg("--disable-web-security").arg("--temp-profile");
		command.args(&self.urls);

		let mut child = command.spawn().expect("Browser failed to start");
		if self.kiosk {
			let self_copy = self.clone();

			thread::spawn(move || {
				self_copy.autoscroll();
			});
		}
		child.wait().expect("Browser failed to run");
		error!("Browser stopped");
	}

	pub fn user_activity(&self) {
		let mut state = self.state.lock().unwrap();
		debug!("User activity");
		self.activity(&mut state);
	}

	fn activity(&self, state: &mut MutexGuard<BrowserState>) {
		state.changed = Instant::now();
		self.sleep.notify_all();
	}

	fn autoscroll(&self) {
		let mut state = self.state.lock().unwrap();
		let mut multiplier = 2; /* startup is slow */

		loop {
			let now = Instant::now();
			let next = state.changed
				+ if state.paused {
					self.config.autoscroll_pause()
				} else {
					self.config.autoscroll_delay() * multiplier
				};

			if now < next {
				let timeout = next - now;

				trace!("Sleep for {timeout:?}");

				state = self.sleep.wait_timeout(state, timeout).unwrap().0;
				continue;
			}

			multiplier = 1;

			let tab = self.next_tab_id(&state);

			debug!("Go to next tab");
			self.change_tab(&mut state, tab);
		}
	}

	pub fn goto_previous_tab(&self) {
		let mut state = self.state.lock().unwrap();
		let tab = self.previous_tab_id(&state);

		debug!("Go to previous tab");
		self.unpause(&mut state);
		self.change_tab(&mut state, tab);
	}

	pub fn goto_next_tab(&self) {
		let mut state = self.state.lock().unwrap();
		let tab = self.next_tab_id(&state);

		debug!("Go to next tab");
		self.unpause(&mut state);
		self.change_tab(&mut state, tab);
	}

	pub fn reload_tab(&self) {
		let mut state = self.state.lock().unwrap();

		debug!("Reload tab");
		self.press("Ctrl+r");
		self.activity(&mut state);
	}

	pub fn goto_by_name(&self, name: &str, sync: bool) {
		debug!("Go to tab {name}");

		match self.tabs.get(name) {
			Some(tab) => {
				let mut state = self.state.lock().unwrap();

				self.unpause(&mut state);

				if self.change_tab(&mut state, *tab) && sync {
					thread::sleep(time::Duration::from_millis(100));
				}
			}
			None => {
				warn!("Tab {name} not found");
			}
		}
	}

	pub fn user_press(&self, keys: &str) {
		let mut state = self.state.lock().unwrap();

		debug!("Press keys on browser: {keys} (user)");
		self.press(keys);
		self.activity(&mut state);
	}

	fn press(&self, keys: &str) {
		let mut hands = self.hands.lock().unwrap();

		trace!("Press keys on browser: {keys}");

		hands.press(keys);
	}

	pub fn toggle_pause(&self) {
		let mut state = self.state.lock().unwrap();

		state.paused = !state.paused;

		if state.paused {
			info!("Paused");
		} else {
			info!("Unpaused");
		}

		self.activity(&mut state);
	}

	fn unpause(&self, state: &mut MutexGuard<BrowserState>) {
		if state.paused {
			state.paused = false;
			info!("Automatically unpaused");
		}
	}

	fn last_tab(&self) -> usize {
		self.tabs.len()
	}

	fn previous_tab_id(&self, state: &MutexGuard<BrowserState>) -> usize {
		if state.tab == Self::FIRST_TAB {
			self.last_tab()
		} else {
			state.tab - 1
		}
	}

	fn next_tab_id(&self, state: &MutexGuard<BrowserState>) -> usize {
		if state.tab == self.last_tab() {
			Self::FIRST_TAB
		} else {
			state.tab + 1
		}
	}

	fn change_tab(&self, state: &mut MutexGuard<BrowserState>, tab: usize) -> bool {
		const KEY_NEXT_TAB: &str = "Ctrl+Tab";
		const KEY_PREVIOUS_TAB: &str = "Ctrl+Shift+Tab";
		const DIRECT_TABS: usize = 8;
		const KEY_LAST_TAB: &str = "Ctrl+9";
		let key_last_direct_tab: &str = &("Ctrl+".to_owned() + &DIRECT_TABS.to_string());
		let current = state.tab;
		let previous = self.previous_tab_id(state);
		let next = self.next_tab_id(state);
		let last = self.last_tab();
		let direct_tabs = Self::FIRST_TAB..=DIRECT_TABS;
		let indirect_tabs = (DIRECT_TABS + 1)..last;

		if tab == current {
			/* Nothing to do */
		} else if direct_tabs.contains(&tab) {
			self.press(&("Ctrl+".to_owned() + &tab.to_string()));
		} else if tab == last {
			self.press(KEY_LAST_TAB);
		} else if tab == next {
			self.press(KEY_NEXT_TAB);
		} else if tab == previous {
			self.press(KEY_PREVIOUS_TAB);
		} else if indirect_tabs.contains(&tab) {
			let (keys_from_current_tab, key_from_current_tab) = if tab < current {
				(current - tab, KEY_PREVIOUS_TAB)
			} else {
				(tab - current, KEY_NEXT_TAB)
			};
			let keys_from_last_direct_tab = 1 + (tab - DIRECT_TABS);
			let keys_from_last_tab = 1 + (last - tab);
			let mut keys = Vec::new();

			if keys_from_current_tab <= keys_from_last_direct_tab.min(keys_from_last_tab) {
				/* Navigate from current tab */
				keys.resize(keys_from_current_tab, key_from_current_tab);
			} else if keys_from_last_direct_tab <= keys_from_last_tab {
				/* Navigate from last direct tab */
				keys.push(key_last_direct_tab);
				keys.resize(keys_from_last_direct_tab, KEY_NEXT_TAB);
			} else {
				/* Navigate from last tab */
				keys.push(KEY_LAST_TAB);
				keys.resize(keys_from_last_tab, KEY_PREVIOUS_TAB);
			}

			self.press(&keys.join(" "));
		} else {
			panic!(
				"Unable to navigate to invalid tab {tab} from tab {}",
				state.tab
			);
		}

		self.activity(state);

		if state.tab != tab {
			state.tab = tab;
			true
		} else {
			false
		}
	}
}

impl Default for BrowserState {
	fn default() -> Self {
		Self {
			tab: Browser::FIRST_TAB,
			changed: Instant::now(),
			paused: false,
		}
	}
}

impl Hands {
	const BROWSER_WINDOW_CLASS_REGEX: &str = "^chromium(-browser)?$";

	pub fn new(use_xdotool: bool, no_search: bool) -> Self {
		let mut script = "xdotool key ".to_string();

		if !no_search {
			script += "--window $(xdotool search --onlyvisible --class '";
			script += Self::BROWSER_WINDOW_CLASS_REGEX;
			script += "') ";
		}

		Self {
			use_xdotool,
			script,
			xdo: XDo::new(None).unwrap(),
			no_search,
			window: None,
		}
	}

	pub fn press(&mut self, keys: &str) {
		if let Err(err) = if self.use_xdotool {
			self.press_xdotool(keys)
		} else {
			self.press_libxdo(keys)
		} {
			error!("Unable to send keys {keys:?} to browser: {err}");
		}
	}

	fn press_xdotool(&mut self, keys: &str) -> Result<(), Error> {
		Command::new("sh")
			.arg("-c")
			.arg(self.script.clone() + keys)
			.output()?;
		Ok(())
	}

	fn press_libxdo(&mut self, keys: &str) -> Result<(), Error> {
		/* xdotool(1): Delay between keystrokes. Default is 12ms. */
		const DELAY_US: u32 = 12_000;

		let window = if self.no_search {
			None
		} else {
			if self.window.is_none() {
				self.window = self
					.xdo
					.search_windows(Search {
						only_visible: true,
						window_class: Some(Self::BROWSER_WINDOW_CLASS_REGEX.to_string()),
						limit: 1,
						..Search::default()
					})
					.inspect_err(|err| error!("Unable to find browser window: {err}"))
					.ok()
					.and_then(|windows| {
						if windows.is_empty() {
							error!("No browser windows found");
						} else if windows.len() > 2 {
							warn!("Multiple browser windows found: {windows:?}");
						} else {
							trace!("Found one browser window: {}", windows[0]);
						}
						windows.first().copied()
					});
			}

			Some(self.window.ok_or(anyhow!("Browser window not found"))?)
		};

		self.xdo
			.send_keysequence(window, keys, DELAY_US)
			.inspect_err(|_| self.window = None)?;
		Ok(())
	}
}

impl TimeSinceLast {
	pub fn new(config: &Config) -> Arc<Self> {
		let client = match config.mqtt_hostname() {
			Ok(hostname) => {
				let mut options = MqttOptions::new("rumqtt-sync", hostname, 1883);

				options.set_keep_alive(Duration::from_secs(60));

				let (client, mut connection) = rumqttc::Client::new(options, 10);

				thread::spawn(move || {
					for notification in connection.iter() {
						trace!("MQTT received: {notification:?}");

						if notification.is_err() {
							thread::sleep(Duration::from_secs(1));
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

		Arc::new(Self { client })
	}

	pub fn reset(&self, name: &str) {
		if let Some(client) = &self.client {
			info!("Reset timer: {name}");

			if let Err(err) = client.publish(
				"time-since-last/reset",
				rumqttc::QoS::AtMostOnce,
				false,
				name,
			) {
				error!("MQTT publish failed: {err}");
			}
		}
	}
}
