use anyhow::{anyhow, Context, Result};
use globset::{Glob, GlobMatcher};
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde::Serialize;
use std::cmp::Ordering as CmpOrdering;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const MAX_SEARCHABLE_FILE_BYTES: u64 = 1024 * 1024;
const MAX_PREVIEW_RESULTS: usize = 100;
const MAX_CONTEXT_LINES: usize = 20;
const MAX_LINE_PREVIEW_CHARS: usize = 500;
const MAX_MATCH_PREVIEW_CHARS: usize = 1_000;
const DEFAULT_SEARCH_TIMEOUT: Duration = Duration::from_secs(20);

const NOISY_DIRECTORIES_TO_EXCLUDE: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    ".bzr",
    ".jj",
    ".sl",
    "node_modules",
    ".next",
    "target",
    "dist",
    "build",
    ".cache",
    ".turbo",
    "__pycache__",
    ".venv",
    "venv",
    "coverage",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchQueryMode {
    Literal,
    Regex,
}

impl SearchQueryMode {
    pub fn from_str(value: Option<&str>) -> Self {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            Some("regex") => Self::Regex,
            _ => Self::Literal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOutputMode {
    Content,
    FilesWithMatches,
    Count,
}

impl SearchOutputMode {
    pub fn from_str(value: Option<&str>) -> Self {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            Some("files_with_matches") => Self::FilesWithMatches,
            Some("count") => Self::Count,
            _ => Self::Content,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::FilesWithMatches => "files_with_matches",
            Self::Count => "count",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalSearchRequest {
    pub workspace_root: PathBuf,
    pub search_root: PathBuf,
    pub query: String,
    pub file_pattern: Option<String>,
    pub file_type: Option<String>,
    pub query_mode: SearchQueryMode,
    pub output_mode: SearchOutputMode,
    pub case_insensitive: bool,
    pub multiline: bool,
    pub context_before: usize,
    pub context_after: usize,
    pub offset: usize,
    pub max_results: usize,
    pub timeout: Option<Duration>,
    pub cancellation: Option<LocalSearchCancellation>,
}

impl LocalSearchRequest {
    pub fn capped_max_results(&self) -> usize {
        self.max_results.clamp(1, MAX_PREVIEW_RESULTS)
    }

    pub fn capped_context_before(&self) -> usize {
        self.context_before.min(MAX_CONTEXT_LINES)
    }

    pub fn capped_context_after(&self) -> usize {
        self.context_after.min(MAX_CONTEXT_LINES)
    }

    pub fn effective_timeout(&self) -> Duration {
        self.timeout.unwrap_or(DEFAULT_SEARCH_TIMEOUT)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchContextLine {
    pub line_number: u64,
    pub line_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub path: String,
    pub absolute_path: String,
    pub line_number: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line_number: Option<u64>,
    pub line_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_text: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub before_context: Vec<SearchContextLine>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub after_context: Vec<SearchContextLine>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFileMatch {
    pub path: String,
    pub absolute_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFileCount {
    pub path: String,
    pub absolute_path: String,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct LocalSearchOutcome {
    pub query: String,
    pub output_mode: SearchOutputMode,
    pub results: Vec<SearchMatch>,
    pub files: Vec<SearchFileMatch>,
    pub file_counts: Vec<SearchFileCount>,
    pub total_matches: usize,
    pub total_files: usize,
    pub shown_count: usize,
    pub truncated: bool,
    pub completed: bool,
    pub cancelled: bool,
    pub timed_out: bool,
    pub partial: bool,
    pub elapsed_ms: u64,
    pub searched_files: usize,
}

#[derive(Debug, Clone, Default)]
pub struct LocalSearchCancellation {
    cancelled: Arc<AtomicBool>,
}

impl LocalSearchCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone)]
pub struct LocalSearchBatch {
    pub output_mode: SearchOutputMode,
    pub results: Vec<SearchMatch>,
    pub files: Vec<SearchFileMatch>,
    pub file_counts: Vec<SearchFileCount>,
    pub count: usize,
    pub total_matches: usize,
    pub total_files: usize,
    pub searched_files: usize,
}

pub struct LocalSearchStream {
    pub receiver: mpsc::Receiver<LocalSearchBatch>,
    join_handle: JoinHandle<Result<LocalSearchOutcome>>,
}

impl LocalSearchStream {
    pub async fn finish(self) -> Result<LocalSearchOutcome> {
        self.join_handle
            .await
            .context("local search task failed to join")?
    }
}

pub async fn run_local_search(request: LocalSearchRequest) -> Result<LocalSearchOutcome> {
    stream_local_search(request).finish().await
}

pub fn stream_local_search(request: LocalSearchRequest) -> LocalSearchStream {
    let (tx, rx) = mpsc::channel(1);
    let join_handle = tokio::task::spawn_blocking(move || {
        run_local_search_blocking(request, |batch| {
            let _ = tx.blocking_send(batch);
        })
    });

    LocalSearchStream {
        receiver: rx,
        join_handle,
    }
}

fn run_local_search_blocking<F>(
    request: LocalSearchRequest,
    mut on_batch: F,
) -> Result<LocalSearchOutcome>
where
    F: FnMut(LocalSearchBatch),
{
    if request
        .cancellation
        .as_ref()
        .is_some_and(LocalSearchCancellation::is_cancelled)
    {
        return Ok(empty_outcome(
            request.query,
            request.output_mode,
            true,
            false,
            0,
        ));
    }

    let matcher = build_regex_matcher(
        &request.query,
        request.query_mode,
        request.case_insensitive,
        request.multiline,
    )?;
    let file_matcher = compile_file_pattern(request.file_pattern.as_deref())?;
    let file_type_matcher = compile_file_type(request.file_type.as_deref())?;
    let context_before = request.capped_context_before();
    let context_after = request.capped_context_after();
    let deadline = SearchDeadline::new(request.effective_timeout());
    let mut collector = SearchCollector::new(&request);
    let mut timed_out = false;
    let mut cancelled = false;

    if request.search_root.is_file() {
        cancelled = request
            .cancellation
            .as_ref()
            .is_some_and(LocalSearchCancellation::is_cancelled);
        timed_out = deadline.is_expired();

        if !cancelled && !timed_out {
            collector.files_scanned += 1;
            collector = search_file(
                &request.workspace_root,
                &request.search_root,
                &request.search_root,
                &matcher,
                file_matcher.as_ref(),
                file_type_matcher.as_ref(),
                collector,
                request.output_mode,
                request.multiline,
                context_before,
                context_after,
            )?;

            if let Some(batch) = collector.take_pending_batch() {
                on_batch(batch);
            }
        }
    } else {
        for entry in build_candidate_walk(&request.search_root, &request.workspace_root) {
            if request
                .cancellation
                .as_ref()
                .is_some_and(LocalSearchCancellation::is_cancelled)
            {
                cancelled = true;
                break;
            }

            if deadline.is_expired() {
                timed_out = true;
                break;
            }

            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };

            let Some(file_type) = entry.file_type() else {
                continue;
            };

            if !file_type.is_file() {
                continue;
            }

            collector.files_scanned += 1;
            collector = search_file(
                &request.workspace_root,
                &request.search_root,
                &entry.into_path(),
                &matcher,
                file_matcher.as_ref(),
                file_type_matcher.as_ref(),
                collector,
                request.output_mode,
                request.multiline,
                context_before,
                context_after,
            )?;

            if let Some(batch) = collector.take_pending_batch() {
                on_batch(batch);
            }
        }
    }

    Ok(collector.finish(
        request.query,
        request.output_mode,
        deadline.elapsed_ms(),
        !timed_out && !cancelled,
        cancelled,
        timed_out,
    ))
}

fn build_candidate_walk(search_root: &Path, workspace_root: &Path) -> ignore::Walk {
    let root_for_filter = search_root.to_path_buf();
    let workspace_root = workspace_root.to_path_buf();
    let mut walk = WalkBuilder::new(search_root);
    walk.hidden(true)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(true)
        .follow_links(false)
        .sort_by_file_name(compare_entry_names_case_insensitive)
        .filter_entry(move |entry| should_descend(entry.path(), &root_for_filter, &workspace_root));
    walk.build()
}

fn compare_entry_names_case_insensitive(left: &OsStr, right: &OsStr) -> CmpOrdering {
    let left = left.to_string_lossy();
    let right = right.to_string_lossy();

    left.to_ascii_lowercase()
        .cmp(&right.to_ascii_lowercase())
        .then_with(|| left.cmp(&right))
}

fn should_descend(path: &Path, search_root: &Path, workspace_root: &Path) -> bool {
    if path == search_root {
        return true;
    }

    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };

    if NOISY_DIRECTORIES_TO_EXCLUDE
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
    {
        return false;
    }

    // Exclude a few common workspace-adjacent caches that add noise when the
    // user searches the repo root, while still allowing explicit searches in
    // those directories by preserving the chosen search_root above.
    let relative = normalized_relative_path(path, workspace_root);
    !matches!(
        relative.as_deref(),
        Some(".yarn") | Some(".pnpm-store") | Some(".cargo") | Some(".gradle") | Some(".m2")
    )
}

fn search_file(
    workspace_root: &Path,
    search_root: &Path,
    file_path: &Path,
    matcher: &regex::Regex,
    file_matcher: Option<&CompiledFileMatcher>,
    file_type_matcher: Option<&CompiledFileType>,
    mut collector: SearchCollector,
    output_mode: SearchOutputMode,
    multiline: bool,
    context_before: usize,
    context_after: usize,
) -> Result<SearchCollector> {
    if !matches_file_pattern(file_matcher, workspace_root, search_root, file_path)
        || !matches_file_type(file_type_matcher, file_path)
    {
        return Ok(collector);
    }

    let metadata = match fs::metadata(file_path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(collector),
    };

    if metadata.len() > MAX_SEARCHABLE_FILE_BYTES {
        return Ok(collector);
    }

    let bytes = match fs::read(file_path) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(collector),
    };

    let Some(content) = decode_text_contents(&bytes) else {
        return Ok(collector);
    };

    let lines = collect_lines(&content);
    if lines.is_empty() {
        return Ok(collector);
    }

    let display_path = display_path_for(file_path, workspace_root);
    let absolute_path = file_path.to_string_lossy().to_string();

    match output_mode {
        SearchOutputMode::Content => {
            let matches = collect_content_matches(
                &content,
                &lines,
                matcher,
                multiline,
                context_before,
                context_after,
            );
            if matches.is_empty() {
                return Ok(collector);
            }

            collector.record_file_match(display_path, absolute_path, matches);
        }
        SearchOutputMode::FilesWithMatches | SearchOutputMode::Count => {
            let match_count = count_search_matches(&content, &lines, matcher, multiline);
            if match_count == 0 {
                return Ok(collector);
            }

            collector.record_file_match_count(display_path, absolute_path, match_count);
        }
    }

    Ok(collector)
}

#[derive(Debug, Clone)]
struct SearchLine<'a> {
    text: &'a str,
}

fn collect_lines(content: &str) -> Vec<SearchLine<'_>> {
    if content.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    for segment in content.split_inclusive('\n') {
        let text = segment.strip_suffix('\n').unwrap_or(segment);
        let text = text.strip_suffix('\r').unwrap_or(text);
        lines.push(SearchLine { text });
    }

    if !content.ends_with('\n') && !content.is_empty() && lines.is_empty() {
        lines.push(SearchLine { text: content });
    }

    lines
}

fn collect_content_matches(
    content: &str,
    lines: &[SearchLine<'_>],
    matcher: &regex::Regex,
    multiline: bool,
    context_before: usize,
    context_after: usize,
) -> Vec<SearchMatch> {
    if multiline {
        collect_multiline_matches(content, lines, matcher, context_before, context_after)
    } else {
        collect_line_matches(lines, matcher, context_before, context_after)
    }
}

fn count_search_matches(
    content: &str,
    lines: &[SearchLine<'_>],
    matcher: &regex::Regex,
    multiline: bool,
) -> usize {
    if multiline {
        count_multiline_matches(content, lines, matcher)
    } else {
        count_line_matches(lines, matcher)
    }
}

fn collect_line_matches(
    lines: &[SearchLine<'_>],
    matcher: &regex::Regex,
    context_before: usize,
    context_after: usize,
) -> Vec<SearchMatch> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            matcher.is_match(line.text).then(|| SearchMatch {
                path: String::new(),
                absolute_path: String::new(),
                line_number: (line_index + 1) as u64,
                end_line_number: None,
                line_text: sanitize_line(line.text),
                match_text: None,
                before_context: collect_context_before(lines, line_index, context_before),
                after_context: collect_context_after(lines, line_index, context_after),
            })
        })
        .collect()
}

fn collect_multiline_matches(
    _content: &str,
    lines: &[SearchLine<'_>],
    matcher: &regex::Regex,
    context_before: usize,
    context_after: usize,
) -> Vec<SearchMatch> {
    let normalized_content = build_normalized_multiline_content(lines);
    let normalized_starts = build_normalized_line_start_offsets(lines);

    matcher
        .find_iter(&normalized_content)
        .filter_map(|matched| {
            if matched.start() == matched.end() {
                return None;
            }

            let start_line_index =
                line_index_for_start_offsets(&normalized_starts, matched.start());
            let end_offset = matched.end().saturating_sub(1);
            let end_line_index = line_index_for_start_offsets(&normalized_starts, end_offset);
            let start_line = lines.get(start_line_index)?;
            let end_line_number =
                (end_line_index != start_line_index).then_some((end_line_index + 1) as u64);

            Some(SearchMatch {
                path: String::new(),
                absolute_path: String::new(),
                line_number: (start_line_index + 1) as u64,
                end_line_number,
                line_text: sanitize_line(start_line.text),
                match_text: Some(sanitize_match_text(
                    &normalized_content[matched.start()..matched.end()],
                )),
                before_context: collect_context_before(lines, start_line_index, context_before),
                after_context: collect_context_after(lines, end_line_index, context_after),
            })
        })
        .collect()
}

fn count_line_matches(lines: &[SearchLine<'_>], matcher: &regex::Regex) -> usize {
    lines
        .iter()
        .filter(|line| matcher.is_match(line.text))
        .count()
}

fn count_multiline_matches(
    _content: &str,
    lines: &[SearchLine<'_>],
    matcher: &regex::Regex,
) -> usize {
    let normalized_content = build_normalized_multiline_content(lines);
    matcher
        .find_iter(&normalized_content)
        .filter(|matched| matched.start() != matched.end())
        .count()
}

fn line_index_for_start_offsets(starts: &[usize], offset: usize) -> usize {
    match starts.binary_search(&offset) {
        Ok(index) => index,
        Err(index) => index.saturating_sub(1),
    }
}

fn build_normalized_multiline_content(lines: &[SearchLine<'_>]) -> String {
    let mut content = String::new();

    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            content.push('\n');
        }
        content.push_str(line.text);
    }

    content
}

fn build_normalized_line_start_offsets(lines: &[SearchLine<'_>]) -> Vec<usize> {
    let mut starts = Vec::with_capacity(lines.len());
    let mut offset = 0;

    for (index, line) in lines.iter().enumerate() {
        starts.push(offset);
        offset += line.text.len();
        if index + 1 < lines.len() {
            offset += 1;
        }
    }

    starts
}

fn decode_text_contents(bytes: &[u8]) -> Option<String> {
    const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
    const UTF16_LE_BOM: &[u8] = &[0xFF, 0xFE];
    const UTF16_BE_BOM: &[u8] = &[0xFE, 0xFF];
    const UTF32_LE_BOM: &[u8] = &[0xFF, 0xFE, 0x00, 0x00];
    const UTF32_BE_BOM: &[u8] = &[0x00, 0x00, 0xFE, 0xFF];

    if bytes.starts_with(UTF32_LE_BOM) {
        return decode_utf32(&bytes[4..], true);
    }
    if bytes.starts_with(UTF32_BE_BOM) {
        return decode_utf32(&bytes[4..], false);
    }
    if bytes.starts_with(UTF16_LE_BOM) {
        return decode_utf16(&bytes[2..], true);
    }
    if bytes.starts_with(UTF16_BE_BOM) {
        return decode_utf16(&bytes[2..], false);
    }
    if bytes.starts_with(UTF8_BOM) {
        return Some(String::from_utf8_lossy(&bytes[3..]).into_owned());
    }

    if bytes.contains(&0) {
        return decode_utf16_without_bom(bytes);
    }

    Some(String::from_utf8_lossy(bytes).into_owned())
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> Option<String> {
    let chunks = bytes.chunks_exact(2);
    if !chunks.remainder().is_empty() {
        return None;
    }

    let units = chunks
        .map(|chunk| {
            if little_endian {
                u16::from_le_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_be_bytes([chunk[0], chunk[1]])
            }
        })
        .collect::<Vec<_>>();

    String::from_utf16(&units).ok()
}

fn decode_utf32(bytes: &[u8], little_endian: bool) -> Option<String> {
    let chunks = bytes.chunks_exact(4);
    if !chunks.remainder().is_empty() {
        return None;
    }

    let mut output = String::new();
    for chunk in chunks {
        let value = if little_endian {
            u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
        } else {
            u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
        };
        let character = char::from_u32(value)?;
        output.push(character);
    }

    Some(output)
}

fn decode_utf16_without_bom(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 4 || bytes.len() % 2 != 0 {
        return None;
    }

    let pair_count = bytes.len() / 2;
    let even_zeros = bytes.iter().step_by(2).filter(|&&byte| byte == 0).count();
    let odd_zeros = bytes
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|&&byte| byte == 0)
        .count();

    if odd_zeros * 4 >= pair_count && even_zeros * 8 <= pair_count {
        return decode_utf16(bytes, true);
    }

    if even_zeros * 4 >= pair_count && odd_zeros * 8 <= pair_count {
        return decode_utf16(bytes, false);
    }

    None
}

fn build_regex_matcher(
    query: &str,
    query_mode: SearchQueryMode,
    case_insensitive: bool,
    multiline: bool,
) -> Result<regex::Regex> {
    let pattern = match query_mode {
        SearchQueryMode::Literal => regex::escape(query),
        SearchQueryMode::Regex => query.to_string(),
    };

    RegexBuilder::new(&pattern)
        .case_insensitive(case_insensitive)
        .multi_line(multiline)
        .dot_matches_new_line(multiline)
        .build()
        .with_context(|| match query_mode {
            SearchQueryMode::Literal => "failed to build literal search matcher".to_string(),
            SearchQueryMode::Regex => format!("invalid regex pattern: {query}"),
        })
}

#[derive(Debug, Clone)]
struct CompiledFileMatcher {
    matcher: GlobMatcher,
    original_pattern: String,
}

fn compile_file_pattern(pattern: Option<&str>) -> Result<Option<CompiledFileMatcher>> {
    let Some(pattern) = normalize_file_pattern(pattern) else {
        return Ok(None);
    };

    let glob = Glob::new(pattern)
        .with_context(|| format!("invalid filePattern glob: {pattern}"))?
        .compile_matcher();

    Ok(Some(CompiledFileMatcher {
        matcher: glob,
        original_pattern: pattern.to_string(),
    }))
}

#[derive(Debug, Clone)]
struct CompiledFileType {
    extensions: &'static [&'static str],
    file_names: &'static [&'static str],
}

fn compile_file_type(file_type: Option<&str>) -> Result<Option<CompiledFileType>> {
    let Some(raw) = file_type.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let normalized = raw.to_ascii_lowercase();
    let (_canonical_name, extensions, file_names) = supported_file_type(&normalized)
        .ok_or_else(|| anyhow!("unsupported type filter '{raw}'"))?;

    Ok(Some(CompiledFileType {
        extensions,
        file_names,
    }))
}

fn supported_file_type(
    normalized: &str,
) -> Option<(
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
)> {
    let value = match normalized {
        "rust" | "rs" => ("rust", &["rs"][..], &[][..]),
        "typescript" | "ts" => ("typescript", &["ts", "tsx", "mts", "cts"][..], &[][..]),
        "javascript" | "js" => ("javascript", &["js", "jsx", "mjs", "cjs"][..], &[][..]),
        "python" | "py" => ("python", &["py"][..], &[][..]),
        "go" => ("go", &["go"][..], &[][..]),
        "java" => ("java", &["java"][..], &[][..]),
        "json" => ("json", &["json"][..], &[][..]),
        "yaml" | "yml" => ("yaml", &["yaml", "yml"][..], &[][..]),
        "toml" => ("toml", &["toml"][..], &[][..]),
        "markdown" | "md" => ("markdown", &["md", "mdx"][..], &[][..]),
        "css" => ("css", &["css", "scss", "sass", "less"][..], &[][..]),
        "html" => ("html", &["html", "htm"][..], &[][..]),
        "sql" => ("sql", &["sql"][..], &[][..]),
        "shell" | "sh" | "bash" => (
            "shell",
            &["sh", "bash", "zsh", "fish", "ksh"][..],
            &[".bashrc", ".zshrc"][..],
        ),
        "powershell" | "ps" | "ps1" => ("powershell", &["ps1", "psm1", "psd1"][..], &[][..]),
        "c" => ("c", &["c", "h"][..], &[][..]),
        "cpp" | "c++" | "cc" => (
            "cpp",
            &["cc", "cpp", "cxx", "hpp", "hh", "hxx"][..],
            &[][..],
        ),
        "swift" => ("swift", &["swift"][..], &[][..]),
        "kotlin" | "kt" => ("kotlin", &["kt", "kts"][..], &[][..]),
        "ruby" | "rb" => ("ruby", &["rb"][..], &["Gemfile", "Rakefile"][..]),
        "php" => ("php", &["php"][..], &[][..]),
        "docker" => ("docker", &[][..], &["Dockerfile", "Containerfile"][..]),
        _ => return None,
    };

    Some(value)
}

fn matches_file_type(matcher: Option<&CompiledFileType>, file_path: &Path) -> bool {
    let Some(matcher) = matcher else {
        return true;
    };

    let file_name = file_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if matcher
        .file_names
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(file_name))
    {
        return true;
    }

    let extension = file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    matcher
        .extensions
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
}

fn matches_file_pattern(
    matcher: Option<&CompiledFileMatcher>,
    workspace_root: &Path,
    search_root: &Path,
    file_path: &Path,
) -> bool {
    let Some(matcher) = matcher else {
        return true;
    };

    let workspace_relative = normalized_relative_path(file_path, workspace_root);
    let search_relative = normalized_relative_path(file_path, search_root);
    let file_name = file_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string());

    let mut candidates = Vec::new();

    if matcher.original_pattern.contains('/') || matcher.original_pattern.contains('\\') {
        if let Some(path) = workspace_relative.as_deref() {
            candidates.push(path);
        }
        if let Some(path) = search_relative.as_deref() {
            candidates.push(path);
        }
    } else {
        if let Some(name) = file_name.as_deref() {
            candidates.push(name);
        }
        if let Some(path) = search_relative.as_deref() {
            candidates.push(path);
        }
        if let Some(path) = workspace_relative.as_deref() {
            candidates.push(path);
        }
    }

    candidates
        .into_iter()
        .any(|candidate| matcher.matcher.is_match(candidate))
}

fn normalized_relative_path(path: &Path, base: &Path) -> Option<String> {
    path.strip_prefix(base)
        .ok()
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
}

fn display_path_for(path: &Path, workspace_root: &Path) -> String {
    normalized_relative_path(path, workspace_root)
        .unwrap_or_else(|| path.to_string_lossy().replace('\\', "/"))
}

pub fn normalize_file_pattern(pattern: Option<&str>) -> Option<&str> {
    let trimmed = pattern?.trim();
    if trimmed.is_empty() || is_noop_file_pattern(trimmed) {
        None
    } else {
        Some(trimmed)
    }
}

pub fn is_noop_file_pattern(pattern: &str) -> bool {
    matches!(pattern.trim(), "*" | "**" | "**/*" | "./*" | "./**/*")
}

#[derive(Debug, Clone)]
struct SearchCollector {
    output_mode: SearchOutputMode,
    offset: usize,
    max_results: usize,
    total_matches: usize,
    total_files: usize,
    files_scanned: usize,
    results: Vec<SearchMatch>,
    files: Vec<SearchFileMatch>,
    file_counts: Vec<SearchFileCount>,
    pending_results: Vec<SearchMatch>,
    pending_files: Vec<SearchFileMatch>,
    pending_file_counts: Vec<SearchFileCount>,
}

impl SearchCollector {
    fn new(request: &LocalSearchRequest) -> Self {
        Self {
            output_mode: request.output_mode,
            offset: request.offset,
            max_results: request.capped_max_results(),
            total_matches: 0,
            total_files: 0,
            files_scanned: 0,
            results: Vec::new(),
            files: Vec::new(),
            file_counts: Vec::new(),
            pending_results: Vec::new(),
            pending_files: Vec::new(),
            pending_file_counts: Vec::new(),
        }
    }

