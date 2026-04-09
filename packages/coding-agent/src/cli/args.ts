/**
 * CLI argument parsing and help display
 */
import { APP_NAME, logger } from "@oh-my-pi/pi-utils";
import { getExtraHelpText } from "./help-text";

type Effort = "minimal" | "low" | "medium" | "high" | "xhigh";

const THINKING_EFFORTS: readonly Effort[] = ["minimal", "low", "medium", "high", "xhigh"];

export type Mode = "text" | "json" | "rpc" | "acp";

export interface Args {
	cwd?: string;
	allowHome?: boolean;
	provider?: string;
	model?: string;
	smol?: string;
	slow?: string;
	plan?: string;
	apiKey?: string;
	systemPrompt?: string;
	appendSystemPrompt?: string;
	thinking?: Effort;
	continue?: boolean;
	resume?: string | true;
	help?: boolean;
	version?: boolean;
	mode?: Mode;
	noSession?: boolean;
	sessionDir?: string;
	providerSessionId?: string;
	fork?: string;
	models?: string[];
	tools?: string[];
	noTools?: boolean;
	noLsp?: boolean;
	noPty?: boolean;
	hooks?: string[];
	extensions?: string[];
	noExtensions?: boolean;
	pluginDirs?: string[];
	print?: boolean;
	export?: string;
	noSkills?: boolean;
	skills?: string[];
	noRules?: boolean;
	listModels?: string | true;
	noTitle?: boolean;
	messages: string[];
	fileArgs: string[];
	/** Unknown flags (potentially extension flags) - map of flag name to value */
	unknownFlags: Map<string, boolean | string>;
}

const CLI_TOOL_NAMES = new Set([
	"ask",
	"ast_edit",
	"ast_grep",
	"await",
	"bash",
	"browser",
	"calc",
	"cancel_job",
	"checkpoint",
	"debug",
	"edit",
	"find",
	"gh_issue_view",
	"gh_pr_checkout",
	"gh_pr_diff",
	"gh_pr_push",
	"gh_pr_view",
	"gh_repo_view",
	"gh_run_watch",
	"gh_search_issues",
	"gh_search_prs",
	"grep",
	"inspect_image",
	"lsp",
	"notebook",
	"python",
	"read",
	"render_mermaid",
	"rewind",
	"search_tool_bm25",
	"ssh",
	"task",
	"todo_write",
	"web_search",
	"write",
]);

function parseEffort(value: string | null | undefined): Effort | undefined {
	return value !== undefined && value !== null && THINKING_EFFORTS.includes(value as Effort)
		? (value as Effort)
		: undefined;
}

