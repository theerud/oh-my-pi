//! AST-aware structural search and rewrite powered by ast-grep.

use std::collections::HashMap;
#[cfg(feature = "structural-search-native")]
use std::path::{Path, PathBuf};

#[cfg(feature = "structural-search-native")]
use ast_grep_core::tree_sitter::LanguageExt;
use napi::bindgen_prelude::*;
use napi_derive::napi;

#[cfg(feature = "structural-search-native")]
use crate::language::SupportLang;
use crate::task;

#[cfg(feature = "structural-search-native")]
const DEFAULT_FIND_LIMIT: u32 = 50;

#[napi(object)]
pub struct AstFindOptions<'env> {
	pub patterns:     Option<Vec<String>>,
	pub lang:         Option<String>,
	pub path:         Option<String>,
	pub glob:         Option<String>,
	pub selector:     Option<String>,
	pub strictness:   Option<String>,
	pub limit:        Option<u32>,
	pub offset:       Option<u32>,
	#[napi(js_name = "includeMeta")]
	pub include_meta: Option<bool>,
	pub context:      Option<u32>,
	pub signal:       Option<Unknown<'env>>,
	#[napi(js_name = "timeoutMs")]
	pub timeout_ms:   Option<u32>,
}

#[napi(object)]
pub struct AstFindMatch {
	pub path:           String,
	pub text:           String,
	#[napi(js_name = "byteStart")]
	pub byte_start:     u32,
	#[napi(js_name = "byteEnd")]
	pub byte_end:       u32,
	#[napi(js_name = "startLine")]
	pub start_line:     u32,
	#[napi(js_name = "startColumn")]
	pub start_column:   u32,
	#[napi(js_name = "endLine")]
	pub end_line:       u32,
	#[napi(js_name = "endColumn")]
	pub end_column:     u32,
	#[napi(js_name = "metaVariables")]
	pub meta_variables: Option<HashMap<String, String>>,
}

#[napi(object)]
pub struct AstFindResult {
	pub matches:            Vec<AstFindMatch>,
	#[napi(js_name = "totalMatches")]
	pub total_matches:      u32,
	#[napi(js_name = "filesWithMatches")]
	pub files_with_matches: u32,
	#[napi(js_name = "filesSearched")]
	pub files_searched:     u32,
	#[napi(js_name = "limitReached")]
	pub limit_reached:      bool,
	#[napi(js_name = "parseErrors")]
	pub parse_errors:       Option<Vec<String>>,
}

#[napi(object)]
pub struct AstReplaceOptions<'env> {
	pub rewrites:            Option<HashMap<String, String>>,
	pub lang:                Option<String>,
	pub path:                Option<String>,
	pub glob:                Option<String>,
	pub selector:            Option<String>,
	pub strictness:          Option<String>,
	#[napi(js_name = "dryRun")]
	pub dry_run:             Option<bool>,
	#[napi(js_name = "maxReplacements")]
	pub max_replacements:    Option<u32>,
	#[napi(js_name = "maxFiles")]
	pub max_files:           Option<u32>,
	#[napi(js_name = "failOnParseError")]
	pub fail_on_parse_error: Option<bool>,
	pub signal:              Option<Unknown<'env>>,
	#[napi(js_name = "timeoutMs")]
	pub timeout_ms:          Option<u32>,
}

#[napi(object)]
pub struct AstReplaceChange {
	pub path:           String,
	pub before:         String,
	pub after:          String,
	#[napi(js_name = "byteStart")]
	pub byte_start:     u32,
	#[napi(js_name = "byteEnd")]
	pub byte_end:       u32,
	#[napi(js_name = "deletedLength")]
	pub deleted_length: u32,
	#[napi(js_name = "startLine")]
	pub start_line:     u32,
	#[napi(js_name = "startColumn")]
	pub start_column:   u32,
	#[napi(js_name = "endLine")]
	pub end_line:       u32,
	#[napi(js_name = "endColumn")]
	pub end_column:     u32,
}

#[napi(object)]
pub struct AstReplaceFileChange {
	pub path:  String,
	pub count: u32,
}

#[napi(object)]
pub struct AstReplaceResult {
	pub changes:            Vec<AstReplaceChange>,
	#[napi(js_name = "fileChanges")]
	pub file_changes:       Vec<AstReplaceFileChange>,
	#[napi(js_name = "totalReplacements")]
	pub total_replacements: u32,
	#[napi(js_name = "filesTouched")]
	pub files_touched:      u32,
	#[napi(js_name = "filesSearched")]
	pub files_searched:     u32,
	pub applied:            bool,
	#[napi(js_name = "limitReached")]
	pub limit_reached:      bool,
	#[napi(js_name = "parseErrors")]
	pub parse_errors:       Option<Vec<String>>,
}

pub struct AstFindConfig {
	pub patterns:     Option<Vec<String>>,
	pub lang:         Option<String>,
	pub path:         Option<String>,
	pub glob:         Option<String>,
	pub selector:     Option<String>,
	pub strictness:   Option<String>,
	pub limit:        Option<u32>,
	pub offset:       Option<u32>,
	pub include_meta: Option<bool>,
	pub context:      Option<u32>,
}

