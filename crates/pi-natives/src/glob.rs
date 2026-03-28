//! Filesystem discovery with glob patterns, ignore semantics, and shared scan
//! caching.
//!
//! # Overview
//! Resolves a search root, obtains scanned entries via [`fs_cache`], applies
//! glob matching plus optional file-type filtering, and optionally streams each
//! accepted match through a callback.
//!
//! The walker always skips `.git`, and skips `node_modules` unless explicitly
//! requested.
//!
//! # Example
//! ```ignore
//! // JS: await native.glob({ pattern: "*.rs", path: "." })
//! ```

#[cfg(feature = "discovery-native")]
use std::path::Path;

#[cfg(feature = "discovery-native")]
use globset::GlobSet;
use napi::{
	bindgen_prelude::*,
	threadsafe_function::ThreadsafeFunction,
};
#[cfg(feature = "discovery-native")]
use napi::threadsafe_function::ThreadsafeFunctionCallMode;
use napi_derive::napi;

// Re-export entry types so existing `glob::FileType` / `glob::GlobMatch` paths still work.
pub use crate::fs_cache::{FileType, GlobMatch};
#[cfg(feature = "discovery-native")]
use crate::glob_util;
use crate::{
	fs_cache,
	search_db::SearchDb,
	task,
};
#[cfg(feature = "text-search-native")]
use crate::search_db::wait_for_picker_scan;

/// Input options for `glob`, including traversal, filtering, and cancellation.
#[napi(object)]
pub struct GlobOptions<'env> {
	/// Glob pattern to match (e.g., "*.ts").
	pub pattern:              String,
	/// Directory to search.
	pub path:                 String,
	/// Filter by file type: "file", "dir", or "symlink". Symlinks are
	/// matched for file/dir filters based on their target type.
	#[napi(js_name = "fileType")]
	pub file_type:            Option<FileType>,
	/// Match simple patterns recursively by default (`*.ts` -> recursive).
	pub recursive:            Option<bool>,
	/// Include hidden files (default: false).
	pub hidden:               Option<bool>,
	/// Maximum number of results to return.
	#[napi(js_name = "maxResults")]
	pub max_results:          Option<u32>,
	/// Respect .gitignore files (default: true).
	pub gitignore:            Option<bool>,
	/// Enable shared filesystem scan cache (default: false).
	pub cache:                Option<bool>,
	/// Sort results by mtime (most recent first) before applying limit.
	#[napi(js_name = "sortByMtime")]
	pub sort_by_mtime:        Option<bool>,
	/// Include `node_modules` entries when the pattern does not explicitly
	/// mention them.
	#[napi(js_name = "includeNodeModules")]
	pub include_node_modules: Option<bool>,
	/// Abort signal for cancelling the operation.
	pub signal:               Option<Unknown<'env>>,
	/// Timeout in milliseconds for the operation.
	#[napi(js_name = "timeoutMs")]
	pub timeout_ms:           Option<u32>,
}

/// Result payload returned by a glob operation.
#[napi(object)]
pub struct GlobResult {
	/// Matched filesystem entries.
	pub matches:       Vec<GlobMatch>,
	/// Number of returned matches (`matches.len()`), clamped to `u32::MAX`.
	pub total_matches: u32,
}

/// Internal runtime config for a single glob execution.
struct GlobConfig {
	root:                  std::path::PathBuf,
	pattern:               String,
	#[cfg(feature = "discovery-native")]
	recursive:             bool,
	include_hidden:        bool,
	file_type_filter:      Option<FileType>,
	max_results:           usize,
	use_gitignore:         bool,
	#[cfg(feature = "discovery-native")]
	mentions_node_modules: bool,
	#[cfg(feature = "discovery-native")]
	sort_by_mtime:         bool,
	#[cfg(feature = "discovery-native")]
	use_cache:             bool,
}

