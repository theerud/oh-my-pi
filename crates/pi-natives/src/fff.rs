//! FFF-backed fuzzy file path discovery for autocomplete and @-mention
//! resolution.
//!
//! Uses `fff-search` as the fuzzy ranking engine. When a shared [`SearchDb`]
//! is provided, file results come from a per-root shared `FilePicker` cache;
//! directory results still come from one-shot scan entries because `fff`'s
//! picker indexes files only.

#[cfg(feature = "text-search-native")]
use std::path::Path;

#[cfg(feature = "text-search-native")]
use std::collections::HashSet;

#[cfg(feature = "text-search-native")]
use fff::{FileItem, FilePicker, FuzzySearchOptions, PaginationArgs, QueryParser};
use napi::bindgen_prelude::*;
use napi_derive::napi;

#[cfg(feature = "text-search-native")]
use crate::{
	fs_cache,
	search_db::wait_for_picker_scan,
};
use crate::task;

// ═══════════════════════════════════════════════════════════════════════════
// Public types
// ═══════════════════════════════════════════════════════════════════════════

/// Options for fuzzy file path search.
#[napi(object)]
pub struct FuzzyFindOptions<'env> {
	/// Fuzzy query to match against file paths (case-insensitive).
	pub query:       String,
	/// Directory to search.
	pub path:        String,
	/// Include hidden files (default: false).
	pub hidden:      Option<bool>,
	/// Respect .gitignore (default: true).
	pub gitignore:   Option<bool>,
	/// Enable shared filesystem scan cache (default: false).
	pub cache:       Option<bool>,
	/// Maximum number of matches to return (default: 100).
	#[napi(js_name = "maxResults")]
	pub max_results: Option<u32>,
	/// Abort signal for cancelling the operation.
	pub signal:      Option<Unknown<'env>>,
	/// Timeout in milliseconds for the operation.
	#[napi(js_name = "timeoutMs")]
	pub timeout_ms:  Option<u32>,
}

/// A single match in fuzzy find results.
#[napi(object)]
#[derive(Clone)]
pub struct FuzzyFindMatch {
	/// Relative path from the search root (uses `/` separators).
	pub path:         String,
	/// Whether this entry is a directory.
	#[napi(js_name = "isDirectory")]
	pub is_directory: bool,
	/// Match quality score (higher is better).
	pub score:        u32,
}

/// Result of fuzzy file path search.
#[napi(object)]
pub struct FuzzyFindResult {
	/// Matched entries (up to `maxResults`).
	pub matches:       Vec<FuzzyFindMatch>,
	/// Total number of matches found (may exceed `matches.len()`).
	#[napi(js_name = "totalMatches")]
	pub total_matches: u32,
}

// ═══════════════════════════════════════════════════════════════════════════
// Execution
// ═══════════════════════════════════════════════════════════════════════════

/// Internal configuration for fuzzy find, extracted from options.
#[cfg(feature = "text-search-native")]
struct FuzzyFindConfig {
	query:       String,
	path:        String,
	hidden:      Option<bool>,
	gitignore:   Option<bool>,
	max_results: Option<u32>,
	cache:       Option<bool>,
}

#[cfg(feature = "text-search-native")]
struct RankedMatch {
	path:         String,
	is_directory: bool,
	score:        u32,
}

#[cfg(feature = "text-search-native")]
fn entry_file_name(path: &str) -> String {
	Path::new(path)
		.file_name()
		.and_then(|name| name.to_str())
		.unwrap_or(path)
		.to_string()
}

#[cfg(feature = "text-search-native")]
fn modified_seconds(mtime_ms: Option<f64>) -> u64 {
	match mtime_ms {
		Some(mtime) if mtime.is_finite() && mtime >= 0.0 => (mtime / 1000.0) as u64,
		_ => 0,
	}
}

