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

use std::{
	path::PathBuf,
	sync::{Arc, Mutex},
	thread,
	time::Duration,
};

use anyhow::{Error, anyhow};
use config::Value;
use indexmap::IndexMap;
use log::{error, trace, warn};

#[derive(Debug, Default, clap::Parser)]
#[command()]
pub struct CommandLineArgs {
	#[arg(short, long = "config", value_names = ["FILE"], default_value = "config")]
	pub config_file: PathBuf,

	/// Start browser in kiosk mode and autoscroll
	#[arg(short, long = "kiosk")]
	pub kiosk: bool,

	/// Use xdotool instead of libxdo
	#[arg(short, long = "xdotool")]
	pub xdotool: bool,

	/// Send keys to the current window without searching
	#[arg(short, long)]
	pub no_search: bool,

	/// Debug logging
	#[arg(short, long, action = clap::ArgAction::Count)]
	pub verbose: u8,
}

#[derive(Debug)]
pub struct Config {
	config_file: String,
	state: Mutex<State>,
}

#[derive(Debug)]
struct State {
	data: IndexMap<String, Value>,
	autoscroll_delay: Duration,
	autoscroll_pause: Duration,
}

impl Config {
	pub fn new(args: &CommandLineArgs) -> Arc<Self> {
		let config_file = args.config_file.to_str().unwrap();

		Arc::new(Self {
			config_file: config_file.to_owned(),
			state: Mutex::new(State::new(
				config::Config::builder()
					.add_source(config::File::with_name(config_file))
					.build()
					.unwrap()
					.try_deserialize::<IndexMap<String, Value>>()
					.unwrap(),
			)),
		})
	}

	pub fn start(self: &Arc<Self>) {
		let self_copy = self.clone();

		thread::spawn(move || {
			loop {
				thread::sleep(Duration::from_secs(60));

				let mut state = self_copy.state.lock().unwrap();

				trace!("Reloading config");

				if let Ok(new_data) = config::Config::builder()
					.add_source(config::File::with_name(self_copy.config_file.as_str()))
					.build()
					.inspect_err(|err| error!("Config file error: {err}"))
					.and_then(|config| config.try_deserialize::<IndexMap<String, Value>>())
				{
					*state = State::new(new_data);

					trace!("Reloaded config");
				}
			}
		});
	}

	pub fn browser_urls(&self) -> Result<IndexMap<String, String>, Error> {
		let state = self.state.lock().unwrap();

		Ok(state
			.data
			.get("urls")
			.ok_or(anyhow!("No urls in config"))?
			.clone()
			.into_table()?
			.clone()
			.iter()
			.filter_map(|(name, url)| match url.clone().into_string() {
				Ok(url) => Some((name.clone(), url)),
				Err(err) => {
					error!("Invalid url string for {name}: {err}");
					None
				}
			})
			.collect())
	}

	pub fn autoscroll_delay(&self) -> Duration {
		let state = self.state.lock().unwrap();

		state.autoscroll_delay
	}

	pub fn autoscroll_pause(&self) -> Duration {
		let state = self.state.lock().unwrap();

		state.autoscroll_pause
	}

	pub fn keyboard_device(&self, name: &str) -> Result<String, Error> {
		let state = self.state.lock().unwrap();

		Ok(state
			.data
			.get("keyboards")
			.ok_or(anyhow!("No keyboards in config"))?
			.clone()
			.into_table()?
			.get(name)
			.ok_or(anyhow!("Missing keyboard: {name}"))?
			.clone()
			.into_string()?
			.clone())
	}

	pub fn tabs_key(&self, id: u16) -> Result<String, Error> {
		let state = self.state.lock().unwrap();

		Ok(state
			.data
			.get("tabs")
			.ok_or(anyhow!("No tab keys in config"))?
			.clone()
			.into_table()?
			.get(&id.to_string())
			.ok_or(anyhow!("Missing key: {id}"))?
			.clone()
			.into_string()?
			.clone())
	}

	pub fn timers_key(&self, id: u16) -> Result<String, Error> {
		let state = self.state.lock().unwrap();

		Ok(state
			.data
			.get("timers")
			.ok_or(anyhow!("No timer keys in config"))?
			.clone()
			.into_table()?
			.get(&id.to_string())
			.ok_or(anyhow!("Missing key: {id}"))?
			.clone()
			.into_string()?
			.clone())
	}

	pub fn konami_command(&self) -> Result<String, Error> {
		let state = self.state.lock().unwrap();

		Ok(state
			.data
			.get("main")
			.ok_or(anyhow!("No main section in config"))?
			.clone()
			.into_table()?
			.get("konami")
			.ok_or(anyhow!("No konami setting in config"))?
			.clone()
			.into_string()?
			.clone())
	}

	pub fn mqtt_hostname(&self) -> Result<String, Error> {
		let state = self.state.lock().unwrap();

		Ok(state
			.data
			.get("mqtt")
			.ok_or(anyhow!("No mqtt section in config"))?
			.clone()
			.into_table()?
			.get("hostname")
			.ok_or(anyhow!("No hostname setting in config"))?
			.clone()
			.into_string()?
			.clone())
	}
}

impl State {
	pub fn new(data: IndexMap<String, Value>) -> Self {
		let autoscroll_section = data.get("autoscroll").and_then(|section| {
			section
				.clone()
				.into_table()
				.inspect_err(|err| warn!("Invalid autoscroll section in config: {err}"))
				.ok()
		});
		let autoscroll_delay = Duration::from_secs(
			autoscroll_section
				.as_ref()
				.and_then(|table| {
					table.clone().get("delay").and_then(|value| {
						value
							.clone()
							.into_uint()
							.inspect_err(|err| {
								warn!("Invalid autoscroll delay value in config: {err}")
							})
							.ok()
					})
				})
				.unwrap_or(20),
		);
		let autoscroll_pause = Duration::from_secs(
			autoscroll_section
				.as_ref()
				.and_then(|table| {
					table.clone().get("pause").and_then(|value| {
						value
							.clone()
							.into_uint()
							.inspect_err(|err| {
								warn!("Invalid autoscroll pause value in config: {err}")
							})
							.ok()
					})
				})
				.unwrap_or(900),
		);

		Self {
			data,
			autoscroll_delay,
			autoscroll_pause,
		}
	}
}