pub struct AstReplaceConfig {
	pub rewrites:            Option<HashMap<String, String>>,
	pub lang:                Option<String>,
	pub path:                Option<String>,
	pub glob:                Option<String>,
	pub selector:            Option<String>,
	pub strictness:          Option<String>,
	pub dry_run:             Option<bool>,
	pub max_replacements:    Option<u32>,
	pub max_files:           Option<u32>,
	pub fail_on_parse_error: Option<bool>,
}

pub fn normalize_search_path(path: Option<String>) -> Result<std::path::PathBuf> {
	let raw = path.unwrap_or_else(|| ".".to_string());
	let candidate = std::path::PathBuf::from(raw.trim());
	let absolute = if candidate.is_absolute() {
		candidate
	} else {
		std::env::current_dir()
			.map_err(|err| Error::from_reason(format!("Failed to resolve cwd: {err}")))?
			.join(candidate)
	};
	Ok(std::fs::canonicalize(&absolute).unwrap_or(absolute))
}

#[cfg(feature = "structural-search-native")]
mod native_impl {
	use std::collections::BTreeSet;

	use ast_grep_core::{Language, MatchStrictness, matcher::Pattern, source::Edit};

	use super::*;
	use crate::{fs_cache, glob_util};

	pub struct FileCandidate {
		pub absolute_path: PathBuf,
		pub display_path:  String,
	}

	pub struct PendingFileChange {
		pub change: AstReplaceChange,
		pub edit:   Edit<String>,
	}

	pub fn to_u32(value: usize) -> u32 {
		value.min(u32::MAX as usize) as u32
	}

