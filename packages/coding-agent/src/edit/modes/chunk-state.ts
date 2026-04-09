import * as nodePath from "node:path";
import { ChunkAnchorStyle, type ChunkInfo, ChunkReadStatus, type ChunkReadTarget, ChunkState } from "@oh-my-pi/pi-natives";
import { LRUCache } from "lru-cache";
import type { Settings } from "../../config/settings";
import { normalizeToLF, stripBom } from "../normalize";

export type { ChunkReadTarget };

export type ParsedChunkReadPath = {
	filePath: string;
	selector?: string;
};

export type ChunkCacheEntry = {
	mtimeMs: number;
	size: number;
	source: string;
	state: ChunkState;
};


const validAnchorStyles: Record<string, ChunkAnchorStyle> = {
	full: ChunkAnchorStyle.Full,
	kind: ChunkAnchorStyle.Kind,
	bare: ChunkAnchorStyle.Bare,
};

export function resolveAnchorStyle(settings?: Settings): ChunkAnchorStyle {
	const envStyle = Bun.env.PI_ANCHOR_STYLE;
	return (
		(envStyle && validAnchorStyles[envStyle]) ||
		(settings?.get("read.anchorstyle") as ChunkAnchorStyle | undefined) ||
		ChunkAnchorStyle.Full
	);
}

const readEnvInt = (name: string, defaultValue: number): number => {
	const value = Bun.env[name];
	if (!value) return defaultValue;
	const parsed = Number.parseInt(value, 10);
	if (Number.isNaN(parsed) || parsed <= 0) return defaultValue;
	return parsed;
};

const chunkStateCache = new LRUCache<string, ChunkCacheEntry>({
	max: readEnvInt("PI_CHUNK_CACHE_MAX_ENTRIES", 200),
});

export function invalidateChunkCache(filePath: string): void {
	chunkStateCache.delete(filePath);
}

export function normalizeLanguage(language: string | undefined): string {
	return language?.trim().toLowerCase() || "";
}

export function normalizeChunkSource(text: string): string {
	return normalizeToLF(stripBom(text).text);
}

function displayPathForFile(filePath: string, cwd: string): string {
	const relative = nodePath.relative(cwd, filePath).replace(/\\/g, "/");
	return relative && !relative.startsWith("..") ? relative : filePath.replace(/\\/g, "/");
}

function fileLanguageTag(filePath: string, language?: string): string | undefined {
	const normalizedLanguage = normalizeLanguage(language);
	if (normalizedLanguage.length > 0) return normalizedLanguage;
	const ext = nodePath.extname(filePath).replace(/^\./, "").toLowerCase();
	return ext.length > 0 ? ext : undefined;
}

function chunkReadPathSeparatorIndex(readPath: string): number {
	if (/^[a-zA-Z]:[/\\]/.test(readPath)) {
		return readPath.indexOf(":", 2);
	}
	return readPath.indexOf(":");
}

export function parseChunkSelector(selector: string | undefined): { selector?: string } {
	if (!selector || selector.length === 0) {
		return {};
	}
	return { selector };
}

export function parseChunkReadPath(readPath: string): ParsedChunkReadPath {
	const colonIndex = chunkReadPathSeparatorIndex(readPath);
	if (colonIndex === -1) {
		return { filePath: readPath };
	}
	const parsedSelector = parseChunkSelector(readPath.slice(colonIndex + 1) || undefined);
	return {
		filePath: readPath.slice(0, colonIndex),
		selector: parsedSelector.selector,
	};
}

export function isChunkReadablePath(readPath: string): boolean {
	return parseChunkReadPath(readPath).selector !== undefined;
}

export async function loadChunkStateForFile(filePath: string, language: string | undefined): Promise<ChunkCacheEntry> {
	const file = Bun.file(filePath);
	const stat = await file.stat();
	const cached = chunkStateCache.get(filePath);
	if (cached && cached.mtimeMs === stat.mtimeMs && cached.size === stat.size) {
		return cached;
	}

	const source = normalizeChunkSource(await file.text());
	const state = ChunkState.parse(source, normalizeLanguage(language));
	const entry = { mtimeMs: stat.mtimeMs, size: stat.size, source, state };
	chunkStateCache.set(filePath, entry);
	return entry;
}

export async function formatChunkedRead(params: {
	filePath: string;
	readPath: string;
	cwd: string;
	language?: string;
	omitChecksum?: boolean;
	anchorStyle?: ChunkAnchorStyle;
	absoluteLineRange?: { startLine: number; endLine?: number };
}): Promise<{ text: string; resolvedPath?: string; chunk?: ChunkReadTarget }> {
	const { filePath, readPath, cwd, language, omitChecksum = false, anchorStyle, absoluteLineRange } = params;
	const normalizedLanguage = normalizeLanguage(language);
	const { state } = await loadChunkStateForFile(filePath, normalizedLanguage);
	const displayPath = displayPathForFile(filePath, cwd);
	const result = state.renderRead({
		readPath,
		displayPath,
		languageTag: fileLanguageTag(filePath, normalizedLanguage),
		omitChecksum,
		anchorStyle,
		absoluteLineRange: absoluteLineRange
			? { startLine: absoluteLineRange.startLine, endLine: absoluteLineRange.endLine ?? absoluteLineRange.startLine }
			: undefined,
		tabReplacement: "    ",
		normalizeIndent: true,
	});
	return { text: result.text, resolvedPath: filePath, chunk: result.chunk };
}

export async function formatChunkedGrepLine(params: {
	filePath: string;
	lineNumber: number;
	line: string;
	cwd: string;
	language?: string;
}): Promise<string> {
	const { filePath, lineNumber, line, cwd, language } = params;
	const { state } = await loadChunkStateForFile(filePath, language);
	return state.formatGrepLine(displayPathForFile(filePath, cwd), lineNumber, line);
}

export async function getChunkInfoForFile(
	filePath: string,
	language: string | undefined,
	chunkPath: string,
): Promise<ChunkInfo | undefined> {
	const { state } = await loadChunkStateForFile(filePath, language);
	return state.chunk(chunkPath) ?? undefined;
}

export function missingChunkReadTarget(selector: string): ChunkReadTarget {
	return { status: ChunkReadStatus.NotFound, selector };
}
