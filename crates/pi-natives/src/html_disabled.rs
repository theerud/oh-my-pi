//! HTML-to-Markdown stubs for slim builds.

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::task;

const HTML_DISABLED_ERROR: &str = "HTML-to-Markdown conversion is disabled in this build";

/// Options for HTML to Markdown conversion.
#[napi(object)]
#[derive(Debug, Default)]
pub struct HtmlToMarkdownOptions {
	/// Remove navigation elements, forms, headers, footers.
	pub clean_content: Option<bool>,
	/// Skip images during conversion.
	pub skip_images:   Option<bool>,
}

#[napi]
pub fn html_to_markdown(
	_html: String,
	_options: Option<HtmlToMarkdownOptions>,
) -> task::Promise<String> {
	task::blocking("html_to_markdown", (), move |_| {
		Err(Error::from_reason(HTML_DISABLED_ERROR))
	})
}