	pub static LANG_ALIASES: phf::Map<&'static str, SupportLang> = phf::phf_map! {
		"bash"           => SupportLang::Bash,
		"sh"             => SupportLang::Bash,
		"c"              => SupportLang::C,
		"cpp"            => SupportLang::Cpp,
		"c++"            => SupportLang::Cpp,
		"cc"             => SupportLang::Cpp,
		"cxx"            => SupportLang::Cpp,
		"csharp"         => SupportLang::CSharp,
		"c#"             => SupportLang::CSharp,
		"cs"             => SupportLang::CSharp,
		"css"            => SupportLang::Css,
		"diff"           => SupportLang::Diff,
		"patch"          => SupportLang::Diff,
		"elixir"         => SupportLang::Elixir,
		"ex"             => SupportLang::Elixir,
		"go"             => SupportLang::Go,
		"golang"         => SupportLang::Go,
		"haskell"        => SupportLang::Haskell,
		"hs"             => SupportLang::Haskell,
		"hcl"            => SupportLang::Hcl,
		"tf"             => SupportLang::Hcl,
		"tfvars"         => SupportLang::Hcl,
		"terraform"      => SupportLang::Hcl,
		"html"           => SupportLang::Html,
		"htm"            => SupportLang::Html,
		"java"           => SupportLang::Java,
		"javascript"     => SupportLang::JavaScript,
		"js"             => SupportLang::JavaScript,
		"jsx"            => SupportLang::JavaScript,
		"mjs"            => SupportLang::JavaScript,
		"cjs"            => SupportLang::JavaScript,
		"json"           => SupportLang::Json,
		"julia"          => SupportLang::Julia,
		"jl"             => SupportLang::Julia,
		"kotlin"         => SupportLang::Kotlin,
		"kt"             => SupportLang::Kotlin,
		"lua"            => SupportLang::Lua,
		"make"           => SupportLang::Make,
		"makefile"       => SupportLang::Make,
		"markdown"       => SupportLang::Markdown,
		"md"             => SupportLang::Markdown,
		"mdx"            => SupportLang::Markdown,
		"nix"            => SupportLang::Nix,
		"objc"           => SupportLang::ObjC,
		"objective-c"    => SupportLang::ObjC,
		"odin"           => SupportLang::Odin,
		"php"            => SupportLang::Php,
		"python"         => SupportLang::Python,
		"py"             => SupportLang::Python,
		"regex"          => SupportLang::Regex,
		"ruby"           => SupportLang::Ruby,
		"rb"             => SupportLang::Ruby,
		"rust"           => SupportLang::Rust,
		"rs"             => SupportLang::Rust,
		"scala"          => SupportLang::Scala,
		"solidity"       => SupportLang::Solidity,
		"sol"            => SupportLang::Solidity,
		"starlark"       => SupportLang::Starlark,
		"star"           => SupportLang::Starlark,
		"swift"          => SupportLang::Swift,
		"toml"           => SupportLang::Toml,
		"tsx"            => SupportLang::Tsx,
		"typescript"     => SupportLang::TypeScript,
		"ts"             => SupportLang::TypeScript,
		"mts"            => SupportLang::TypeScript,
		"cts"            => SupportLang::TypeScript,
		"verilog"        => SupportLang::Verilog,
		"systemverilog"  => SupportLang::Verilog,
		"sv"             => SupportLang::Verilog,
		"xml"            => SupportLang::Xml,
		"xsl"            => SupportLang::Xml,
		"svg"            => SupportLang::Xml,
		"yaml"           => SupportLang::Yaml,
		"yml"            => SupportLang::Yaml,
		"zig"            => SupportLang::Zig,
	};

	pub fn supported_lang_list() -> String {
		let mut keys: Vec<&str> = LANG_ALIASES.keys().copied().collect();
		keys.sort_unstable();
		keys.join(", ")
	}

	pub fn resolve_supported_lang(value: &str) -> Result<SupportLang> {
		let lower = value.to_ascii_lowercase();
		LANG_ALIASES.get(lower.as_str()).copied().ok_or_else(|| {
			Error::from_reason(format!(
				"Unsupported language '{value}'. Supported: {}",
				supported_lang_list()
			))
		})
	}

	pub fn resolve_language(lang: Option<&str>, file_path: &Path) -> Result<SupportLang> {
		if let Some(lang) = lang.map(str::trim).filter(|lang| !lang.is_empty()) {
			return resolve_supported_lang(lang);
		}
		SupportLang::from_path(file_path).ok_or_else(|| {
			Error::from_reason(format!(
				"Unable to infer language from file extension: {}. Specify `lang` explicitly.",
				file_path.display()
			))
		})
	}

	pub fn is_supported_file(file_path: &Path, explicit_lang: Option<&str>) -> bool {
		if explicit_lang.is_some() {
			return true;
		}
		resolve_language(None, file_path).is_ok()
	}

	pub fn infer_single_replace_lang(
		candidates: &[FileCandidate],
		ct: &task::CancelToken,
	) -> Result<String> {
		let mut inferred = BTreeSet::new();
		let mut unresolved = Vec::new();
		for candidate in candidates {
			ct.heartbeat()?;
			match resolve_language(None, &candidate.absolute_path) {
				Ok(language) => {
					inferred.insert(language.canonical_name().to_string());
				},
				Err(err) => unresolved.push(format!("{}: {}", candidate.display_path, err)),
			}
		}
		if !unresolved.is_empty() {
			let details = unresolved
				.into_iter()
				.map(|entry| format!("- {entry}"))
				.collect::<Vec<_>>()
				.join("\n");
			return Err(Error::from_reason(format!(
				"`lang` is required for ast_edit when language cannot be inferred from all \
				 files:\n{details}"
			)));
		}
		if inferred.is_empty() {
			return Err(Error::from_reason(
				"`lang` is required for ast_edit when no files match path/glob".to_string(),
			));
		}
		if inferred.len() > 1 {
			return Err(Error::from_reason(format!(
				"`lang` is required for ast_edit when path/glob resolves to multiple languages: {}",
				inferred.into_iter().collect::<Vec<_>>().join(", ")
			)));
		}
		Ok(inferred.into_iter().next().expect("non-empty inferred set"))
	}

	pub fn parse_strictness(value: Option<&str>) -> Result<MatchStrictness> {
		let Some(raw) = value.map(str::trim).filter(|v| !v.is_empty()) else {
			return Ok(MatchStrictness::Smart);
		};
		raw.parse::<MatchStrictness>()
			.map_err(|err| Error::from_reason(format!("Invalid strictness '{raw}': {err}")))
	}

	pub fn collect_from_entries(
		root: &Path,
		entries: &[fs_cache::GlobMatch],
		glob_set: Option<&globset::GlobSet>,
		mentions_node_modules: bool,
		ct: &task::CancelToken,
	) -> Result<Vec<FileCandidate>> {
		let mut files = Vec::new();
		for entry in entries {
			ct.heartbeat()?;
			if entry.file_type != fs_cache::FileType::File {
				continue;
			}
			let relative = entry.path.replace('\\', "/");
			if fs_cache::should_skip_path(Path::new(&relative), mentions_node_modules) {
				continue;
			}
			if let Some(glob_set) = glob_set
				&& !glob_set.is_match(&relative)
			{
				continue;
			}
			files.push(FileCandidate { absolute_path: root.join(&relative), display_path: relative });
		}
		Ok(files)
	}

	pub fn collect_candidates(
		path: Option<String>,
		glob: Option<&str>,
		ct: &task::CancelToken,
	) -> Result<Vec<FileCandidate>> {
		let search_path = normalize_search_path(path)?;
		let metadata = std::fs::metadata(&search_path)
			.map_err(|err| Error::from_reason(format!("Path not found: {err}")))?;
		if metadata.is_file() {
			let display_path = search_path
				.file_name()
				.and_then(|name| name.to_str())
				.map_or_else(
					|| search_path.to_string_lossy().into_owned(),
					std::string::ToString::to_string,
				);
			return Ok(vec![FileCandidate { absolute_path: search_path, display_path }]);
		}
		if !metadata.is_dir() {
			return Err(Error::from_reason(format!(
				"Search path must be a file or directory: {}",
				search_path.display()
			)));
		}

		let glob_set = glob_util::try_compile_glob(glob, false)?;
		let mentions_node_modules = glob.is_some_and(|value| value.contains("node_modules"));
		let scan = fs_cache::get_or_scan(&search_path, true, true, ct)?;
		let mut files = collect_from_entries(
			&search_path,
			&scan.entries,
			glob_set.as_ref(),
			mentions_node_modules,
			ct,
		)?;

		if files.is_empty() && scan.cache_age_ms >= fs_cache::empty_recheck_ms() {
			let fresh = fs_cache::force_rescan(&search_path, true, true, true, ct)?;
			files = collect_from_entries(
				&search_path,
				&fresh,
				glob_set.as_ref(),
				mentions_node_modules,
				ct,
			)?;
		}

		files.sort_by(|a, b| a.display_path.cmp(&b.display_path));
		Ok(files)
	}

	pub fn compile_pattern(
		pattern: &str,
		selector: Option<&str>,
		strictness: &MatchStrictness,
		lang: SupportLang,
	) -> Result<Pattern> {
		let mut compiled =
			if let Some(selector) = selector.map(str::trim).filter(|s| !s.is_empty()) {
				Pattern::contextual(pattern, selector, lang)
			} else {
				Pattern::try_new(pattern, lang)
			}
			.map_err(|err| Error::from_reason(format!("Invalid pattern: {err}")))?;
		compiled.strictness = strictness.clone();
		Ok(compiled)
	}

	pub fn apply_edits(content: &str, edits: &[Edit<String>]) -> Result<String> {
		let mut sorted: Vec<&Edit<String>> = edits.iter().collect();
		sorted.sort_by_key(|edit| edit.position);
		let mut prev_end = 0usize;
		for edit in &sorted {
			if edit.position < prev_end {
				return Err(Error::from_reason(
					"Overlapping replacements detected; refine pattern to avoid ambiguous edits"
						.to_string(),
				));
			}
			prev_end = edit.position.saturating_add(edit.deleted_length);
		}

		let mut output = content.to_string();
		for edit in sorted.into_iter().rev() {
			let start = edit.position;
			let end = edit.position.saturating_add(edit.deleted_length);
			if end > output.len() || start > end {
				return Err(Error::from_reason("Computed edit range is out of bounds".to_string()));
			}
			let replacement = String::from_utf8(edit.inserted_text.clone()).map_err(|err| {
				Error::from_reason(format!("Replacement text is not valid UTF-8: {err}"))
			})?;
			output.replace_range(start..end, &replacement);
		}
		Ok(output)
	}

	pub fn normalize_pattern_list(patterns: Option<Vec<String>>) -> Result<Vec<String>> {
		let mut normalized = Vec::new();
		let mut seen = BTreeSet::new();
		for raw in patterns.unwrap_or_default() {
			let pattern = raw.trim();
			if pattern.is_empty() {
				continue;
			}
			if seen.insert(pattern.to_string()) {
				normalized.push(pattern.to_string());
			}
		}
		if normalized.is_empty() {
			return Err(Error::from_reason(
				"`patterns` is required and must include at least one non-empty pattern".to_string(),
			));
		}
		Ok(normalized)
	}

	pub fn normalize_rewrite_map(
		rewrites: Option<HashMap<String, String>>,
	) -> Result<Vec<(String, String)>> {
		let mut normalized = Vec::new();
		for (pattern, rewrite) in rewrites.unwrap_or_default() {
			if pattern.is_empty() {
				return Err(Error::from_reason(
					"`rewrites` keys must be non-empty pattern strings".to_string(),
				));
			}
			normalized.push((pattern, rewrite));
		}
		if normalized.is_empty() {
			return Err(Error::from_reason(
				"`rewrites` is required and must include at least one pattern->rewrite mapping"
					.to_string(),
			));
		}
		normalized.sort_by(|left, right| left.0.cmp(&right.0));
		Ok(normalized)
	}

	pub struct CompiledFindPattern {
		pub pattern:                String,
		pub compiled_by_lang:       HashMap<String, Pattern>,
		pub compile_errors_by_lang: HashMap<String, String>,
	}

	pub struct ResolvedCandidate {
		pub candidate:      FileCandidate,
		pub language:       Option<SupportLang>,
		pub language_error: Option<String>,
	}

	pub fn resolve_candidates_for_find(
		candidates: Vec<FileCandidate>,
		lang: Option<&str>,
		ct: &task::CancelToken,
	) -> Result<(Vec<ResolvedCandidate>, HashMap<String, SupportLang>)> {
		let mut resolved = Vec::with_capacity(candidates.len());
		let mut languages = HashMap::new();

		for candidate in candidates {
			ct.heartbeat()?;
			match resolve_language(lang, &candidate.absolute_path) {
				Ok(language) => {
					let key = language.canonical_name().to_string();
					languages.entry(key).or_insert(language);
					resolved.push(ResolvedCandidate {
						candidate,
						language: Some(language),
						language_error: None,
					});
				},
				Err(err) => {
					resolved.push(ResolvedCandidate {
						candidate,
						language: None,
						language_error: Some(err.to_string()),
					});
				},
			}
		}

		Ok((resolved, languages))
	}

	pub fn compile_find_patterns(
		patterns: &[String],
		languages: &HashMap<String, SupportLang>,
		selector: Option<&str>,
		strictness: &MatchStrictness,
		ct: &task::CancelToken,
	) -> Result<Vec<CompiledFindPattern>> {
		let mut compiled = Vec::with_capacity(patterns.len());

		for pattern in patterns {
			ct.heartbeat()?;
			let mut compiled_by_lang = HashMap::with_capacity(languages.len());
			let mut compile_errors_by_lang = HashMap::new();

			for (lang_key, &language) in languages {
				ct.heartbeat()?;
				match compile_pattern(pattern, selector, strictness, language) {
					Ok(compiled_pattern) => {
						compiled_by_lang.insert(lang_key.clone(), compiled_pattern);
					},
					Err(err) => {
						compile_errors_by_lang.insert(lang_key.clone(), err.to_string());
					},
				}
			}

			compiled.push(CompiledFindPattern {
				pattern: pattern.clone(),
				compiled_by_lang,
				compile_errors_by_lang,
			});
		}

		Ok(compiled)
	}

	pub fn ast_grep_sync(config: AstFindConfig, ct: task::CancelToken) -> Result<AstFindResult> {
		let AstFindConfig {
			patterns,
			lang,
			path,
			glob,
			selector,
			strictness,
			limit,
			offset,
			include_meta,
			..
		} = config;

		let normalized_limit = limit.unwrap_or(DEFAULT_FIND_LIMIT).max(1);
		let normalized_offset = offset.unwrap_or(0);

		let patterns = normalize_pattern_list(patterns)?;
		let strictness = parse_strictness(strictness.as_deref())?;
		let include_meta = include_meta.unwrap_or(false);
		let lang_str = lang.as_deref().map(str::trim).filter(|v| !v.is_empty());
		let candidates: Vec<_> = collect_candidates(path, glob.as_deref(), &ct)?
			.into_iter()
			.filter(|candidate| is_supported_file(&candidate.absolute_path, lang_str))
			.collect();

		let (resolved_candidates, languages) =
			resolve_candidates_for_find(candidates, lang_str, &ct)?;
		let compiled_patterns =
			compile_find_patterns(&patterns, &languages, selector.as_deref(), &strictness, &ct)?;
		let files_searched = to_u32(resolved_candidates.len());

		let mut all_matches = Vec::new();
		let mut parse_errors = Vec::new();
		let mut total_matches = 0u32;
		let mut files_with_matches = std::collections::BTreeSet::new();
		for resolved in resolved_candidates {
			ct.heartbeat()?;
			let ResolvedCandidate { candidate, language, language_error } = resolved;

			if let Some(error) = language_error.as_deref() {
				for compiled in &compiled_patterns {
					parse_errors
						.push(format!("{}: {}: {error}", compiled.pattern, candidate.display_path));
				}
				continue;
			}

			let Some(language) = language else {
				continue;
			};
			let lang_key = language.canonical_name();
			let source = match std::fs::read_to_string(&candidate.absolute_path) {
				Ok(source) => source,
				Err(err) => {
					for compiled in &compiled_patterns {
						parse_errors
							.push(format!("{}: {}: {err}", compiled.pattern, candidate.display_path));
					}
					continue;
				},
			};

			let mut runnable_patterns: Vec<(&str, &ast_grep_core::matcher::Pattern)> = Vec::new();
			for compiled in &compiled_patterns {
				ct.heartbeat()?;
				if let Some(error) = compiled.compile_errors_by_lang.get(lang_key) {
					parse_errors
						.push(format!("{}: {}: {error}", compiled.pattern, candidate.display_path));
					continue;
				}
				if let Some(pattern) = compiled.compiled_by_lang.get(lang_key) {
					runnable_patterns.push((compiled.pattern.as_str(), pattern));
				}
			}
			if runnable_patterns.is_empty() {
				continue;
			}

			let ast = language.ast_grep(source);
			if ast.root().dfs().any(|node| node.is_error()) {
				parse_errors.push(format!(
					"{}: parse error (syntax tree contains error nodes)",
					candidate.display_path
				));
			}

			for (_, pattern) in runnable_patterns {
				ct.heartbeat()?;
				for matched in ast.root().find_all(pattern.clone()) {
					ct.heartbeat()?;
					total_matches = total_matches.saturating_add(1);
					let range = matched.range();
					let start = matched.start_pos();
					let end = matched.end_pos();
					let meta_variables = if include_meta {
						Some(HashMap::<String, String>::from(matched.get_env().clone()))
					} else {
						None
					};
					all_matches.push(AstFindMatch {
						path: candidate.display_path.clone(),
						text: matched.text().into_owned(),
						byte_start: to_u32(range.start),
						byte_end: to_u32(range.end),
						start_line: to_u32(start.line().saturating_add(1)),
						start_column: to_u32(start.column(matched.get_node()).saturating_add(1)),
						end_line: to_u32(end.line().saturating_add(1)),
						end_column: to_u32(end.column(matched.get_node()).saturating_add(1)),
						meta_variables,
					});
					files_with_matches.insert(candidate.display_path.clone());
				}
			}
		}

		all_matches.sort_by(|left, right| {
			left
				.path
				.cmp(&right.path)
				.then(left.start_line.cmp(&right.start_line))
				.then(left.start_column.cmp(&right.start_column))
				.then(left.end_line.cmp(&right.end_line))
				.then(left.end_column.cmp(&right.end_column))
				.then(left.byte_start.cmp(&right.byte_start))
				.then(left.byte_end.cmp(&right.byte_end))
		});

		let visible_matches = all_matches
			.into_iter()
			.skip(normalized_offset as usize)
			.collect::<Vec<_>>();
		let limit_reached = visible_matches.len() > normalized_limit as usize;
		let matches = visible_matches
			.into_iter()
			.take(normalized_limit as usize)
			.collect::<Vec<_>>();

		Ok(AstFindResult {
			matches,
			total_matches,
			files_with_matches: to_u32(files_with_matches.len()),
			files_searched,
			limit_reached,
			parse_errors: (!parse_errors.is_empty()).then_some(parse_errors),
		})
	}

	pub fn ast_edit_sync(
		config: AstReplaceConfig,
		ct: task::CancelToken,
	) -> Result<AstReplaceResult> {
		let AstReplaceConfig {
			rewrites,
			lang,
			path,
			glob,
			selector,
			strictness,
			dry_run,
			max_replacements,
			max_files,
			fail_on_parse_error,
		} = config;

		let rewrite_rules = normalize_rewrite_map(rewrites)?;
		let strictness = parse_strictness(strictness.as_deref())?;
		let dry_run = dry_run.unwrap_or(true);
		let max_replacements = max_replacements.unwrap_or(u32::MAX).max(1);
		let max_files = max_files.unwrap_or(u32::MAX).max(1);
		let fail_on_parse_error = fail_on_parse_error.unwrap_or(false);

		let lang_str = lang.as_deref().map(str::trim).filter(|v| !v.is_empty());
		let candidates: Vec<_> = collect_candidates(path, glob.as_deref(), &ct)?
			.into_iter()
			.filter(|candidate| is_supported_file(&candidate.absolute_path, lang_str))
			.collect();
		let effective_lang = if let Some(lang) = lang_str {
			lang.to_string()
		} else {
			infer_single_replace_lang(&candidates, &ct)?
		};

		let language = resolve_supported_lang(&effective_lang)?;
		let mut parse_errors = Vec::new();
		let mut compiled_rules = Vec::new();
		for (pattern, rewrite) in rewrite_rules {
			ct.heartbeat()?;
			match compile_pattern(&pattern, selector.as_deref(), &strictness, language) {
				Ok(compiled) => compiled_rules.push((pattern, rewrite, compiled)),
				Err(err) => {
					if fail_on_parse_error {
						return Err(err);
					}
					parse_errors.push(format!("{pattern}: {err}"));
				},
			}
		}
		if compiled_rules.is_empty() {
			return Ok(AstReplaceResult {
				file_changes:       vec![],
				total_replacements: 0,
				files_touched:      0,
				files_searched:     to_u32(candidates.len()),
				applied:            !dry_run,
				limit_reached:      false,
				parse_errors:       (!parse_errors.is_empty()).then_some(parse_errors),
				changes:            vec![],
			});
		}

		let mut changes = Vec::new();
		let mut file_counts: std::collections::BTreeMap<String, u32> =
			std::collections::BTreeMap::new();
		let mut files_touched = 0u32;
		let mut limit_reached = false;

		for candidate in &candidates {
			ct.heartbeat()?;
			let source = match std::fs::read_to_string(&candidate.absolute_path) {
				Ok(source) => source,
				Err(err) => {
					if fail_on_parse_error {
						return Err(Error::from_reason(format!("{}: {err}", candidate.display_path)));
					}
					parse_errors.push(format!("{}: {err}", candidate.display_path));
					continue;
				},
			};

			let ast = language.ast_grep(&source);
			if ast.root().dfs().any(|node| node.is_error()) {
				let parse_issue = format!(
					"{}: parse error (syntax tree contains error nodes)",
					candidate.display_path
				);
				if fail_on_parse_error {
					return Err(Error::from_reason(parse_issue));
				}
				parse_errors.push(parse_issue);
				continue;
			}

			let mut file_changes = Vec::new();
			let mut reached_max_replacements = false;
			'patterns: for (_pattern, rewrite, compiled) in &compiled_rules {
				for matched in ast.root().find_all(compiled.clone()) {
					ct.heartbeat()?;
					if changes.len() + file_changes.len() >= max_replacements as usize {
						limit_reached = true;
						reached_max_replacements = true;
						break 'patterns;
					}
					let edit = matched.replace_by(rewrite.as_str());
					let range = matched.range();
					let start = matched.start_pos();
					let end = matched.end_pos();
					let after = String::from_utf8(edit.inserted_text.clone()).map_err(|err| {
						Error::from_reason(format!(
							"{}: replacement text is not valid UTF-8: {err}",
							candidate.display_path
						))
					})?;
					file_changes.push(PendingFileChange {
						change: AstReplaceChange {
							path: candidate.display_path.clone(),
							before: matched.text().into_owned(),
							after,
							byte_start: to_u32(range.start),
							byte_end: to_u32(range.end),
							deleted_length: to_u32(edit.deleted_length),
							start_line: to_u32(start.line().saturating_add(1)),
							start_column: to_u32(start.column(matched.get_node()).saturating_add(1)),
							end_line: to_u32(end.line().saturating_add(1)),
							end_column: to_u32(end.column(matched.get_node()).saturating_add(1)),
						},
						edit,
					});
				}
			}

			if file_changes.is_empty() {
				if reached_max_replacements {
					break;
				}
				continue;
			}
			if files_touched >= max_files {
				limit_reached = true;
				break;
			}
			files_touched = files_touched.saturating_add(1);
			file_counts.insert(candidate.display_path.clone(), to_u32(file_changes.len()));

			if !dry_run {
				let edits: Vec<ast_grep_core::source::Edit<String>> = file_changes
					.iter()
					.map(|entry| ast_grep_core::source::Edit {
						position:       entry.edit.position,
						deleted_length: entry.edit.deleted_length,
						inserted_text:  entry.edit.inserted_text.clone(),
					})
					.collect();
				let output = apply_edits(&source, &edits)?;
				if output != source {
					std::fs::write(&candidate.absolute_path, output).map_err(|err| {
						Error::from_reason(format!("Failed to write {}: {err}", candidate.display_path))
					})?;
				}
			}

			changes.extend(file_changes.into_iter().map(|entry| entry.change));
			if reached_max_replacements {
				break;
			}
		}

		let file_changes = file_counts
			.into_iter()
			.map(|(path, count)| AstReplaceFileChange { path, count })
			.collect::<Vec<_>>();

		Ok(AstReplaceResult {
			file_changes,
			total_replacements: to_u32(changes.len()),
			files_touched,
			files_searched: to_u32(candidates.len()),
			applied: !dry_run,
			limit_reached,
			parse_errors: (!parse_errors.is_empty()).then_some(parse_errors),
			changes,
		})
	}
}

