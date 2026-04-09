import * as fs from "node:fs/promises";
import * as nodePath from "node:path";
import type { AgentToolResult } from "@oh-my-pi/pi-agent-core";
import { StringEnum } from "@oh-my-pi/pi-ai";
import { ChunkEditOp, type ChunkAnchorStyle, ChunkState, type EditOperation as NativeEditOperation } from "@oh-my-pi/pi-natives";
import { type Static, Type } from "@sinclair/typebox";
import type { BunFile } from "bun";
import type { WritethroughCallback, WritethroughDeferredHandle } from "../../lsp";
import { getLanguageFromPath } from "../../modes/theme/theme";
import type { ToolSession } from "../../tools";
import { assertEditableFileContent } from "../../tools/auto-generated-guard";
import { invalidateFsScanAfterWrite } from "../../tools/fs-cache-invalidation";
import { outputMeta } from "../../tools/output-meta";
import { enforcePlanModeWrite, resolvePlanPath } from "../../tools/plan-mode-guard";
import { generateUnifiedDiffString } from "../diff";
import { detectLineEnding, restoreLineEndings, stripBom } from "../normalize";
import type { EditToolDetails, LspBatchRequest } from "../renderer";
import { normalizeChunkSource, normalizeLanguage, resolveAnchorStyle } from "./chunk-state";
export {
	type ChunkCacheEntry,
	type ChunkReadTarget,
	type ParsedChunkReadPath,
	formatChunkedGrepLine,
	formatChunkedRead,
	getChunkInfoForFile,
	invalidateChunkCache,
	isChunkReadablePath,
	loadChunkStateForFile,
	missingChunkReadTarget,
	parseChunkReadPath,
	parseChunkSelector,
	resolveAnchorStyle,
} from "./chunk-state";

export type ChunkEditOperation =
	| { op: "replace"; sel?: string; content: string }
	| { op: "before"; sel?: string; content: string }
	| { op: "after"; sel?: string; content: string }
	| { op: "prepend"; sel?: string; content: string }
	| { op: "append"; sel?: string; content: string };

type ChunkEditResult = {
	diffSourceBefore: string;
	diffSourceAfter: string;
	responseText: string;
	changed: boolean;
	parseValid: boolean;
	touchedPaths: string[];
	warnings: string[];
};

type ChunkSourceContext = {
	resolvedPath: string;
	sourceFile: BunFile;
	sourceExists: boolean;
	rawContent: string;
	chunkLanguage: string | undefined;
};

async function resolveChunkSourceContext(session: ToolSession, path: string): Promise<ChunkSourceContext> {
	const resolvedPath = resolvePlanPath(session, path);
	const sourceFile = Bun.file(resolvedPath);
	const sourceExists = await sourceFile.exists();
	enforcePlanModeWrite(session, path, { op: sourceExists ? "update" : "create" });

	let rawContent = "";
	if (sourceExists) {
		rawContent = await sourceFile.text();
		assertEditableFileContent(rawContent, path);
	}

	return {
		resolvedPath,
		sourceFile,
		sourceExists,
		rawContent,
		chunkLanguage: getLanguageFromPath(resolvedPath),
	};
}

function buildChunkEditResult(result: {
	diffBefore: string;
	diffAfter: string;
	responseText: string;
	changed: boolean;
	parseValid: boolean;
	touchedPaths: string[];
	warnings: string[];
}): ChunkEditResult {
	return {
		diffSourceBefore: result.diffBefore,
		diffSourceAfter: result.diffAfter,
		responseText: result.responseText,
		changed: result.changed,
		parseValid: result.parseValid,
		touchedPaths: result.touchedPaths,
		warnings: result.warnings,
	};
}

function toNativeEditOperation(operation: ChunkEditOperation): NativeEditOperation {
	switch (operation.op) {
		case "replace":
			return {
				op: ChunkEditOp.Replace,
				sel: operation.sel,
				content: operation.content,
			};
		case "before":
			return { op: ChunkEditOp.Before, sel: operation.sel, content: operation.content };
		case "after":
			return { op: ChunkEditOp.After, sel: operation.sel, content: operation.content };
		case "prepend":
			return { op: ChunkEditOp.Prepend, sel: operation.sel, content: operation.content };
		case "append":
			return { op: ChunkEditOp.Append, sel: operation.sel, content: operation.content };
		default: {
			const exhaustive: never = operation;
			return exhaustive;
		}
	}
}

export function applyChunkEdits(params: {
	source: string;
	language?: string;
	cwd: string;
	filePath: string;
	operations: ChunkEditOperation[];
	defaultSelector?: string;
	defaultCrc?: string;
	anchorStyle?: ChunkAnchorStyle;
}): ChunkEditResult {
	const normalizedSource = normalizeChunkSource(params.source);
	const nativeOperations = params.operations.map(toNativeEditOperation);
	const state = ChunkState.parse(normalizedSource, normalizeLanguage(params.language));
	const result = state.applyEdits({
		operations: nativeOperations,
		defaultSelector: params.defaultSelector,
		defaultCrc: params.defaultCrc,
		anchorStyle: params.anchorStyle,
		cwd: params.cwd,
		filePath: params.filePath,
	});

	return buildChunkEditResult(result);
}

