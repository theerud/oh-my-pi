import * as fsSync from "node:fs";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { $ } from "bun";

const repoRoot = path.join(import.meta.dir, "../../..");
const rustDir = path.join(repoRoot, "crates/pi-natives");
const nativeDir = path.join(import.meta.dir, "../native");
const generatedDir = path.join(import.meta.dir, "../.generated");
const packageJsonPath = path.join(import.meta.dir, "../package.json");

const isDev = process.argv.includes("--dev");
const isLean = process.argv.includes("--lean");
const noHighlights = process.argv.includes("--no-highlights");
const noImage = process.argv.includes("--no-image");
const crossTarget = Bun.env.CROSS_TARGET;
const targetPlatform = Bun.env.TARGET_PLATFORM || process.platform;
const targetArch = Bun.env.TARGET_ARCH || process.arch;
const configuredVariantRaw = Bun.env.TARGET_VARIANT;
const isCrossCompile = Boolean(crossTarget) || targetPlatform !== process.platform || targetArch !== process.arch;
const buildFlavor = noImage && noHighlights ? "minimal" : isLean ? "lean" : "full";
const bindingsDir =
	buildFlavor === "full" ? nativeDir : path.join(generatedDir, buildFlavor);
const runtimeAddonDir = bindingsDir;

type X64Variant = "modern" | "baseline";

let configuredVariant: X64Variant | undefined;
if (configuredVariantRaw) {
	if (targetArch !== "x64") {
		throw new Error(`TARGET_VARIANT is only supported for x64 builds, got ${targetPlatform}-${targetArch}.`);
	}
	if (configuredVariantRaw !== "modern" && configuredVariantRaw !== "baseline") {
		throw new Error(`Unsupported TARGET_VARIANT: ${configuredVariantRaw}. Expected "modern" or "baseline".`);
	}
	configuredVariant = configuredVariantRaw;
}

function runCommand(command: string, args: string[]): string | null {
	try {
		const result = Bun.spawnSync([command, ...args], { stdout: "pipe", stderr: "pipe" });
		if (result.exitCode !== 0) return null;
		return result.stdout.toString("utf-8").trim();
	} catch {
		return null;
	}
}

function detectHostAvx2Support(): boolean {
	if (process.arch !== "x64") return false;

	if (process.platform === "linux") {
		try {
			const cpuInfo = fsSync.readFileSync("/proc/cpuinfo", "utf8");
			return /\bavx2\b/i.test(cpuInfo);
		} catch {
			return false;
		}
	}

	if (process.platform === "darwin") {
		const leaf7 = runCommand("sysctl", ["-n", "machdep.cpu.leaf7_features"]);
		if (leaf7 && /\bAVX2\b/i.test(leaf7)) return true;
		const features = runCommand("sysctl", ["-n", "machdep.cpu.features"]);
		return Boolean(features && /\bAVX2\b/i.test(features));
	}

	if (process.platform === "win32") {
		const output = runCommand("powershell.exe", [
			"-NoProfile",
			"-NonInteractive",
			"-Command",
			"[System.Runtime.Intrinsics.X86.Avx2]::IsSupported",
		]);
		return output?.toLowerCase() === "true";
	}

	return false;
}

function resolveEffectiveVariant(): X64Variant | null {
	if (targetArch !== "x64") return null;
	if (configuredVariant) return configuredVariant;
	if (isCrossCompile) {
		throw new Error("x64 cross-builds require TARGET_VARIANT=modern or TARGET_VARIANT=baseline.");
	}
	return detectHostAvx2Support() ? "modern" : "baseline";
}

const effectiveVariant = resolveEffectiveVariant();
const variantSuffix = effectiveVariant ? `-${effectiveVariant}` : "";

// Default to native CPU optimization for local builds; explicit variants use fixed ISA targets.
if (!isCrossCompile && !Bun.env.RUSTFLAGS) {
	if (effectiveVariant === "modern") {
		Bun.env.RUSTFLAGS = "-C target-cpu=x86-64-v3";
	} else if (effectiveVariant === "baseline") {
		Bun.env.RUSTFLAGS = "-C target-cpu=x86-64-v2";
	} else {
		Bun.env.RUSTFLAGS = "-C target-cpu=native";
	}
}

async function cleanupStaleTemps(dir: string): Promise<void> {
	try {
		const entries = await fs.readdir(dir);
		for (const entry of entries) {
			if (entry.includes(".tmp.") || entry.includes(".old.") || entry.includes(".new.")) {
				await fs.unlink(path.join(dir, entry)).catch(() => {});
			}
		}
	} catch {
		// Directory might not exist yet
	}
}

async function cleanupNativeAddons(dir: string): Promise<void> {
	try {
		const entries = await fs.readdir(dir);
		for (const entry of entries) {
			if (entry.startsWith("pi_natives.") && entry.endsWith(".node")) {
				await fs.unlink(path.join(dir, entry)).catch(() => {});
			}
		}
	} catch {
		// Directory might not exist yet
	}
}