    fn record_file_match(
        &mut self,
        display_path: String,
        absolute_path: String,
        matches: Vec<SearchMatch>,
    ) {
        let file_match_count = matches.len();
        let match_start_index = self.total_matches;
        self.total_matches += file_match_count;
        self.total_files += 1;

        match self.output_mode {
            SearchOutputMode::Content => {
                for (match_offset, search_match) in matches.into_iter().enumerate() {
                    let ordinal = match_start_index + match_offset;
                    if ordinal < self.offset {
                        continue;
                    }
                    if self.results.len() >= self.max_results {
                        continue;
                    }

                    let search_match = SearchMatch {
                        path: display_path.clone(),
                        absolute_path: absolute_path.clone(),
                        ..search_match
                    };
                    self.pending_results.push(search_match.clone());
                    self.results.push(search_match);
                }
            }
            SearchOutputMode::FilesWithMatches => {
                let ordinal = self.total_files - 1;
                if ordinal < self.offset || self.files.len() >= self.max_results {
                    return;
                }
                let file = SearchFileMatch {
                    path: display_path,
                    absolute_path,
                };
                self.pending_files.push(file.clone());
                self.files.push(file);
            }
            SearchOutputMode::Count => {
                let ordinal = self.total_files - 1;
                if ordinal < self.offset || self.file_counts.len() >= self.max_results {
                    return;
                }
                let file_count = SearchFileCount {
                    path: display_path,
                    absolute_path,
                    count: file_match_count,
                };
                self.pending_file_counts.push(file_count.clone());
                self.file_counts.push(file_count);
            }
        }
    }