#[cfg(feature = "text-search-native")]
fn build_file_items(
	root: &Path,
	entries: &[fs_cache::GlobMatch],
	include_files: bool,
	include_directories: bool,
	db: Option<&crate::search_db::SearchDb>,
	ct: &task::CancelToken,
) -> Result<(Vec<FileItem>, HashSet<String>)> {
	let mut files = Vec::with_capacity(entries.len());
	let mut directories = HashSet::new();

	for entry in entries {
		ct.heartbeat()?;
		if entry.file_type == fs_cache::FileType::Symlink {
			continue;
		}

		let is_directory = entry.file_type == fs_cache::FileType::Dir;
		if is_directory && !include_directories {
			continue;
		}
		if !is_directory && !include_files {
			continue;
		}

		let mut relative_path = entry.path.clone();
		if is_directory {
			relative_path.push('/');
			directories.insert(relative_path.clone());
		}

		let mut item = FileItem::new_raw(
			root.join(&entry.path),
			relative_path,
			entry_file_name(&entry.path),
			0,
			modified_seconds(entry.mtime),
			None,
			false,
		);
		if !is_directory && let Some(db) = db {
			db.update_frecency_scores(&mut item);
		}
		files.push(item);
	}

	Ok((files, directories))
}

#[cfg(feature = "text-search-native")]
fn search_limit(max_results: usize) -> usize {
	max_results.max(50).saturating_mul(4).min(5000)
}

#[cfg(feature = "text-search-native")]
fn to_ranked_matches(
	results: fff::SearchResult,
	directories: &HashSet<String>,
) -> (Vec<RankedMatch>, u32) {
	let matches = results
		.items
		.into_iter()
		.zip(results.scores)
		.map(|(item, score)| RankedMatch {
			path:         item.relative_path.clone(),
			is_directory: directories.contains(&item.relative_path),
			score:        crate::utils::clamp_u32(score.total.max(0) as u64),
		})
		.collect();
	(matches, crate::utils::clamp_u32(results.total_matched as u64))
}

#[cfg(feature = "text-search-native")]
#[allow(clippy::too_many_arguments, reason = "search helper carries per-call filters explicitly")]
fn search_stateless_entries(
	root: &Path,
	entries: &[fs_cache::GlobMatch],
	query: &str,
	limit: usize,
	include_files: bool,
	include_directories: bool,
	db: Option<&crate::search_db::SearchDb>,
	ct: &task::CancelToken,
) -> Result<(Vec<RankedMatch>, u32)> {
	let (items, directories) =
		build_file_items(root, entries, include_files, include_directories, db, ct)?;
	let parser = QueryParser::default();
	let parsed = parser.parse(query);
	let results = FilePicker::fuzzy_search(&items, &parsed, None, FuzzySearchOptions {
		pagination: PaginationArgs { offset: 0, limit },
		..Default::default()
	});
	Ok(to_ranked_matches(results, &directories))
}

#[cfg(feature = "text-search-native")]
fn search_stateful_files(
	root: &Path,
	query: &str,
	limit: usize,
	db: &crate::search_db::SearchDb,
	ct: &task::CancelToken,
) -> Result<(Vec<RankedMatch>, u32)> {
	let shared_picker = db.get_or_init_picker(root)?;
	wait_for_picker_scan(&shared_picker, ct)?;
	let guard = shared_picker
		.read()
		.map_err(|_| Error::from_reason("shared picker lock poisoned"))?;
	let Some(picker) = guard.as_ref() else {
		return Ok((Vec::new(), 0));
	};

	let parser = QueryParser::default();
	let parsed = parser.parse(query);
	let results = FilePicker::fuzzy_search(picker.get_files(), &parsed, None, FuzzySearchOptions {
		pagination: PaginationArgs { offset: 0, limit },
		..Default::default()
	});
	Ok(to_ranked_matches(results, &HashSet::new()))
}

#[cfg(feature = "text-search-native")]
fn finalize_results(
	mut matches: Vec<RankedMatch>,
	total_matches: u32,
	max_results: usize,
) -> FuzzyFindResult {
	matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
	matches.truncate(max_results);
	FuzzyFindResult {
		matches: matches
			.into_iter()
			.map(|matched| FuzzyFindMatch {
				path:         matched.path,
				is_directory: matched.is_directory,
				score:        matched.score,
			})
			.collect(),
		total_matches,
	}
}