async function installBinary(src: string, dest: string): Promise<void> {
	const tempPath = `${dest}.tmp.${process.pid}`;

	await fs.copyFile(src, tempPath);

	try {
		// Atomic rename - works even if dest is loaded on Linux/macOS (old inode stays valid)
		await fs.rename(tempPath, dest);
	} catch {
		// On Windows, loaded DLLs cannot be overwritten via rename
		// Try delete-then-rename as fallback
		try {
			await fs.unlink(dest);
		} catch (unlinkErr) {
			if ((unlinkErr as NodeJS.ErrnoException).code !== "ENOENT") {
				await fs.unlink(tempPath).catch(() => {});
				const isWindows = process.platform === "win32";
				throw new Error(
					`Cannot replace ${path.basename(dest)}${isWindows ? " (file may be in use - close any running processes)" : ""}: ${(unlinkErr as Error).message}`,
				);
			}
		}
		try {
			await fs.rename(tempPath, dest);
		} catch (finalErr) {
			await fs.unlink(tempPath).catch(() => {});
			throw new Error(`Failed to install ${path.basename(dest)}: ${(finalErr as Error).message}`);
		}
	}
}
async function patchGeneratedIndexLoader(targetDir: string): Promise<void> {
	const indexPath = path.join(targetDir, "index.js");
	let content = await Bun.file(indexPath).text();
	const modulePathPatch = [
		'const moduleFilename = typeof __filename === "string" ? __filename : process.argv[1] || path.join(process.cwd(), "omp.js");',
		'const moduleDir = typeof __dirname === "string" ? __dirname : path.dirname(moduleFilename);',
		"",
	].join("\n");
	if (!content.includes(modulePathPatch)) {
		content = content.replace(
			'const path = require("node:path");\n',
			`const path = require("node:path");\n\n${modulePathPatch}`,
		);
	}
	const embeddedLoadPatch = "let embeddedAddon = null;\n";
	if (!content.includes(embeddedLoadPatch)) {
		content = content.replace(/const \{ embeddedAddon \} = require\("\.\/embedded-addon"\);\n/, embeddedLoadPatch);
	}
	const lazyLoadPatch = [
		"if (isCompiledBinary) {",
		"\ttry {",
		'\t\t({ embeddedAddon } = require("./embedded-addon"));',
		"\t} catch {",
		"\t\tembeddedAddon = null;",
		"\t}",
		"}",
		"",
	].join("\n");
	if (!content.includes(lazyLoadPatch)) {
		content = content.replace(
			/(const isCompiledBinary =[\s\S]*?__filename\.includes\("%7EBUN"\);\n)/,
			`$1\n${lazyLoadPatch}`,
		);
	}
	content = content.replace('const require_ = createRequire(__filename);\n', "const require_ = createRequire(moduleFilename);\n");
	content = content.replace('const nativeDir = path.join(__dirname, "..", "native");\n', "");
	if (!content.includes("const nativeDirCandidates = [")) {
		content = content.replace(
			/(const addonLabel = selectedVariant \? `\$\{platformTag\} \(\$\{selectedVariant\}\)` : platformTag;\n)/,
			`$1const nativeDirCandidates = [\n\tpath.join(moduleDir, "..", "native"),\n\tpath.join(moduleDir, "native"),\n\tpath.join(moduleDir, "..", "..", "natives", "native"),\n];\n`,
		);
	}
	content = content.replace(
		/const baseReleaseCandidates = addonFilenames\.flatMap\(filename => \[\n\tpath\.join\(nativeDir, filename\),\n\tpath\.join\(execDir, filename\),\n\]\);\n/,
		'const baseReleaseCandidates = addonFilenames.flatMap(filename => [\n\t...nativeDirCandidates.map(dir => path.join(dir, filename)),\n\tpath.join(execDir, filename),\n]);\n',
	);
	content = content.replace(/__filename\.includes\(/g, "moduleFilename.includes(");
	await Bun.write(indexPath, content);
}

async function hasGeneratedIndexJs(targetDir: string): Promise<boolean> {
	try {
		const stat = await fs.stat(path.join(targetDir, "index.js"));
		return stat.isFile();
	} catch {
		return false;
	}
}

async function resolveBuiltAddonPath(outputDir: string, canonicalFilename: string): Promise<string> {
	// Variant-tagged files produced by previous invocations of this script that
	// should NOT be treated as this build's output (unless they equal our target).
	const siblingVariantFilenames = new Set([
		`pi_natives.${targetPlatform}-${targetArch}-modern.node`,
		`pi_natives.${targetPlatform}-${targetArch}-baseline.node`,
	]);
	siblingVariantFilenames.delete(canonicalFilename);

	const entries = await fs.readdir(outputDir);

	if (entries.includes(canonicalFilename)) {
		return path.join(outputDir, canonicalFilename);
	}

	// napi-rs 3.x emits `${binaryName}.${platformArchABI}.node` where
	// platformArchABI is e.g. `darwin-x64`, `linux-x64-gnu`, `win32-x64-msvc`,
	// `darwin-arm64`. Match any file for this platform/arch that isn't a
	// sibling variant we might have produced previously.
	const generatedCandidates = entries.filter(entry => {
		if (!entry.startsWith(`pi_natives.${targetPlatform}-${targetArch}`) || !entry.endsWith(".node")) {
			return false;
		}
		return !siblingVariantFilenames.has(entry);
	});

	if (generatedCandidates.length === 1) {
		return path.join(outputDir, generatedCandidates[0]);
	}

	if (generatedCandidates.length === 0) {
		throw new Error(
			`napi build succeeded but did not emit a native addon for ${targetPlatform}-${targetArch}. Expected ${canonicalFilename} or an environment-tagged variant in ${outputDir}. Directory contents: ${entries.join(", ") || "(empty)"}.`,
		);
	}

	const formattedCandidates = generatedCandidates.map(candidate => `  - ${candidate}`).join("\n");
	throw new Error(
		`napi build emitted multiple unrecognized native addons for ${targetPlatform}-${targetArch}:\n${formattedCandidates}`,
	);
}

const isCI = Boolean(Bun.env.CI);
const useLocalProfile = !isCI && !isCrossCompile;
const features = new Set<string>();

if (isLean) {
	features.add("chunk-native");
	features.add("structural-search-system");
	features.add("text-search-system");
	features.add("shell-system");
	features.add("fuzzy-search-system");
	features.add("discovery-system");
} else {
	features.add("chunk-native");
	features.add("structural-search-native");
	features.add("text-search-native");
	features.add("shell-native");
	features.add("fuzzy-search-native");
	features.add("discovery-native");
}

if (!noHighlights) {
	features.add("syntax-highlighting");
}

if (!noImage) {
	features.add("image");
}

if (Bun.env.PI_NATIVE_FEATURES) {
	for (const feature of Bun.env.PI_NATIVE_FEATURES.split(",")) {
		const trimmed = feature.trim();
		if (trimmed) {
			features.add(trimmed);
		}
	}
}

// Build napi args
const napiArgs = [
	"build",
	"--manifest-path",
	path.join(rustDir, "Cargo.toml"),
	"--package-json-path",
	packageJsonPath,
	"--platform",
	"--no-js",
	"--dts",
	"index.d.ts",
	"-o",
	bindingsDir,
	"--no-default-features",
];

if (features.size > 0) {
	napiArgs.push("--features", Array.from(features).join(","));
}

if (useLocalProfile) {
	napiArgs.push("--profile", "local");
} else {
	napiArgs.push("--release");
}

if (crossTarget) napiArgs.push("--target", crossTarget);

const profileLabel = useLocalProfile ? " (local)" : "";
const canonicalAddonFilename = `pi_natives.${targetPlatform}-${targetArch}${variantSuffix}.node`;
const canonicalAddonPath = path.join(runtimeAddonDir, canonicalAddonFilename);

console.log(`Building pi-natives for ${targetPlatform}-${targetArch}${variantSuffix}${profileLabel}…`);
console.log(`Features: ${Array.from(features).join(", ")}`);
if (buildFlavor !== "full") {
	console.log(`Generated bindings dir: ${bindingsDir}`);
}

await fs.mkdir(bindingsDir, { recursive: true });
await fs.mkdir(nativeDir, { recursive: true });
await cleanupStaleTemps(bindingsDir);
await cleanupStaleTemps(nativeDir);
await cleanupNativeAddons(runtimeAddonDir);

// Resolve napi bin directly: `bunx @napi-rs/cli` can pick up the wrong bin on
// systems where `cli` exists on PATH (e.g. Mono's /usr/bin/cli on Ubuntu).
const napiBin = Bun.which("napi", {
	PATH: `${path.join(import.meta.dir, "..", "node_modules", ".bin")}:${path.join(repoRoot, "node_modules", ".bin")}:${process.env.PATH ?? ""}`,
});
if (!napiBin) {
	throw new Error("Could not locate @napi-rs/cli `napi` binary in node_modules/.bin");
}
const buildResult = await $`${napiBin} ${napiArgs}`.nothrow();
if (buildResult.exitCode !== 0) {
	const stderr = buildResult.stderr?.toString("utf-8") ?? "";
	throw new Error(`napi build failed${stderr ? `:\n${stderr}` : ""}`);
}

const builtAddonPath = await resolveBuiltAddonPath(bindingsDir, canonicalAddonFilename);
if (builtAddonPath !== canonicalAddonPath) {
	console.log(`Normalizing native addon filename: ${path.basename(builtAddonPath)} → ${canonicalAddonFilename}`);
	await installBinary(builtAddonPath, canonicalAddonPath);
	await fs.unlink(builtAddonPath).catch(() => {});
}

if (await hasGeneratedIndexJs(bindingsDir)) {
	await patchGeneratedIndexLoader(bindingsDir);

	// Generate runtime enum exports from const enums in index.d.ts
	await $`bun ${path.join(import.meta.dir, "gen-enums.ts")}`.env({
		...process.env,
		PI_NATIVE_BINDINGS_DIR: bindingsDir,
	});
	await patchGeneratedIndexLoader(bindingsDir);
} else if (buildFlavor !== "full") {
	console.log("Skipping JS binding post-processing for non-full build output.");
}

console.log("Build complete.");