#[cfg(all(not(feature = "structural-search-native"), feature = "structural-search-system"))]
mod system_impl {
	use std::{
		io::{BufRead, BufReader},
		process::{Command, Stdio},
	};

	use serde::Deserialize;

	use super::*;

	#[derive(Deserialize)]
	struct AstGrepMatch {
		text:                String,
		#[serde(rename = "file")]
		path:                String,
		range:               AstGrepRange,
		#[allow(dead_code, reason = "deserialized from json")]
		replacement:         Option<String>,
		#[serde(rename = "replacementOffsets")]
		#[allow(dead_code, reason = "deserialized from json")]
		replacement_offsets: Option<AstGrepByteOffset>,
		#[serde(rename = "metaVariables")]
		meta_variables:      Option<AstGrepMetaVariables>,
	}

	#[derive(Deserialize)]
	struct AstGrepRange {
		start:       AstGrepPos,
		end:         AstGrepPos,
		#[serde(rename = "byteOffset")]
		byte_offset: AstGrepByteOffset,
	}

	#[derive(Deserialize)]
	struct AstGrepByteOffset {
		start: u32,
		end:   u32,
	}

	#[derive(Deserialize)]
	struct AstGrepPos {
		line:   u32,
		column: u32,
	}

	#[derive(Deserialize)]
	struct AstGrepMetaVariables {
		single: HashMap<String, AstGrepMetaValue>,
	}