#[cfg(feature = "text-search-native")]
#[allow(clippy::too_many_arguments, reason = "search helper carries per-call filters explicitly")]
fn run_fff_search(
	root: &Path,
	entries: &[fs_cache::GlobMatch],
	query: &str,
	max_results: usize,
	include_hidden: bool,
	respect_gitignore: bool,
	db: Option<&crate::search_db::SearchDb>,
	ct: &task::CancelToken,
) -> Result<FuzzyFindResult> {
	let limit = search_limit(max_results);
	if let Some(db) = db
		&& include_hidden
		&& respect_gitignore
	{
		let (mut file_matches, file_total) = search_stateful_files(root, query, limit, db, ct)?;
		let (dir_matches, dir_total) =
			search_stateless_entries(root, entries, query, limit, false, true, None, ct)?;
		file_matches.extend(dir_matches);
		let total_matches = crate::utils::clamp_u32(file_total as u64 + dir_total as u64);
		return Ok(finalize_results(file_matches, total_matches, max_results));
	}

	let (matches, total_matches) =
		search_stateless_entries(root, entries, query, limit, true, true, db, ct)?;
	Ok(finalize_results(matches, total_matches, max_results))
}

#[cfg(feature = "text-search-native")]
fn fuzzy_find_sync(
	config: FuzzyFindConfig,
	db: Option<&crate::search_db::SearchDb>,
	ct: task::CancelToken,
) -> Result<FuzzyFindResult> {
	let root = fs_cache::resolve_search_path(&config.path)?;
	let include_hidden = config.hidden.unwrap_or(false);
	let respect_gitignore = config.gitignore.unwrap_or(true);
	let max_results = config.max_results.unwrap_or(100) as usize;
	if max_results == 0 {
		return Ok(FuzzyFindResult { matches: Vec::new(), total_matches: 0 });
	}

	let query = config.query.trim().to_string();
	let use_cache = config.cache.unwrap_or(false);
	if use_cache {
		let scan = fs_cache::get_or_scan(&root, include_hidden, respect_gitignore, &ct)?;
		let mut results = run_fff_search(
			&root,
			&scan.entries,
			&query,
			max_results,
			include_hidden,
			respect_gitignore,
			db,
			&ct,
		)?;
		if results.total_matches == 0
			&& !query.is_empty()
			&& scan.cache_age_ms >= fs_cache::empty_recheck_ms()
		{
			let fresh =
				fs_cache::force_rescan(&root, include_hidden, respect_gitignore, true, &ct)?;
			results = run_fff_search(
				&root,
				&fresh,
				&query,
				max_results,
				include_hidden,
				respect_gitignore,
				db,
				&ct,
			)?;
		}
		return Ok(results);
	}

	let fresh = fs_cache::force_rescan(&root, include_hidden, respect_gitignore, false, &ct)?;
	run_fff_search(&root, &fresh, &query, max_results, include_hidden, respect_gitignore, db, &ct)
}

#[cfg(feature = "fuzzy-search-system")]
mod system_impl {
	use std::{
		io::{BufRead, BufReader},
		process::{Command, Stdio},
	};

	use crate::fs_cache;
	use super::*;

	pub struct SystemFuzzyFindConfig {
		pub query:       String,
		pub path:        String,
		pub hidden:      Option<bool>,
		pub gitignore:   Option<bool>,
		pub max_results: Option<u32>,
	}

