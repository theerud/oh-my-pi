#!/usr/bin/env bun
import { APP_NAME, MIN_BUN_VERSION, VERSION } from "@oh-my-pi/pi-utils";
/**
 * CLI entry point — registers all commands explicitly and delegates to the
 * lightweight CLI runner from pi-utils.
 */
import { type CommandEntry, run } from "@oh-my-pi/pi-utils/cli";
import { renderTopLevelHelp, type RootCommandSummary } from "./cli/help-text";

function parseSemver(version: string): [number, number, number] {
	function toint(value: string): number {
		const int = Number.parseInt(value, 10);
		if (Number.isNaN(int) || !Number.isFinite(int)) return 0;
		return int;
	}
	const [majorRaw, minorRaw, patchRaw] = version.split(".").map(toint);
	return [majorRaw, minorRaw, patchRaw];
}

function isAtLeastBunVersion(minimum: string): boolean {
	const ver = parseSemver(Bun.version);
	const min = parseSemver(minimum);
	for (let i = 0; i < 3; i++) {
		if (ver[i] !== min[i]) {
			return ver[i] > min[i];
		}
	}
	return true;
}

if (typeof Bun.JSONL?.parseChunk !== "function" || !isAtLeastBunVersion(MIN_BUN_VERSION)) {
	process.stderr.write(
		`error: Bun runtime must be >= ${MIN_BUN_VERSION} (found v${Bun.version}). Please update Bun: bun upgrade\n`,
	);
	process.exit(1);
}

// Detect known Bun errata that cause TUI crashes (e.g. Bun.stringWidth mishandling OSC sequences).
if (Bun.stringWidth("\x1b[0m\x1b]8;;\x07") !== 0) {
	process.stderr.write(`error: Bun runtime errata detected (v${Bun.version}). Please update Bun: bun upgrade\n`);
	process.exit(1);
}

process.title = APP_NAME;

type RegisteredCommand = CommandEntry & RootCommandSummary;

const commands: RegisteredCommand[] = [
	{ name: "launch", description: "AI coding assistant", hidden: true, load: () => import("./commands/launch").then(m => m.default) },
	{ name: "agents", description: "Manage bundled task agents", load: () => import("./commands/agents").then(m => m.default) },
	{ name: "commit", description: "Generate a commit message and update changelogs", load: () => import("./commands/commit").then(m => m.default) },
	{ name: "config", description: "Manage configuration settings", load: () => import("./commands/config").then(m => m.default) },
	{ name: "grep", description: "Test grep tool", load: () => import("./commands/grep").then(m => m.default) },
	{ name: "grievances", description: "View reported tool issues (auto-QA grievances)", load: () => import("./commands/grievances").then(m => m.default) },
	{ name: "read", description: "Read a file as a chunk tree", load: () => import("./commands/read").then(m => m.default) },
	{ name: "jupyter", description: "Manage the shared Jupyter gateway", load: () => import("./commands/jupyter").then(m => m.default) },
	{ name: "plugin", description: "Manage plugins (install, uninstall, list, etc.)", load: () => import("./commands/plugin").then(m => m.default) },
	{ name: "setup", description: "Install dependencies for optional features", load: () => import("./commands/setup").then(m => m.default) },
	{ name: "shell", description: "Interactive shell console", load: () => import("./commands/shell").then(m => m.default) },
	{ name: "ssh", description: "Manage SSH host configurations", load: () => import("./commands/ssh").then(m => m.default) },
	{ name: "stats", description: "View usage statistics", load: () => import("./commands/stats").then(m => m.default) },
	{ name: "update", description: "Check for and install updates", load: () => import("./commands/update").then(m => m.default) },
	{ name: "search", description: "Test web search providers", aliases: ["q"], load: () => import("./commands/web-search").then(m => m.default) },
];

/**
 * Determine whether argv[0] is a known subcommand name.
 * If not, the entire argv is treated as args to the default "launch" command.
 */
function isSubcommand(first: string | undefined): boolean {
	if (!first || first.startsWith("-") || first.startsWith("@")) return false;
	return commands.some(e => e.name === first || e.aliases?.includes(first));
}

/** Run the CLI with the given argv (no `process.argv` prefix). */
export function runCli(argv: string[]): Promise<void> {
	const first = argv[0];
	if (first === "--help" || first === "-h" || first === "help") {
		renderTopLevelHelp(APP_NAME, VERSION, commands);
		return Promise.resolve();
	}
	const runArgv =
		first === "--version" || first === "-v" ? argv : isSubcommand(first) ? argv : ["launch", ...argv];
	return run({ bin: APP_NAME, version: VERSION, argv: runArgv, commands });
}

try {
	await runCli(process.argv.slice(2));
} catch (error) {
	const message = error instanceof Error ? (error.stack ?? error.message) : String(error);
	process.stderr.write(`${message}\n`);
	process.exit(1);
}
