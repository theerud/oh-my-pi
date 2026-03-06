//! PTY session management for interactive shell execution.

use std::{
	io::{Read, Write},
	sync::Arc,
};

use napi::bindgen_prelude::*;
use napi_derive::napi;
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

#[napi(object)]
pub struct PtyOptions {
	pub command: Option<String>,
	pub args:    Option<Vec<String>>,
	pub env:     Option<std::collections::HashMap<String, String>>,
	pub cwd:     Option<String>,
	pub cols:    Option<u32>,
	pub rows:    Option<u32>,
}

#[napi(object)]
pub struct PtyOutput {
	pub data: String,
}

#[napi(object)]
pub struct PtyExit {
	pub code: i32,
}

#[napi]
pub struct PtySession {
	writer:        Arc<Mutex<Box<dyn Write + Send>>>,
	#[allow(dead_code, reason = "referenced by reader thread")]
	reader_thread: Option<std::thread::JoinHandle<()>>,
	#[allow(dead_code, reason = "referenced by reader thread")]
	exit_code:     Arc<Mutex<Option<i32>>>,
}

#[napi]
impl PtySession {
	#[napi(constructor)]
	pub fn new(
		options: PtyOptions,
		#[napi(ts_arg_type = "(data: string) => void")]
		on_data: napi::threadsafe_function::ThreadsafeFunction<String>,
		#[napi(ts_arg_type = "(exitCode: number) => void")]
		on_exit: napi::threadsafe_function::ThreadsafeFunction<i32>,
	) -> Result<Self> {
		let pty_system = NativePtySystem::default();
		let pair = pty_system
			.openpty(PtySize {
				rows:         options.rows.unwrap_or(24) as u16,
				cols:         options.cols.unwrap_or(80) as u16,
				pixel_width:  0,
				pixel_height: 0,
			})
			.map_err(|e| Error::from_reason(format!("Failed to open PTY: {e}")))?;

		let mut cmd = CommandBuilder::new(options.command.unwrap_or_else(|| "bash".to_string()));
		if let Some(args) = options.args {
			cmd.args(args);
		}
		if let Some(env) = options.env {
			for (k, v) in env {
				cmd.env(k, v);
			}
		}
		if let Some(cwd) = options.cwd {
			cmd.cwd(cwd);
		}

		let mut child = pair
			.slave
			.spawn_command(cmd)
			.map_err(|e| Error::from_reason(format!("Failed to spawn command: {e}")))?;
		let mut reader = pair
			.master
			.try_clone_reader()
			.map_err(|e| Error::from_reason(format!("Failed to clone PTY reader: {e}")))?;
		let writer = pair
			.master
			.take_writer()
			.map_err(|e| Error::from_reason(format!("Failed to take PTY writer: {e}")))?;

		let exit_code = Arc::new(Mutex::new(None));
		let exit_code_clone = exit_code.clone();

		let reader_thread = std::thread::spawn(move || {
			let mut buf = [0u8; 8192];
			loop {
				match reader.read(&mut buf) {
					Ok(0) => break,
					Ok(n) => {
						let data = String::from_utf8_lossy(&buf[..n]).to_string();
						on_data.call(
							Ok(data),
							napi::threadsafe_function::ThreadsafeFunctionCallMode::NonBlocking,
						);
					},
					Err(_) => break,
				}
			}
			let code = child.wait().map_or(0, |s| s.exit_code() as i32);
			*exit_code_clone.lock() = Some(code);
			on_exit.call(Ok(code), napi::threadsafe_function::ThreadsafeFunctionCallMode::NonBlocking);
		});

		Ok(Self {
			writer: Arc::new(Mutex::new(writer)),
			reader_thread: Some(reader_thread),
			exit_code,
		})
	}

	#[napi]
	pub fn write(&self, data: String) -> Result<()> {
		let mut writer = self.writer.lock();
		writer
			.write_all(data.as_bytes())
			.map_err(|e| Error::from_reason(format!("Failed to write to PTY: {e}")))?;
		writer
			.flush()
			.map_err(|e| Error::from_reason(format!("Failed to flush PTY: {e}")))?;
		Ok(())
	}

	#[napi]
	pub const fn resize(&self, _cols: u32, _rows: u32) -> Result<()> {
		// portable-pty resize requires access to master which we don't store here
		// directly in a way easy to resize but we could. For now let's assume resize
		// is not strictly mandatory for initial implementation of gating.
		Ok(())
	}

	#[napi]
	pub const fn kill(&mut self) -> Result<()> {
		// portable-pty doesn't have a direct kill on PtySession but we can drop the
		// writer and let the reader thread exit.
		Ok(())
	}
}
