import { $ } from "bun";
import * as fs from "node:fs/promises";
import * as fsSync from "node:fs";
import * as path from "node:path";

const repoRoot = import.meta.dir ? path.join(import.meta.dir, "..") : process.cwd();
const distDir = path.join(repoRoot, "dist");
const stagingDir = path.join(distDir, "omp");
const isLean = process.argv.includes("--lean");
const isMinimal = process.argv.includes("--minimal");

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
	
	// Identify better-sqlite3 version from root devDeps
	const sqliteVersion = rootPkg.devDependencies["better-sqlite3"] || "latest";

	const runtimePkg = {
		name: "omp-runtime",
		version: agentPkg.version,
		private: true,
		type: "module",
		dependencies: {
			"better-sqlite3": sqliteVersion
		}
	};
	await fs.writeFile(path.join(stagingDir, "package.json"), JSON.stringify(runtimePkg, null, 2));

	console.log(`📥 Running npm install for external native modules (better-sqlite3@${sqliteVersion})...`);
	// Use child_process.execSync to bypass Bun's shell environment and explicitly target the system node version
	const { execSync } = await import("node:child_process");
	// Find system node/npm that aren't Bun's temporary shims
	const nodePaths = execSync("which -a node").toString().trim().split("\n");
	const nodePath = nodePaths.find(p => !p.includes("bun")) || nodePaths[0];
	const npmPaths = execSync("which -a npm").toString().trim().split("\n");
	const npmPath = npmPaths.find(p => !p.includes("bun")) || npmPaths[0];

	const nodeVersion = execSync(`${nodePath} -v`).toString().trim().replace("v", "");
	execSync(`${npmPath} install --omit=dev --no-package-lock --target=${nodeVersion}`, {
		cwd: stagingDir,
		stdio: "inherit",
		env: { ...process.env, npm_config_user_agent: undefined }
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
