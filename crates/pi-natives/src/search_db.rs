//! Shared native search DB state for grep/glob/fuzzyFind.
//!
//! This owns search-side shared state that should outlive individual native
//! calls: frecency tracking plus a per-root cache of `fff` file pickers.

use std::path::Path;
use std::sync::Arc;

#[cfg(feature = "text-search-native")]
use std::{
	collections::HashMap,
	sync::atomic::Ordering,
	time::Duration,
};

#[cfg(feature = "text-search-native")]
use fff::{FFFMode, FileItem, FilePicker, FrecencyTracker, SharedFrecency, SharedPicker};
use napi::bindgen_prelude::Result;
#[cfg(feature = "text-search-native")]
use napi::Error;
use napi_derive::napi;
#[cfg(feature = "text-search-native")]
use parking_lot::Mutex;

use crate::task;

struct SearchDbInner {
	path:            String,
	#[cfg(feature = "text-search-native")]
	shared_frecency: SharedFrecency,
	#[cfg(feature = "text-search-native")]
	pickers:         Mutex<HashMap<String, SharedPicker>>,
}

impl Drop for SearchDbInner {
	fn drop(&mut self) {
		#[cfg(feature = "text-search-native")]
		{
			for shared_picker in self.pickers.lock().values() {
				let Ok(mut guard) = shared_picker.write() else {
					continue;
				};
				if let Some(picker) = guard.as_mut() {
					picker.cancel();
					picker.stop_background_monitor();
				}
			}
		}
	}
}

#[derive(Clone)]
#[napi]
/// Long-lived native search state: frecency persistence and per-workspace file
/// picker caches.
pub struct SearchDb {
	inner: Arc<SearchDbInner>,
}

#[napi]
impl SearchDb {
	#[napi(constructor)]
	/// Create search DB state rooted at `path` (trimmed). An empty path skips
	/// frecency storage.
	pub fn new(path: String) -> Self {
		let normalized = path.trim().to_string();

		#[cfg(feature = "text-search-native")]
		{
			let shared_frecency: SharedFrecency = Default::default();

			if !normalized.is_empty()
				&& let Ok(tracker) = FrecencyTracker::new(&normalized, false)
			{
				if let Ok(mut guard) = shared_frecency.write() {
					*guard = Some(tracker);
				}
				let _ =
					FrecencyTracker::spawn_gc(Arc::clone(&shared_frecency), normalized.clone(), false);
			}

			Self {
				inner: Arc::new(SearchDbInner {
					path: normalized,
					shared_frecency,
					pickers: Mutex::new(HashMap::new()),
				}),
			}
		}

		#[cfg(not(feature = "text-search-native"))]
		{
			Self { inner: Arc::new(SearchDbInner { path: normalized }) }
		}
	}

	#[napi(getter)]
	/// Root path string associated with this instance (same as passed to the
	/// constructor).
	pub fn path(&self) -> String {
		self.inner.path.clone()
	}
}

impl SearchDb {
	#[allow(dead_code)]
	fn picker_key(root: &Path) -> String {
		root
			.canonicalize()
			.unwrap_or_else(|_| root.to_path_buf())
			.to_string_lossy()
			.into_owned()
	}

	#[cfg(feature = "text-search-native")]
	pub fn get_or_init_picker(&self, root: &Path) -> Result<SharedPicker> {
		let key = Self::picker_key(root);
		let mut pickers = self.inner.pickers.lock();
		if let Some(shared_picker) = pickers.get(&key) {
			return Ok(Arc::clone(shared_picker));
		}

		let shared_picker: SharedPicker = Default::default();
		FilePicker::new_with_shared_state(
			key.clone(),
			false,
			FFFMode::Ai,
			Arc::clone(&shared_picker),
			Arc::clone(&self.inner.shared_frecency),
		)
		.map_err(|err| Error::from_reason(format!("Failed to init file picker: {err}")))?;
		pickers.insert(key, Arc::clone(&shared_picker));
		Ok(shared_picker)
	}

	#[cfg(feature = "text-search-native")]
	pub fn update_frecency_scores(&self, item: &mut FileItem) {
		let Ok(guard) = self.inner.shared_frecency.read() else {
			return;
		};
		let Some(tracker) = guard.as_ref() else {
			return;
		};
		let _ = item.update_frecency_scores(tracker, FFFMode::Ai);
	}
}

pub fn wait_for_picker_scan(
	#[cfg(feature = "text-search-native")] shared_picker: &SharedPicker,
	#[cfg(not(feature = "text-search-native"))] _shared_picker: &SharedPicker,
	#[cfg(feature = "text-search-native")] ct: &task::CancelToken,
	#[cfg(not(feature = "text-search-native"))] _ct: &task::CancelToken,
) -> Result<()> {
	#[cfg(not(feature = "text-search-native"))]
	{
		Ok(())
	}

	#[cfg(feature = "text-search-native")]
	{
		let signal = {
			let guard = shared_picker
				.read()
				.map_err(|_| Error::from_reason("shared picker lock poisoned"))?;
			let Some(picker) = guard.as_ref() else {
				return Ok(());
			};
			picker.scan_signal()
		};

		while signal.load(Ordering::Acquire) {
			ct.heartbeat()?;
			std::thread::sleep(Duration::from_millis(10));
		}

		ct.heartbeat()?;
		Ok(())
	}
}

#[cfg(not(feature = "text-search-native"))]
pub type SharedPicker = ();