	#[derive(Deserialize)]
	struct AstGrepMetaValue {
		text: String,
	}

	pub fn ast_grep_sync(config: AstFindConfig, _ct: task::CancelToken) -> Result<AstFindResult> {
		if !crate::utils::command_exists("ast-grep") {
			return Err(Error::from_reason("ast-grep binary not found in PATH."));
		}

		let search_path = normalize_search_path(config.path.clone())?;
		let metadata = std::fs::metadata(&search_path)
			.map_err(|err| Error::from_reason(format!("Path not found: {err}")))?;

		let (current_dir, target_arg) = if metadata.is_file() {
			(
				search_path
					.parent()
					.unwrap_or_else(|| std::path::Path::new("."))
					.to_path_buf(),
				search_path
					.file_name()
					.map_or_else(|| ".".to_string(), |n| n.to_string_lossy().into_owned()),
			)
		} else {
			(search_path, ".".to_string())
		};

		let mut args = vec!["run".to_string(), "--json=stream".to_string()];

		if let Some(patterns) = config.patterns {
			if let Some(p) = patterns.first() {
				args.push("-p".to_string());
				args.push(p.clone());
			}
		}

		if let Some(lang) = config.lang {
			args.push("-l".to_string());
			args.push(lang);
		}

		args.push(target_arg);

		let mut child = Command::new("ast-grep")
			.args(&args)
			.current_dir(current_dir)
			.stdout(Stdio::piped())
			.spawn()
			.map_err(|e| Error::from_reason(format!("Failed to spawn ast-grep: {e}")))?;

		let stdout = child.stdout.take().unwrap();
		let reader = BufReader::new(stdout);

		let mut matches = vec![];
		let mut files_with_matches = std::collections::HashSet::new();

		for line in reader.lines() {
			let line =
				line.map_err(|e| Error::from_reason(format!("Error reading ast-grep output: {e}")))?;
			if let Ok(m) = serde_json::from_str::<AstGrepMatch>(&line) {
				let meta_variables = m
					.meta_variables
					.map(|mv| mv.single.into_iter().map(|(k, v)| (k, v.text)).collect());

				files_with_matches.insert(m.path.clone());
				matches.push(AstFindMatch {
					path: m.path,
					text: m.text,
					byte_start: m.range.byte_offset.start,
					byte_end: m.range.byte_offset.end,
					start_line: m.range.start.line + 1,
					start_column: m.range.start.column + 1,
					end_line: m.range.end.line + 1,
					end_column: m.range.end.column + 1,
					meta_variables,
				});
			}
		}

		let _ = child.wait();

		Ok(AstFindResult {
			total_matches: matches.len() as u32,
			files_with_matches: files_with_matches.len() as u32,
			files_searched: 0, // Unknown from stream
			limit_reached: false,
			parse_errors: None,
			matches,
		})
	}