    fn record_file_match_count(
        &mut self,
        display_path: String,
        absolute_path: String,
        file_match_count: usize,
    ) {
        self.total_matches += file_match_count;
        self.total_files += 1;

        match self.output_mode {
            SearchOutputMode::Content => {}
            SearchOutputMode::FilesWithMatches => {
                let ordinal = self.total_files - 1;
                if ordinal < self.offset || self.files.len() >= self.max_results {
                    return;
                }
                let file = SearchFileMatch {
                    path: display_path,
                    absolute_path,
                };
                self.pending_files.push(file.clone());
                self.files.push(file);
            }
            SearchOutputMode::Count => {
                let ordinal = self.total_files - 1;
                if ordinal < self.offset || self.file_counts.len() >= self.max_results {
                    return;
                }
                let file_count = SearchFileCount {
                    path: display_path,
                    absolute_path,
                    count: file_match_count,
                };
                self.pending_file_counts.push(file_count.clone());
                self.file_counts.push(file_count);
            }
        }
    }

    fn take_pending_batch(&mut self) -> Option<LocalSearchBatch> {
        if self.pending_results.is_empty()
            && self.pending_files.is_empty()
            && self.pending_file_counts.is_empty()
        {
            return None;
        }

        Some(LocalSearchBatch {
            output_mode: self.output_mode,
            results: std::mem::take(&mut self.pending_results),
            files: std::mem::take(&mut self.pending_files),
            file_counts: std::mem::take(&mut self.pending_file_counts),
            count: match self.output_mode {
                SearchOutputMode::Content => self.results.len(),
                SearchOutputMode::FilesWithMatches => self.files.len(),
                SearchOutputMode::Count => self.file_counts.len(),
            },
            total_matches: self.total_matches,
            total_files: self.total_files,
            searched_files: self.files_scanned,
        })
    }

