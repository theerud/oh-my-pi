# Node.js Bundling & Deployment

This document describes how to build and deploy a Node.js-compatible bundle of the coding agent (`omp.js`). This is the recommended approach for lightweight container environments where you prefer a standard Node.js runtime over Bun.

## Overview

While the default build (`bun run build:binary`) produces a standalone native binary using Bun's compiler, the `build:bundle` command uses `esbuild` to create a single-file ES module (`dist/omp.js`).

### Advantages
- **Lighter weight:** No need to include the Bun runtime in your container if Node.js is already present.
- **Portability:** Runs on any standard Node.js 20+ environment.
- **Easier Debugging:** The bundle is a standard JS file with sourcemaps.

## Building the Bundle

From the root of the repository or within `packages/coding-agent`:

```bash
bun --cwd packages/coding-agent run build:bundle
```

This will:
1. Generate the stats dashboard client bundle.
2. Use `esbuild` to bundle all TypeScript source files.
3. Apply a `bun-shim.ts` to provide compatibility for Bun-specific APIs (like `Bun.file`, `Bun.spawn`, etc.).
4. Output a shebang-enabled ESM file at `packages/coding-agent/dist/omp.js`.

## Native Modules

The `omp.js` bundle relies on the `pi_natives` Rust module. Because native `.node` modules cannot be bundled into a single JS file, they must be provided alongside the bundle.

### Search Path
The bundle looks for the native module in these locations (in order):
1. `../native/` relative to `omp.js`.
2. Adjacent to the `node` executable.
3. `~/.local/bin/` (Linux/macOS) or `AppData/Local/omp` (Windows).
4. `~/.omp/natives/VERSION/`

## Deployment (Container)

For a Docker container, we recommend a structure where `omp.js` and its `native/` folder are kept together.

### Example File Layout
```text
/opt/omp/
├── omp.js                      # The bundle
└── native/                     # Folder containing the .node file
    └── pi_natives.linux-x64-modern.node
```

### External Dependencies
The bundle marks `better-sqlite3` as an external dependency to avoid bundling its native components incorrectly. You must install it in your container:

```bash
cd /opt/omp && npm install better-sqlite3
```

### Example Dockerfile
```dockerfile
FROM node:20-slim

WORKDIR /opt/omp

# Install required external native dependencies
RUN npm install better-sqlite3

# Copy built artifacts from your build stage
COPY packages/coding-agent/dist/omp.js .
COPY packages/natives/native/ ./native/

# Create a symlink for easy access
RUN chmod +x omp.js && ln -s /opt/omp/omp.js /usr/local/bin/omp

# Standard OMP config directory
RUN mkdir -p /root/.omp

ENTRYPOINT ["omp"]
```

## Platform Support

Currently, the bundled `omp.js` supports standard glibc-based Linux (e.g., Debian, Ubuntu, `node:slim`). 

Alpine Linux (musl) is not supported by the default native builds and requires a specific `x86_64-unknown-linux-musl` compilation of the Rust crates. For smallest image size, use `node:20-slim`.
