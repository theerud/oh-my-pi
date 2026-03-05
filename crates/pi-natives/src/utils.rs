pub fn clamp_u32(val: u64) -> u32 {
	val.min(u32::MAX as u64) as u32
}

#[allow(dead_code, reason = "required for system implementations")]
pub fn command_exists(cmd: &str) -> bool {
	#[cfg(unix)]
	let check_cmd = "which";
	#[cfg(windows)]
	let check_cmd = "where";

	std::process::Command::new(check_cmd)
		.arg(cmd)
		.stdout(std::process::Stdio::null())
		.stderr(std::process::Stdio::null())
		.status()
		.is_ok_and(|s| s.success())
}