    fn finish(
        self,
        query: String,
        output_mode: SearchOutputMode,
        elapsed_ms: u64,
        completed: bool,
        cancelled: bool,
        timed_out: bool,
    ) -> LocalSearchOutcome {
        let shown_count = match output_mode {
            SearchOutputMode::Content => self.results.len(),
            SearchOutputMode::FilesWithMatches => self.files.len(),
            SearchOutputMode::Count => self.file_counts.len(),
        };
        let total_units = match output_mode {
            SearchOutputMode::Content => self.total_matches,
            SearchOutputMode::FilesWithMatches | SearchOutputMode::Count => self.total_files,
        };
        let truncated = total_units > self.offset + shown_count;

        LocalSearchOutcome {
            query,
            output_mode,
            results: self.results,
            files: self.files,
            file_counts: self.file_counts,
            total_matches: self.total_matches,
            total_files: self.total_files,
            shown_count,
            truncated,
            completed,
            cancelled,
            timed_out,
            partial: timed_out || truncated || cancelled,
            elapsed_ms,
            searched_files: self.files_scanned,
        }
    }
}

fn empty_outcome(
    query: String,
    output_mode: SearchOutputMode,
    cancelled: bool,
    timed_out: bool,
    elapsed_ms: u64,
) -> LocalSearchOutcome {
    LocalSearchOutcome {
        query,
        output_mode,
        results: Vec::new(),
        files: Vec::new(),
        file_counts: Vec::new(),
        total_matches: 0,
        total_files: 0,
        shown_count: 0,
        truncated: false,
        completed: false,
        cancelled,
        timed_out,
        partial: cancelled || timed_out,
        elapsed_ms,
        searched_files: 0,
    }
}

fn collect_context_before(
    lines: &[SearchLine<'_>],
    line_index: usize,
    context_before: usize,
) -> Vec<SearchContextLine> {
    if context_before == 0 {
        return Vec::new();
    }

    let start = line_index.saturating_sub(context_before);
    lines[start..line_index]
        .iter()
        .enumerate()
        .map(|(offset, line)| SearchContextLine {
            line_number: (start + offset + 1) as u64,
            line_text: sanitize_line(line.text),
        })
        .collect()
}

fn collect_context_after(
    lines: &[SearchLine<'_>],
    end_line_index: usize,
    context_after: usize,
) -> Vec<SearchContextLine> {
    if context_after == 0 {
        return Vec::new();
    }

    let start = end_line_index + 1;
    let end = (end_line_index + 1 + context_after).min(lines.len());
    lines[start..end]
        .iter()
        .enumerate()
        .map(|(offset, line)| SearchContextLine {
            line_number: (start + offset + 1) as u64,
            line_text: sanitize_line(line.text),
        })
        .collect()
}

fn sanitize_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() <= MAX_LINE_PREVIEW_CHARS {
        trimmed.to_string()
    } else {
        let mut preview = trimmed
            .chars()
            .take(MAX_LINE_PREVIEW_CHARS)
            .collect::<String>();
        preview.push_str("... [truncated]");
        preview
    }
}

fn sanitize_match_text(text: &str) -> String {
    let trimmed = text.trim_matches(|ch| ch == '\r' || ch == '\n');
    if trimmed.chars().count() <= MAX_MATCH_PREVIEW_CHARS {
        trimmed.to_string()
    } else {
        let mut preview = trimmed
            .chars()
            .take(MAX_MATCH_PREVIEW_CHARS)
            .collect::<String>();
        preview.push_str("... [truncated]");
        preview
    }
}

#[derive(Debug, Clone, Copy)]
struct SearchDeadline {
    started_at: Instant,
    timeout: Duration,
}

impl SearchDeadline {
    fn new(timeout: Duration) -> Self {
        Self {
            started_at: Instant::now(),
            timeout,
        }
    }

    fn is_expired(self) -> bool {
        self.started_at.elapsed() >= self.timeout
    }

    fn elapsed_ms(self) -> u64 {
        self.started_at
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_mode_treats_regex_metacharacters_as_plain_text() {
        let regex = build_regex_matcher("warn!(", SearchQueryMode::Literal, false, false).unwrap();
        assert!(regex.is_match("warn!(\"hello\")"));
    }

    #[test]
    fn regex_mode_supports_regular_expressions() {
        let regex =
            build_regex_matcher("warn!\\(.+hello", SearchQueryMode::Regex, false, false).unwrap();
        assert!(regex.is_match("warn!(\"hello\")"));
    }

    #[test]
    fn normalize_file_pattern_drops_wildcard_only_values() {
        assert_eq!(normalize_file_pattern(Some("*")), None);
        assert_eq!(normalize_file_pattern(Some(" **/* ")), None);
        assert_eq!(normalize_file_pattern(Some("")), None);
        assert_eq!(normalize_file_pattern(Some("*.rs")), Some("*.rs"));
    }

    #[test]
    fn basename_glob_matches_nested_file_name() {
        let matcher = compile_file_pattern(Some("*.rs")).unwrap().unwrap();
        let file = PathBuf::from("/workspace/src/main.rs");
        assert!(matches_file_pattern(
            Some(&matcher),
            Path::new("/workspace"),
            Path::new("/workspace"),
            &file,
        ));
    }

    #[test]
    fn path_glob_matches_workspace_relative_path() {
        let matcher = compile_file_pattern(Some("src/**/*.ts")).unwrap().unwrap();
        let file = PathBuf::from("/workspace/src/components/widget.ts");
        assert!(matches_file_pattern(
            Some(&matcher),
            Path::new("/workspace"),
            Path::new("/workspace/src"),
            &file,
        ));
    }

    #[test]
    fn file_type_filter_matches_alias_extensions() {
        let matcher = compile_file_type(Some("ts")).unwrap().unwrap();
        assert!(matches_file_type(
            Some(&matcher),
            Path::new("/workspace/src/component.tsx"),
        ));
        assert!(!matches_file_type(
            Some(&matcher),
            Path::new("/workspace/src/component.rs"),
        ));
    }

    #[test]
    fn context_helpers_capture_requested_lines() {
        let lines = vec!["one", "two", "three", "four"];
        let lines = lines
            .iter()
            .map(|line| SearchLine { text: line })
            .collect::<Vec<_>>();
        let before = collect_context_before(&lines, 2, 2);
        let after = collect_context_after(&lines, 1, 2);
        assert_eq!(before.len(), 2);
        assert_eq!(before[0].line_number, 1);
        assert_eq!(after.len(), 2);
        assert_eq!(after[1].line_number, 4);
    }

    #[test]
    fn multiline_mode_returns_match_text_and_end_line() {
        let content = "const query = `\nSELECT *\nFROM users\n`;\n";
        let lines = collect_lines(content);
        let regex = build_regex_matcher(
            "SELECT \\*\\nFROM users",
            SearchQueryMode::Regex,
            false,
            true,
        )
        .unwrap();

        let matches = collect_content_matches(content, &lines, &regex, true, 0, 1);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line_number, 2);
        assert_eq!(matches[0].end_line_number, Some(3));
        assert_eq!(
            matches[0].match_text.as_deref(),
            Some("SELECT *\nFROM users")
        );
        assert_eq!(matches[0].after_context[0].line_number, 4);
    }

    #[test]
    fn multiline_mode_normalizes_crlf_line_endings() {
        let content = "const query = `\r\nSELECT *\r\nFROM users\r\n`;\r\n";
        let lines = collect_lines(content);
        let regex = build_regex_matcher(
            "SELECT \\*\\nFROM users",
            SearchQueryMode::Regex,
            false,
            true,
        )
        .unwrap();

        let matches = collect_content_matches(content, &lines, &regex, true, 0, 0);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line_number, 2);
        assert_eq!(matches[0].end_line_number, Some(3));
        assert_eq!(
            matches[0].match_text.as_deref(),
            Some("SELECT *\nFROM users")
        );
    }

    #[test]
    fn decodes_utf16le_bom_content() {
        let bytes = vec![0xFF, 0xFE, b'h', 0x00, b'i', 0x00];
        assert_eq!(decode_text_contents(&bytes).as_deref(), Some("hi"));
    }

    #[test]
    fn count_mode_preserves_multiline_match_counts_without_building_context() {
        let content = "const query = `\nSELECT *\nFROM users\n`;\n";
        let lines = collect_lines(content);
        let regex = build_regex_matcher(
            "SELECT \\*\\nFROM users",
            SearchQueryMode::Regex,
            false,
            true,
        )
        .unwrap();

        assert_eq!(count_search_matches(content, &lines, &regex, true), 1);
    }

    #[tokio::test]
    async fn search_timeout_reports_partial_results() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(workspace.path().join("b.rs"), "fn b() {}\n").unwrap();

        let outcome = run_local_search(LocalSearchRequest {
            workspace_root: workspace.path().to_path_buf(),
            search_root: workspace.path().to_path_buf(),
            query: "fn".to_string(),
            file_pattern: None,
            file_type: None,
            query_mode: SearchQueryMode::Literal,
            output_mode: SearchOutputMode::FilesWithMatches,
            case_insensitive: false,
            multiline: false,
            context_before: 0,
            context_after: 0,
            offset: 0,
            max_results: 10,
            timeout: Some(Duration::ZERO),
            cancellation: None,
        })
        .await
        .unwrap();

        assert!(outcome.timed_out);
        assert!(!outcome.completed);
        assert!(outcome.partial);
    }