	pub fn fuzzy_find_sync(
		config: SystemFuzzyFindConfig,
		_db: Option<&crate::search_db::SearchDb>,
		ct: task::CancelToken,
	) -> Result<FuzzyFindResult> {
		if !crate::utils::command_exists("fd") {
			return Err(Error::from_reason("fd binary not found in PATH."));
		}

		let root = fs_cache::resolve_search_path(&config.path)?;
		let mut fd_args = vec!["--type".to_string(), "f".to_string(), "--type".to_string(), "d".to_string()];
		if config.hidden.unwrap_or(false) {
			fd_args.push("--hidden".to_string());
		}
		if !config.gitignore.unwrap_or(true) {
			fd_args.push("--no-ignore".to_string());
		}
		fd_args.push("--color=never".to_string());
		fd_args.push(".".to_string());
		fd_args.push(root.to_string_lossy().into_owned());

		let mut fd_child = Command::new("fd")
			.args(&fd_args)
			.stdout(Stdio::piped())
			.spawn()
			.map_err(|e| Error::from_reason(format!("Failed to spawn fd: {e}")))?;

		let query = config.query.trim();
		let mut matches = Vec::new();
		let max_results = config.max_results.unwrap_or(100) as usize;

		if query.is_empty() {
			let stdout = fd_child.stdout.take().unwrap();
			let reader = BufReader::new(stdout);
			for line in reader.lines() {
				ct.heartbeat()?;
				let path = line.map_err(|e| Error::from_reason(format!("Error reading fd output: {e}")))?;
				matches.push(FuzzyFindMatch {
					is_directory: path.ends_with('/'),
					path,
					score: 1,
				});
				if matches.len() >= max_results {
					break;
				}
			}
			let _ = fd_child.kill();
			let total_matches = total_matches_clamped(&matches);
			return Ok(FuzzyFindResult { matches, total_matches });
		}

		if !crate::utils::command_exists("fzf") {
			return Err(Error::from_reason(
				"fzf binary not found in PATH (required for fuzzy find query).",
			));
		}

		let mut fzf_child = Command::new("fzf")
			.args(["-f", query])
			.stdin(fd_child.stdout.take().unwrap())
			.stdout(Stdio::piped())
			.spawn()
			.map_err(|e| Error::from_reason(format!("Failed to spawn fzf: {e}")))?;

		let stdout = fzf_child.stdout.take().unwrap();
		let reader = BufReader::new(stdout);
		for line in reader.lines() {
			ct.heartbeat()?;
			let path = line.map_err(|e| Error::from_reason(format!("Error reading fzf output: {e}")))?;
			matches.push(FuzzyFindMatch {
				is_directory: path.ends_with('/'),
				path,
				score: 1,
			});
			if matches.len() >= max_results {
				break;
			}
		}

		let _ = fd_child.kill();
		let _ = fzf_child.kill();

		let total_matches = total_matches_clamped(&matches);
		Ok(FuzzyFindResult { matches, total_matches })
	}

	fn total_matches_clamped(matches: &[FuzzyFindMatch]) -> u32 {
		crate::utils::clamp_u32(matches.len() as u64)
	}
}

/// Fuzzy file path search for autocomplete.
///
/// # Arguments
/// - `options`: Query string, root path, and limits.
///
/// # Returns
/// Matching file and directory entries sorted by match quality.
#[napi(js_name = "fuzzyFind")]
pub fn fuzzy_find(
	mut options: FuzzyFindOptions<'_>,
	db: Option<&crate::search_db::SearchDb>,
) -> task::Promise<FuzzyFindResult> {
	let timeout_ms = options.timeout_ms;
	let signal = options.signal.take();

	#[cfg(feature = "fuzzy-search-native")]
	{
		let ct = task::CancelToken::new(timeout_ms, signal);
		let db = db.cloned();
		let config = FuzzyFindConfig {
			query:       options.query,
			path:        options.path,
			hidden:      options.hidden,
			gitignore:   options.gitignore,
			max_results: options.max_results,
			cache:       options.cache,
		};
		task::blocking("fuzzy_find", ct, move |ct| fuzzy_find_sync(config, db.as_ref(), ct))
	}

	#[cfg(all(not(feature = "fuzzy-search-native"), feature = "fuzzy-search-system"))]
	{
		let ct = task::CancelToken::new(timeout_ms, signal);
		let db = db.cloned();
		let config = system_impl::SystemFuzzyFindConfig {
			query:       options.query,
			path:        options.path,
			hidden:      options.hidden,
			gitignore:   options.gitignore,
			max_results: options.max_results,
		};
		let _ = options.cache;
		task::blocking("fuzzy_find", ct, move |ct| system_impl::fuzzy_find_sync(config, db.as_ref(), ct))
	}

	#[cfg(all(not(feature = "fuzzy-search-native"), not(feature = "fuzzy-search-system")))]
	{
		let ct = task::CancelToken::new(timeout_ms, signal);
		let _ = (options, db);
		task::blocking("fuzzy_find", ct, move |_| {
			Err(Error::from_reason("Fuzzy search is disabled in this build."))
		})
	}
}
