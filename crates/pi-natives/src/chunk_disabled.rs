use napi_derive::napi;

#[path = "chunk/kind.rs"]
pub mod kind;
#[path = "chunk/disabled_state.rs"]
pub mod state;
#[path = "chunk/types.rs"]
pub mod types;

pub use self::{
	state::ChunkState,
	types::{ChunkNode, ChunkTree},
};
use self::types::ChunkAnchorStyle;

#[napi]
pub fn format_anchor(
	name: String,
	checksum: String,
	style: ChunkAnchorStyle,
	omit_checksum: Option<bool>,
) -> String {
	style
		.with_omit_checksum(omit_checksum.unwrap_or(false))
		.render("", name.as_str(), checksum.as_str())
}

pub fn mask_chunk_display_source(source: &str, _language: &str) -> String {
	source.to_string()
}