    #[tokio::test]
    async fn files_mode_is_sorted_for_stable_pagination() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("b.rs"), "hello\n").unwrap();
        std::fs::write(workspace.path().join("a.rs"), "hello\n").unwrap();

        let outcome = run_local_search(LocalSearchRequest {
            workspace_root: workspace.path().to_path_buf(),
            search_root: workspace.path().to_path_buf(),
            query: "hello".to_string(),
            file_pattern: None,
            file_type: None,
            query_mode: SearchQueryMode::Literal,
            output_mode: SearchOutputMode::FilesWithMatches,
            case_insensitive: false,
            multiline: false,
            context_before: 0,
            context_after: 0,
            offset: 1,
            max_results: 1,
            timeout: None,
            cancellation: None,
        })
        .await
        .unwrap();

        assert_eq!(outcome.files.len(), 1);
        assert_eq!(outcome.files[0].path, "b.rs");
    }

    #[tokio::test]
    async fn content_mode_is_sorted_for_stable_pagination() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(workspace.path().join("a")).unwrap();
        std::fs::write(workspace.path().join("b.rs"), "hello\n").unwrap();
        std::fs::write(workspace.path().join("a").join("a.rs"), "hello\n").unwrap();
        std::fs::write(workspace.path().join("a").join("z.rs"), "hello\n").unwrap();

        let outcome = run_local_search(LocalSearchRequest {
            workspace_root: workspace.path().to_path_buf(),
            search_root: workspace.path().to_path_buf(),
            query: "hello".to_string(),
            file_pattern: None,
            file_type: None,
            query_mode: SearchQueryMode::Literal,
            output_mode: SearchOutputMode::Content,
            case_insensitive: false,
            multiline: false,
            context_before: 0,
            context_after: 0,
            offset: 1,
            max_results: 1,
            timeout: None,
            cancellation: None,
        })
        .await
        .unwrap();

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].path, "a/z.rs");
    }
}