#[cfg(feature = "discovery-native")]
fn resolve_symlink_target_type(root: &Path, relative_path: &str) -> Option<FileType> {
	let target_path = root.join(relative_path);
	let metadata = std::fs::metadata(target_path).ok()?;
	if metadata.is_dir() {
		Some(FileType::Dir)
	} else if metadata.is_file() {
		Some(FileType::File)
	} else {
		None
	}
}

#[cfg(feature = "discovery-native")]
fn apply_file_type_filter(entry: &GlobMatch, config: &GlobConfig) -> Option<FileType> {
	let Some(filter) = config.file_type_filter else {
		return Some(entry.file_type);
	};
	if entry.file_type == filter {
		return Some(entry.file_type);
	}
	if entry.file_type != FileType::Symlink {
		return None;
	}
	match filter {
		FileType::File | FileType::Dir => {
			let resolved = resolve_symlink_target_type(&config.root, &entry.path)?;
			if resolved == filter {
				Some(resolved)
			} else {
				None
			}
		},
		FileType::Symlink => None,
	}
}

/// Returns true if any path component starts with `.` (hidden file/dir).
#[cfg(feature = "text-search-native")]
fn has_hidden_component(path: &str) -> bool {
	path.split('/').any(|component| component.starts_with('.'))
}

/// Collect file matches from the shared `SearchDb` picker.
///
/// The picker indexes files only (no directories/symlinks), so this path is
/// currently used for `fileType=file` requests when gitignore semantics match
/// the picker configuration.
#[cfg(feature = "text-search-native")]
fn collect_files_from_picker(
	root: &Path,
	glob_set: &GlobSet,
	config: &GlobConfig,
	db: &SearchDb,
	on_match: Option<&ThreadsafeFunction<GlobMatch>>,
	ct: &task::CancelToken,
) -> Result<Vec<GlobMatch>> {
	let shared_picker = db.get_or_init_picker(root)?;
	wait_for_picker_scan(&shared_picker, ct)?;

	let guard = shared_picker
		.read()
		.map_err(|_| Error::from_reason("shared picker lock poisoned"))?;
	let Some(picker) = guard.as_ref() else {
		return Ok(Vec::new());
	};

	let mut matches = Vec::new();
	for file in picker.get_files() {
		ct.heartbeat()?;
		let relative_path = file.relative_path.replace('\\', "/");
		if !config.include_hidden && has_hidden_component(&relative_path) {
			continue;
		}
		if fs_cache::should_skip_path(Path::new(&relative_path), config.mentions_node_modules) {
			continue;
		}
		if !glob_set.is_match(Path::new(&relative_path)) {
			continue;
		}

		let matched_entry = GlobMatch {
			path:      relative_path,
			file_type: FileType::File,
			mtime:     Some((file.modified as f64) * 1000.0),
		};

		if let Some(callback) = on_match {
			callback.call(Ok(matched_entry.clone()), ThreadsafeFunctionCallMode::NonBlocking);
		}
		matches.push(matched_entry);
		if !config.sort_by_mtime && matches.len() >= config.max_results {
			break;
		}
	}

	Ok(matches)
}

/// Filter and collect matching entries from a pre-scanned list.
#[cfg(feature = "discovery-native")]
fn filter_entries(
	entries: &[GlobMatch],
	glob_set: &GlobSet,
	config: &GlobConfig,
	on_match: Option<&ThreadsafeFunction<GlobMatch>>,
	ct: &task::CancelToken,
) -> Result<Vec<GlobMatch>> {
	let mut matches = Vec::new();
	if config.max_results == 0 {
		return Ok(matches);
	}

	for entry in entries {
		ct.heartbeat()?;
		if fs_cache::should_skip_path(Path::new(&entry.path), config.mentions_node_modules) {
			// Apply post-scan node_modules policy before glob matching.
			continue;
		}
		if !glob_set.is_match(&entry.path) {
			continue;
		}
		let Some(effective_file_type) = apply_file_type_filter(entry, config) else {
			continue;
		};
		let mut matched_entry = entry.clone();
		matched_entry.file_type = effective_file_type;
		if let Some(callback) = on_match {
			callback.call(Ok(matched_entry.clone()), ThreadsafeFunctionCallMode::NonBlocking);
		}

		matches.push(matched_entry);
		// Only early-break when not sorting; mtime sort requires full candidate set.
		if !config.sort_by_mtime && matches.len() >= config.max_results {
			break;
		}
	}
	Ok(matches)
}