export function parseArgs(args: string[], extensionFlags?: Map<string, { type: "boolean" | "string" }>): Args {
	const result: Args = {
		messages: [],
		fileArgs: [],
		unknownFlags: new Map(),
	};

	for (let i = 0; i < args.length; i++) {
		const arg = args[i];

		if (arg === "--help" || arg === "-h") {
			result.help = true;
		} else if (arg === "--version" || arg === "-v") {
			result.version = true;
		} else if (arg === "--allow-home") {
			result.allowHome = true;
		} else if (arg === "--mode" && i + 1 < args.length) {
			const mode = args[++i];
			if (mode === "text" || mode === "json" || mode === "rpc" || mode === "acp") {
				result.mode = mode;
			}
		} else if (arg === "--continue" || arg === "-c") {
			result.continue = true;
		} else if (arg === "--resume" || arg === "-r" || arg === "--session") {
			const next = args[i + 1];
			if (next && !next.startsWith("-")) {
				result.resume = args[++i];
			} else {
				result.resume = true;
			}
		} else if (arg === "--fork" && i + 1 < args.length) {
			result.fork = args[++i];
		} else if (arg === "--provider" && i + 1 < args.length) {
			result.provider = args[++i];
		} else if (arg === "--model" && i + 1 < args.length) {
			result.model = args[++i];
		} else if (arg === "--smol" && i + 1 < args.length) {
			result.smol = args[++i];
		} else if (arg === "--slow" && i + 1 < args.length) {
			result.slow = args[++i];
		} else if (arg === "--plan" && i + 1 < args.length) {
			result.plan = args[++i];
		} else if (arg === "--api-key" && i + 1 < args.length) {
			result.apiKey = args[++i];
		} else if (arg === "--system-prompt" && i + 1 < args.length) {
			result.systemPrompt = args[++i];
		} else if (arg === "--append-system-prompt" && i + 1 < args.length) {
			result.appendSystemPrompt = args[++i];
		} else if (arg === "--provider-session-id" && i + 1 < args.length) {
			result.providerSessionId = args[++i];
		} else if (arg === "--no-session") {
			result.noSession = true;
		} else if (arg === "--session-dir" && i + 1 < args.length) {
			result.sessionDir = args[++i];
		} else if (arg === "--models" && i + 1 < args.length) {
			result.models = args[++i].split(",").map(s => s.trim());
		} else if (arg === "--no-tools") {
			result.noTools = true;
		} else if (arg === "--no-lsp") {
			result.noLsp = true;
		} else if (arg === "--no-pty") {
			result.noPty = true;
		} else if (arg === "--tools" && i + 1 < args.length) {
			const toolNames = args[++i]
				.split(",")
				.map(s => s.trim().toLowerCase())
				.filter(Boolean);
			const validTools: string[] = [];
			for (const name of toolNames) {
				if (CLI_TOOL_NAMES.has(name)) {
					validTools.push(name);
				} else {
					logger.warn("Unknown tool passed to --tools", {
						tool: name,
						validTools: Array.from(CLI_TOOL_NAMES).sort(),
					});
				}
			}
			result.tools = validTools;
		} else if (arg === "--thinking" && i + 1 < args.length) {
			const rawThinking = args[++i];
			const thinking = parseEffort(rawThinking);
			if (thinking !== undefined) {
				result.thinking = thinking;
			} else {
				logger.warn("Invalid thinking level passed to --thinking", {
					level: rawThinking,
					validThinkingLevels: THINKING_EFFORTS,
				});
			}
		} else if (arg === "--print" || arg === "-p") {
			result.print = true;
		} else if (arg === "--export" && i + 1 < args.length) {
			result.export = args[++i];
		} else if (arg === "--hook" && i + 1 < args.length) {
			result.hooks = result.hooks ?? [];
			result.hooks.push(args[++i]);
		} else if ((arg === "--extension" || arg === "-e") && i + 1 < args.length) {
			result.extensions = result.extensions ?? [];
			result.extensions.push(args[++i]);
		} else if (arg === "--plugin-dir" && i + 1 < args.length) {
			result.pluginDirs = result.pluginDirs ?? [];
			result.pluginDirs.push(args[++i]);
		} else if (arg === "--no-extensions") {
			result.noExtensions = true;
		} else if (arg === "--no-skills") {
			result.noSkills = true;
		} else if (arg === "--no-rules") {
			result.noRules = true;
		} else if (arg === "--no-title") {
			result.noTitle = true;
		} else if (arg === "--skills" && i + 1 < args.length) {
			// Comma-separated glob patterns for skill filtering
			result.skills = args[++i].split(",").map(s => s.trim());
		} else if (arg === "--list-models") {
			// Check if next arg is a search pattern (not a flag or file arg)
			if (i + 1 < args.length && !args[i + 1].startsWith("-") && !args[i + 1].startsWith("@")) {
				result.listModels = args[++i];
			} else {
				result.listModels = true;
			}
		} else if (arg.startsWith("@")) {
			result.fileArgs.push(arg.slice(1)); // Remove @ prefix
		} else if (arg.startsWith("--") && extensionFlags) {
			// Check if it's an extension-registered flag
			const flagName = arg.slice(2);
			const extFlag = extensionFlags.get(flagName);
			if (extFlag) {
				if (extFlag.type === "boolean") {
					result.unknownFlags.set(flagName, true);
				} else if (extFlag.type === "string" && i + 1 < args.length) {
					result.unknownFlags.set(flagName, args[++i]);
				}
			}
			// Unknown flags without extensionFlags are silently ignored (first pass)
		} else if (!arg.startsWith("-")) {
			result.messages.push(arg);
		}
	}

	return result;
}

export function printHelp(): void {
	process.stdout.write(
		`${APP_NAME} - AI coding assistant\n\n` +
			`Run ${APP_NAME} --help for full command and option details.\n` +
			`Run ${APP_NAME} <command> --help for command-specific help.\n\n` +
			`${getExtraHelpText()}\n`,
	);
}
