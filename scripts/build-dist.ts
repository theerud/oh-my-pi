import { $ } from "bun";
import * as childProcess from "node:child_process";
import * as fs from "node:fs/promises";
import * as fsSync from "node:fs";
import * as path from "node:path";

const repoRoot = import.meta.dir ? path.join(import.meta.dir, "..") : process.cwd();
const distDir = path.join(repoRoot, "dist");
const stagingDir = path.join(distDir, "omp");
const isLean = process.argv.includes("--lean");
const isMinimal = process.argv.includes("--minimal");

function pickSystemBinary(name: string): string {
	const candidates = childProcess.execSync(`which -a ${name}`).toString().trim().split("\n").filter(Boolean);
	return candidates.find(candidate => !candidate.includes("bun")) || candidates[0] || name;
}

function buildSystemNodePath(nodePath: string): string {
	const nodeDir = path.dirname(nodePath);
	const bunDir = path.dirname(process.execPath);
	const existing = (process.env.PATH ?? "")
		.split(path.delimiter)
		.filter(Boolean)
		.filter(entry => path.resolve(entry) !== path.resolve(bunDir) && path.resolve(entry) !== path.resolve(nodeDir));
	return [nodeDir, ...existing].join(path.delimiter);
}

function findLockedPackageVersion(lockText: string, packageName: string): string | null {
	const match = lockText.match(new RegExp(`"${packageName}": \\["${packageName}@([^"]+)"`));
	return match?.[1] ?? null;
}

async function main() {
	console.log("🚀 Starting distribution build...");

	// 1. Ensure directories exist
	if (fsSync.existsSync(distDir)) {
		await fs.rm(distDir, { recursive: true, force: true });
	}
	await fs.mkdir(stagingDir, { recursive: true });

	// 2. Build Native Modules
	console.log("\n📦 Building native modules...");
	if (isMinimal) {
		await $`bun run build:native:minimal`.cwd(repoRoot);
	} else if (isLean) {
		await $`bun run build:native:lean`.cwd(repoRoot);
	} else {
		await $`bun run build:native`.cwd(repoRoot);
	}

	// 3. Build JS Bundle
	console.log("\n📦 Building JS bundle...");
	const agentPkgDir = path.join(repoRoot, "packages/coding-agent");
	await $`bun run build:bundle`.cwd(agentPkgDir);

	// 4. Collect custom artifacts
	console.log("\n🚚 Assembling artifacts...");
	
	// Copy omp.js
	const sourceBundle = path.join(agentPkgDir, "dist/omp.js");
	await fs.copyFile(sourceBundle, path.join(stagingDir, "omp.js"));
	await fs.chmod(path.join(stagingDir, "omp.js"), 0o755);

	// Copy our custom native modules
	const nativeTargetDir = path.join(stagingDir, "native");
	await fs.mkdir(nativeTargetDir, { recursive: true });
	const sourceNativeDir = path.join(repoRoot, "packages/natives/native");
	const nativeFiles = await fs.readdir(sourceNativeDir);
	for (const file of nativeFiles) {
		if (file.endsWith(".node")) {
			await fs.copyFile(path.join(sourceNativeDir, file), path.join(nativeTargetDir, file));
		}
	}

	// 5. Setup external native dependencies
	console.log("\n📋 Setting up external dependencies...");
	const agentPkg = JSON.parse(await fs.readFile(path.join(agentPkgDir, "package.json"), "utf-8"));
	const rootPkg = JSON.parse(await fs.readFile(path.join(repoRoot, "package.json"), "utf-8"));

	// Identify runtime-native dependency versions from the current workspace.
	const lockText = await Bun.file(path.join(repoRoot, "bun.lock")).text();
	const sqliteVersion =
		rootPkg.devDependencies?.["better-sqlite3"] ??
		findLockedPackageVersion(lockText, "better-sqlite3") ??
		"12.8.0";
	const mupdfVersion =
		rootPkg.devDependencies?.mupdf ?? findLockedPackageVersion(lockText, "mupdf") ?? "1.27.0";

	const runtimePkg = {
		name: "omp-runtime",
		version: agentPkg.version,
		private: true,
		type: "module",
		dependencies: {
			"better-sqlite3": sqliteVersion,
			"mupdf": mupdfVersion,
		},
	};
	await fs.writeFile(path.join(stagingDir, "package.json"), JSON.stringify(runtimePkg, null, 2));

	console.log(
		`📥 Running npm install for external runtime modules (better-sqlite3@${sqliteVersion}, mupdf@${mupdfVersion})...`,
	);
	const nodePath = pickSystemBinary("node");
	const npmPath = pickSystemBinary("npm");
	const nodeVersion = childProcess.execFileSync(nodePath, ["-p", "process.versions.node"], { encoding: "utf-8" }).trim();
	const installPath = buildSystemNodePath(nodePath);

	childProcess.execFileSync(npmPath, ["install", "--omit=dev", "--no-package-lock", `--target=${nodeVersion}`], {
		cwd: stagingDir,
		stdio: "inherit",
		env: {
			...process.env,
			PATH: installPath,
			npm_config_runtime: "node",
			npm_config_target: nodeVersion,
			npm_config_user_agent: undefined,
		},
	});

	// 6. Create Tarball
	console.log("\n🎁 Creating tarball...");
	const flavor = isMinimal ? "-minimal" : isLean ? "-lean" : "";
	const tarballName = `omp-v${agentPkg.version}-${process.platform}-${process.arch}${flavor}.tar.gz`;
	await $`tar -czf ${tarballName} omp`.cwd(distDir);

	console.log(`\n✅ Build Complete!`);
	console.log(`Location: ${stagingDir}`);
	console.log(`Tarball:  ${path.join(distDir, tarballName)}`);
	console.log("\nTo run: node dist/omp/omp.js");
}

main().catch(err => {
	console.error("\n❌ Distribution build failed:", err);
	process.exit(1);
});