/// Executes matching/filtering over scanned entries and optionally streams each
/// hit.
#[cfg(feature = "discovery-native")]
fn run_glob(
	config: GlobConfig,
	#[allow(unused_variables)] db: Option<&SearchDb>,
	on_match: Option<&ThreadsafeFunction<GlobMatch>>,
	ct: task::CancelToken,
) -> Result<GlobResult> {
	let glob_set = glob_util::compile_glob(&config.pattern, config.recursive)?;
	if config.max_results == 0 {
		return Ok(GlobResult { matches: Vec::new(), total_matches: 0 });
	}

	#[cfg(feature = "text-search-native")]
	let use_picker = db.is_some() && config.use_gitignore && config.file_type_filter == Some(FileType::File);
	#[cfg(not(feature = "text-search-native"))]
	let use_picker = false;

	let mut matches = if use_picker {
		#[cfg(feature = "text-search-native")]
		{ collect_files_from_picker(&config.root, &glob_set, &config, db.unwrap(), on_match, &ct)? }
		#[cfg(not(feature = "text-search-native"))]
		{ Vec::new() }
	} else if config.use_cache {
		let scan =
			fs_cache::get_or_scan(&config.root, config.include_hidden, config.use_gitignore, &ct)?;
		let mut matches = filter_entries(&scan.entries, &glob_set, &config, on_match, &ct)?;
		// Empty-result recheck: if we got zero matches from a cached scan that's old
		// enough, force a rescan and try once more before returning empty.
		if matches.is_empty() && scan.cache_age_ms >= fs_cache::empty_recheck_ms() {
			let fresh = fs_cache::force_rescan(
				&config.root,
				config.include_hidden,
				config.use_gitignore,
				true,
				&ct,
			)?;
			matches = filter_entries(&fresh, &glob_set, &config, on_match, &ct)?;
		}
		matches
	} else {
		let fresh = fs_cache::force_rescan(
			&config.root,
			config.include_hidden,
			config.use_gitignore,
			false,
			&ct,
		)?;
		filter_entries(&fresh, &glob_set, &config, on_match, &ct)?
	};

	if config.sort_by_mtime {
		// Sorting mode: rank by mtime descending, then apply max-results truncation.
		matches.sort_by(|a, b| {
			let a_mtime = a.mtime.unwrap_or(0.0);
			let b_mtime = b.mtime.unwrap_or(0.0);
			b_mtime
				.partial_cmp(&a_mtime)
				.unwrap_or(std::cmp::Ordering::Equal)
		});
		matches.truncate(config.max_results);
	}
	let total_matches = matches.len().min(u32::MAX as usize) as u32;
	Ok(GlobResult { matches, total_matches })
}

#[cfg(feature = "discovery-system")]
mod system_impl {
	use std::{
		io::{BufRead, BufReader},
		process::{Command, Stdio},
	};

	use super::*;