const CHUNK_OP_VALUES = ["replace", "after", "before", "prepend", "append"] as const;

export const chunkToolEditSchema = Type.Object({
	op: StringEnum(CHUNK_OP_VALUES),
	sel: Type.String({
		description:
			"Chunk selector. Format: 'path@region' for insertions, 'path#CRC@region' for replace. Omit @region to target the full chunk. Valid regions: head, body, tail, decl.",
	}),
	content: Type.String({
		description: "New content. Use one leading space per indent level; do not include the chunk's base padding.",
	}),
});
export const chunkEditParamsSchema = Type.Object(
	{
		path: Type.String({ description: "File path" }),
		edits: Type.Array(chunkToolEditSchema, {
			description: "Chunk edits",
			minItems: 1,
		}),
	},
	{ additionalProperties: false },
);

export type ChunkToolEdit = Static<typeof chunkToolEditSchema>;
export type ChunkParams = Static<typeof chunkEditParamsSchema>;

interface ExecuteChunkModeOptions {
	session: ToolSession;
	params: ChunkParams;
	signal?: AbortSignal;
	batchRequest?: LspBatchRequest;
	writethrough: WritethroughCallback;
	beginDeferredDiagnosticsForPath: (path: string) => WritethroughDeferredHandle;
}

export function isChunkParams(params: unknown): params is ChunkParams {
	return (
		typeof params === "object" &&
		params !== null &&
		"edits" in params &&
		Array.isArray(params.edits) &&
		params.edits.length > 0 &&
		typeof params.edits[0] === "object" &&
		params.edits[0] !== null &&
		"sel" in params.edits[0]
	);
}

function normalizeChunkEditOperations(edits: ChunkToolEdit[]): ChunkEditOperation[] {
	return edits as ChunkEditOperation[];
}

async function writeChunkResult(params: {
	result: ChunkEditResult;
	resolvedPath: string;
	sourceFile: BunFile;
	sourceText: string;
	sourceExists: boolean;
	signal?: AbortSignal;
	batchRequest?: LspBatchRequest;
	writethrough: WritethroughCallback;
	beginDeferredDiagnosticsForPath: (path: string) => WritethroughDeferredHandle;
}): Promise<AgentToolResult<EditToolDetails, typeof chunkEditParamsSchema>> {
	const {
		result,
		resolvedPath,
		sourceFile,
		sourceText,
		sourceExists,
		signal,
		batchRequest,
		writethrough,
		beginDeferredDiagnosticsForPath,
	} = params;

	const { bom, text } = stripBom(sourceText);
	const originalEnding = detectLineEnding(text);
	const finalContent = bom + restoreLineEndings(result.diffSourceAfter, originalEnding);
	const diagnostics = await writethrough(resolvedPath, finalContent, signal, sourceFile, batchRequest, dst =>
		dst === resolvedPath ? beginDeferredDiagnosticsForPath(resolvedPath) : undefined,
	);
	invalidateFsScanAfterWrite(resolvedPath);

	const diffResult = generateUnifiedDiffString(result.diffSourceBefore, result.diffSourceAfter);
	const warningsBlock = result.warnings.length > 0 ? `\n\n${result.warnings.join("\n")}` : "";
	const meta = outputMeta()
		.diagnostics(diagnostics?.summary ?? "", diagnostics?.messages ?? [])
		.get();

	return {
		content: [{ type: "text", text: `${result.responseText}${warningsBlock}` }],
		details: {
			diff: diffResult.diff,
			firstChangedLine: diffResult.firstChangedLine,
			diagnostics,
			op: sourceExists ? "update" : "create",
			meta,
		},
	};
}

export async function executeChunkMode(
	options: ExecuteChunkModeOptions,
): Promise<AgentToolResult<EditToolDetails, typeof chunkEditParamsSchema>> {
	const { session, params, signal, batchRequest, writethrough, beginDeferredDiagnosticsForPath } = options;
	const { path, edits } = params;
	const { resolvedPath, sourceFile, sourceExists, rawContent, chunkLanguage } = await resolveChunkSourceContext(
		session,
		path,
	);
	const parentDir = nodePath.dirname(resolvedPath);
	if (parentDir && parentDir !== ".") {
		await fs.mkdir(parentDir, { recursive: true });
	}
	const normalizedOperations = normalizeChunkEditOperations(edits);

	const chunkResult = applyChunkEdits({
		source: rawContent,
		language: chunkLanguage,
		cwd: session.cwd,
		filePath: resolvedPath,
		operations: normalizedOperations,
		anchorStyle: resolveAnchorStyle(session.settings),
	});

	if (!chunkResult.changed) {
		const responseText = `[No changes needed — content already matches.]\n\n${chunkResult.responseText}`;
		return {
			content: [{ type: "text", text: responseText }],
			details: {
				diff: "",
				op: sourceExists ? "update" : "create",
				meta: outputMeta().get(),
			},
		};
	}

	return writeChunkResult({
		result: chunkResult,
		resolvedPath,
		sourceFile,
		sourceText: rawContent,
		sourceExists,
		signal,
		batchRequest,
		writethrough,
		beginDeferredDiagnosticsForPath,
	});
}
