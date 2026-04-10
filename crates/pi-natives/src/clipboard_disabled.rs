//! Clipboard stubs for slim/headless builds.

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::task;

const CLIPBOARD_DISABLED_ERROR: &str = "Clipboard support is disabled in this build";

/// Clipboard image payload encoded as PNG bytes.
#[napi(object)]
pub struct ClipboardImage {
	/// PNG-encoded image bytes.
	pub data:      Uint8Array,
	/// MIME type for the encoded image payload.
	pub mime_type: String,
}

#[napi]
pub fn copy_to_clipboard(_text: String) -> Result<()> {
	Err(Error::from_reason(CLIPBOARD_DISABLED_ERROR))
}

#[napi]
pub fn read_image_from_clipboard() -> task::Promise<Option<ClipboardImage>> {
	task::blocking("clipboard.read_image", (), move |_| -> Result<Option<ClipboardImage>> {
		Err(Error::from_reason(CLIPBOARD_DISABLED_ERROR))
	})
}