	pub fn ast_edit_sync(
		config: AstReplaceConfig,
		_ct: task::CancelToken,
	) -> Result<AstReplaceResult> {
		if !crate::utils::command_exists("ast-grep") {
			return Err(Error::from_reason("ast-grep binary not found in PATH."));
		}

		let search_path = normalize_search_path(config.path.clone())?;
		let metadata = std::fs::metadata(&search_path)
			.map_err(|err| Error::from_reason(format!("Path not found: {err}")))?;

		let (current_dir, target_arg) = if metadata.is_file() {
			(
				search_path
					.parent()
					.unwrap_or_else(|| std::path::Path::new("."))
					.to_path_buf(),
				search_path
					.file_name()
					.map_or_else(|| ".".to_string(), |n| n.to_string_lossy().into_owned()),
			)
		} else {
			(search_path, ".".to_string())
		};

		let rewrites = config.rewrites.unwrap_or_default();
		let dry_run = config.dry_run.unwrap_or(true);

		let mut all_changes = Vec::new();
		let mut files_touched = std::collections::HashSet::new();

		for (pattern, replacement) in rewrites {
			let mut args = vec![
				"run".to_string(),
				"--pattern".to_string(),
				pattern,
				"--rewrite".to_string(),
				replacement,
				"--json=stream".to_string(),
			];

			if let Some(lang) = &config.lang {
				args.push("-l".to_string());
				args.push(lang.clone());
			}

			if !dry_run {
				args.push("--update-all".to_string());
			}

			args.push(target_arg.clone());

			let mut child = Command::new("ast-grep")
				.args(&args)
				.current_dir(&current_dir)
				.stdout(Stdio::piped())
				.spawn()
				.map_err(|e| Error::from_reason(format!("Failed to spawn ast-grep: {e}")))?;

			let stdout = child.stdout.take().unwrap();
			let reader = BufReader::new(stdout);

			for line in reader.lines() {
				let line = line.map_err(|e| {
					Error::from_reason(format!("Error reading ast-grep output: {e}"))
				})?;
				if let Ok(m) = serde_json::from_str::<AstGrepMatch>(&line) {
					files_touched.insert(m.path.clone());
					all_changes.push(AstReplaceChange {
						path: m.path,
						before: m.text,
						after: m.replacement.unwrap_or_default(),
						byte_start: m.range.byte_offset.start,
						byte_end: m.range.byte_offset.end,
						deleted_length: m.range.byte_offset.end.saturating_sub(m.range.byte_offset.start),
						start_line: m.range.start.line + 1,
						start_column: m.range.start.column + 1,
						end_line: m.range.end.line + 1,
						end_column: m.range.end.column + 1,
					});
				}
			}
			let _ = child.wait();
		}

		let mut file_counts = HashMap::new();
		for change in &all_changes {
			*file_counts.entry(change.path.clone()).or_insert(0) += 1;
		}

		let file_changes = file_counts
			.into_iter()
			.map(|(path, count)| AstReplaceFileChange { path, count })
			.collect();

		Ok(AstReplaceResult {
			total_replacements: all_changes.len() as u32,
			files_touched: files_touched.len() as u32,
			files_searched: 0,
			applied: !dry_run,
			limit_reached: false,
			parse_errors: None,
			changes: all_changes,
			file_changes,
		})
	}
}

