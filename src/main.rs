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
mod config;
mod input;
mod output;

use std::process::ExitCode;

use anyhow::Error;
use clap::Parser;
use config::CommandLineArgs;

fn main() -> Result<ExitCode, Error> {
	let args: CommandLineArgs = CommandLineArgs::parse();

	stderrlog::new()
		.module(module_path!())
		.show_module_names(true)
		.verbosity(usize::from(args.verbose) + 2)
		.init()
		.unwrap();

	let config = config::Config::new(&args);
	let browser = output::Browser::new(&args, config.clone());
	let time_since_last = output::TimeSinceLast::new(&config);
	let input = input::Input::new(config.clone(), browser.clone(), time_since_last.clone())?;

	input.start();
	config.start();
	browser.run();
	Ok(ExitCode::FAILURE)
}
