use std::ffi::OsStr;
use std::process::Command;
use std::str;

use chrono::SecondsFormat;

fn print_env(key: &str, val: &str) {
	println!("cargo::rustc-env={key}={val}");
}

fn env_command<I, S>(env: &str, program: S, args: I) -> String
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let mut string = String::from("n/a");

	match Command::new(program).args(args).output() {
		Ok(output) => {
			string = String::from_utf8(output.stdout)
				.unwrap_or(string)
				.trim()
				.to_string();
			if string.is_empty() {
				print_env(env, "n/a");
			} else {
				print_env(env, string.as_str());
			}
		}
		Err(_e) => {
			print_env(env, "n/a");
		}
	}

	string
}

fn main() {
	env_command("GIT_HASH", "git", ["rev-parse", "--short", "HEAD"]);
	env_command("GIT_BRANCH", "git", ["rev-parse", "--abbrev-ref", "HEAD"]);
	env_command("GIT_VERSION", "git", ["describe", "--tags", "--always", "--dirty"]);

	print_env(
		"BUILD_DATE",
		chrono::Utc::now()
			.to_rfc3339_opts(SecondsFormat::Secs, true)
			.as_str(),
	);

	// std::process::exit(1); // debug
}