	pub fn run_glob(config: GlobConfig, ct: task::CancelToken) -> Result<GlobResult> {
		if !crate::utils::command_exists("fd") {
			return Err(Error::from_reason("fd binary not found in PATH."));
		}

		let mut args = vec!["--glob".to_string(), config.pattern.clone()];
		if config.include_hidden {
			args.push("--hidden".to_string());
		}
		if !config.use_gitignore {
			args.push("--no-ignore".to_string());
		}
		if let Some(ft) = config.file_type_filter {
			match ft {
				FileType::File => args.push("--type=f".to_string()),
				FileType::Dir => args.push("--type=d".to_string()),
				FileType::Symlink => args.push("--type=l".to_string()),
			}
		}
		args.push("--color=never".to_string());
		args.push(config.root.to_string_lossy().into_owned());

		let mut child = Command::new("fd")
			.args(&args)
			.stdout(Stdio::piped())
			.spawn()
			.map_err(|e| Error::from_reason(format!("Failed to spawn fd: {e}")))?;

		let stdout = child.stdout.take().unwrap();
		let reader = BufReader::new(stdout);
		let mut matches = Vec::new();

		for line in reader.lines() {
			ct.heartbeat()?;
			let path = line.map_err(|e| Error::from_reason(format!("Error reading fd output: {e}")))?;
			// fd doesn't give us mtime easily without extra calls, so we'll leave it as None
			// or we could use metadata but that's slow. For a thin build, None is acceptable.
			matches.push(GlobMatch { path: path.clone(), file_type: FileType::File, mtime: None });
			if matches.len() >= config.max_results {
				break;
			}
		}

		let _ = child.kill();
		let total_matches = matches.len() as u32;
		Ok(GlobResult { matches, total_matches })
	}
}

/// Find filesystem entries matching a glob pattern.
///
/// Resolves the search root, scans entries, applies glob and optional file-type
/// filters, and optionally streams each accepted match through `on_match`.
///
/// If `sortByMtime` is enabled, all matching entries are collected, sorted by
/// descending mtime, then truncated to `maxResults`.
///
/// # Errors
/// Returns an error when the search path cannot be resolved, the path is not a
/// directory, the glob pattern is invalid, or cancellation/timeout is
/// triggered.
#[napi(js_name = "glob")]
pub fn glob(
	options: GlobOptions<'_>,
	#[napi(ts_arg_type = "((match: GlobMatch) => void) | undefined | null")] on_match: Option<
		ThreadsafeFunction<GlobMatch>,
	>,
	db: Option<&SearchDb>,
) -> task::Async<GlobResult> {
	let GlobOptions {
		pattern,
		path,
		file_type,
		recursive,
		hidden,
		max_results,
		gitignore,
		sort_by_mtime,
		cache,
		include_node_modules,
		timeout_ms,
		signal,
	} = options;

	let pattern_str = pattern.trim();
	let pattern_str = if pattern_str.is_empty() { "*" } else { pattern_str };
	let pattern_final = pattern_str.to_string();
	let mentions_node_modules = include_node_modules.unwrap_or_else(|| pattern_str.contains("node_modules"));

	#[cfg(not(feature = "discovery-native"))]
	{
		let _ = (recursive, sort_by_mtime, cache, mentions_node_modules);
	}

	let ct = task::CancelToken::new(timeout_ms, signal);
	let db = db.cloned();

	task::blocking("glob", ct, move |ct| {
		let config = GlobConfig {
			root: fs_cache::resolve_search_path(&path)?,
			include_hidden: hidden.unwrap_or(false),
			file_type_filter: file_type,
			max_results: max_results.map_or(usize::MAX, |value| value as usize),
			use_gitignore: gitignore.unwrap_or(true),
			pattern: pattern_final,
			#[cfg(feature = "discovery-native")]
			recursive: recursive.unwrap_or(true),
			#[cfg(feature = "discovery-native")]
			mentions_node_modules,
			#[cfg(feature = "discovery-native")]
			sort_by_mtime: sort_by_mtime.unwrap_or(false),
			#[cfg(feature = "discovery-native")]
			use_cache: cache.unwrap_or(false),
		};

		#[cfg(feature = "discovery-native")]
		{
			run_glob(config, db.as_ref(), on_match.as_ref(), ct)
		}

		#[cfg(all(not(feature = "discovery-native"), feature = "discovery-system"))]
		{
			let _ = (db, on_match);
			system_impl::run_glob(config, ct)
		}

		#[cfg(all(not(feature = "discovery-native"), not(feature = "discovery-system")))]
		{
			let _ = (config, db, on_match);
			Err(Error::from_reason("Glob discovery is disabled in this build."))
		}
	})
}
