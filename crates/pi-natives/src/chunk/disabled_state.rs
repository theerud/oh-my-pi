use napi::{Error, Result};
use napi_derive::napi;

use crate::chunk::types::{
	ChunkInfo, EditParams, EditResult, ReadRenderParams, ReadResult, RenderParams,
};

const CHUNK_DISABLED_ERROR: &str = "Chunk parsing is disabled in this build";

#[napi]
#[derive(Clone)]
pub struct ChunkState;

#[napi]
impl ChunkState {
	#[napi(factory)]
	pub fn parse(_source: String, _language: String) -> Result<Self> {
		Err(Error::from_reason(CHUNK_DISABLED_ERROR.to_string()))
	}

	#[napi(getter)]
	pub fn language(&self) -> String {
		String::new()
	}

	#[napi(getter)]
	pub fn source(&self) -> String {
		String::new()
	}

	#[napi(getter)]
	pub fn checksum(&self) -> String {
		String::new()
	}

	#[napi(getter)]
	pub fn line_count(&self) -> u32 {
		0
	}

	#[napi(getter)]
	pub fn parse_errors(&self) -> u32 {
		0
	}

	#[napi(getter)]
	pub fn fallback(&self) -> bool {
		true
	}

	#[napi(getter)]
	pub fn root_path(&self) -> String {
		String::new()
	}

	#[napi(getter)]
	pub fn root_children(&self) -> Vec<String> {
		Vec::new()
	}

	#[napi(getter)]
	pub fn chunk_count(&self) -> u32 {
		0
	}

	#[napi]
	pub fn root(&self) -> Option<ChunkInfo> {
		None
	}

	#[napi]
	pub fn chunk(&self, _chunk_path: String) -> Option<ChunkInfo> {
		None
	}

	#[napi]
	pub fn chunks(&self) -> Vec<ChunkInfo> {
		Vec::new()
	}

	#[napi]
	pub fn children(&self, _chunk_path: Option<String>) -> Result<Vec<ChunkInfo>> {
		Err(Error::from_reason(CHUNK_DISABLED_ERROR.to_string()))
	}

	#[napi]
	pub fn line_to_containing_chunk_path(&self, _line: u32) -> Option<String> {
		None
	}

	#[napi]
	pub fn render(&self, _params: RenderParams) -> String {
		CHUNK_DISABLED_ERROR.to_string()
	}

	#[napi]
	pub fn render_read(&self, _params: ReadRenderParams) -> Result<ReadResult> {
		Err(Error::from_reason(CHUNK_DISABLED_ERROR.to_string()))
	}

	#[napi]
	pub fn format_grep_line(&self, display_path: String, line_number: u32, line: String) -> String {
		format!("{display_path}>{line_number}|{line}")
	}

	#[napi]
	pub fn apply_edits(&self, _params: EditParams) -> Result<EditResult> {
		Err(Error::from_reason(CHUNK_DISABLED_ERROR.to_string()))
	}
}