#[cfg(all(not(feature = "structural-search-native"), not(feature = "structural-search-system")))]
mod no_impl {
	use super::*;
	pub fn ast_grep_sync(_config: AstFindConfig, _ct: task::CancelToken) -> Result<AstFindResult> {
		Err(Error::from_reason("Structural search is disabled in this build."))
	}
	pub fn ast_edit_sync(
		_config: AstReplaceConfig,
		_ct: task::CancelToken,
	) -> Result<AstReplaceResult> {
		Err(Error::from_reason("Structural search is disabled in this build."))
	}
}

#[cfg(feature = "structural-search-native")]
use native_impl::*;
#[cfg(all(
	not(feature = "structural-search-native"),
	not(feature = "structural-search-system")
))]
use no_impl::*;
#[cfg(all(not(feature = "structural-search-native"), feature = "structural-search-system"))]
use system_impl::*;

#[napi(js_name = "astGrep")]
pub fn ast_grep(options: AstFindOptions<'_>) -> task::Async<AstFindResult> {
	#[allow(unused_variables, reason = "required for config initialization")]
	let AstFindOptions {
		patterns,
		lang,
		path,
		glob,
		selector,
		strictness,
		limit,
		offset,
		include_meta,
		context,
		signal,
		timeout_ms,
	} = options;

	let ct = task::CancelToken::new(timeout_ms, signal);

	let config = AstFindConfig {
		patterns,
		lang,
		path,
		glob,
		selector,
		strictness,
		limit,
		offset,
		include_meta,
		context,
	};

	task::blocking("ast_grep", ct, move |ct| ast_grep_sync(config, ct))
}

#[napi(js_name = "astEdit")]
pub fn ast_edit(options: AstReplaceOptions<'_>) -> task::Async<AstReplaceResult> {
	#[allow(unused_variables, reason = "required for config initialization")]
	let AstReplaceOptions {
		rewrites,
		lang,
		path,
		glob,
		selector,
		strictness,
		dry_run,
		max_replacements,
		max_files,
		fail_on_parse_error,
		signal,
		timeout_ms,
	} = options;

	let ct = task::CancelToken::new(timeout_ms, signal);

	let config = AstReplaceConfig {
		rewrites,
		lang,
		path,
		glob,
		selector,
		strictness,
		dry_run,
		max_replacements,
		max_files,
		fail_on_parse_error,
	};

	task::blocking("ast_edit", ct, move |ct| ast_edit_sync(config, ct))
}
