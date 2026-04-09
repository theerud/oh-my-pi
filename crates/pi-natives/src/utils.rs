#[macro_export]
macro_rules! env_uint {
	($( $vis:vis static $name:ident : $type:ty = $env:literal or $default:expr => [$min:expr, $max:expr];)*) => {
		$(
			$vis static $name: std::sync::LazyLock<$type> = std::sync::LazyLock::new(|| {
				std::env::var($env)
					.ok()
					.and_then(|v| std::str::FromStr::from_str(&v).ok())
					.unwrap_or($default)
					.clamp($min, $max)
			});
		)*
	};
	($( $vis:vis static $name:ident : $type:ty = $env:literal or $default:expr;)*) => {
		$(
			$vis static $name: std::sync::LazyLock<$type> = std::sync::LazyLock::new(|| {
				std::env::var($env)
					.ok()
					.and_then(|v| std::str::FromStr::from_str(&v).ok())
					.unwrap_or($default)
			});
		)*
	};
}

/// Saturating cast from `u64` to `u32`, clamping at [`u32::MAX`].
pub const fn clamp_u32(value: u64) -> u32 {
	if value > u32::MAX as u64 {
		u32::MAX
	} else {
		value as u32
	}
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
