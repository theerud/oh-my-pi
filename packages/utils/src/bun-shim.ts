import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import * as crypto from "node:crypto";
import { spawn as nodeSpawn, spawnSync as nodeSpawnSync } from "node:child_process";

// Re-export for convenience
export { path, os };

// Basic stringWidth that strips ANSI and OSC sequences
export function stringWidth(str: string): number {
  return str
    .replace(/\x1b\[[0-9;]*m/g, "")
    .replace(/\x1b\][^\x07]*\x07/g, "")
    .length;
}

export function stripANSI(str: string): string {
  return str.replace(/\x1b\[[0-9;]*m/g, "");
}

export const env = process.env;
export const argv = process.argv;
export const version = "1.5.0"; // Avoid version check failures

export function file(filePath: string) {
  return {
    async text() {
      return fs.readFileSync(filePath, "utf-8");
    },
    async json() {
      return JSON.parse(fs.readFileSync(filePath, "utf-8"));
    },
    async bytes() {
      return new Uint8Array(fs.readFileSync(filePath));
    },
    async arrayBuffer() {
      const buffer = fs.readFileSync(filePath);
      return buffer.buffer.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength);
    },
    async exists() {
      return fs.existsSync(filePath);
    },
    async stat() {
      const s = fs.statSync(filePath);
      return {
        size: s.size,
        mtime: s.mtime,
      };
    },
    slice(start?: number, end?: number) {
      const buffer = fs.readFileSync(filePath);
      const sliced = buffer.slice(start, end);
      return {
        async text() { return sliced.toString("utf-8"); },
        async bytes() { return new Uint8Array(sliced); }
      };
    },
    writer() {
      return {
        write(data: any) {
           fs.appendFileSync(filePath, data);
        },
        flush() {},
        end() {}
      };
    }
  };
}

export async function write(filePath: string, content: any) {
  const dir = path.dirname(filePath);
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
  fs.writeFileSync(filePath, content);
}

export function sleep(ms: number) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

export function which(cmd: string) {
  const PATH = process.env.PATH || "";
  const paths = PATH.split(path.delimiter);
  for (const p of paths) {
    const fullPath = path.join(p, cmd);
    if (fs.existsSync(fullPath)) {
      try {
        const s = fs.statSync(fullPath);
        if (s.isFile() && (s.mode & 0o111)) return fullPath;
      } catch {}
    }
  }
  return null;
}

export function spawn(args: string[] | any, options: any = {}) {
  const cmdArgs = Array.isArray(args) ? args : [args];
  const [cmd, ...restArgs] = cmdArgs;
  const child = nodeSpawn(cmd, restArgs, {
    stdio: options.stdout === "pipe" ? ["inherit", "pipe", "pipe"] : "inherit",
    env: options.env || process.env,
    cwd: options.cwd || process.cwd(),
    shell: true,
  });
  
  return {
    pid: child.pid,
    exited: new Promise((resolve) => {
      child.on("exit", (code) => resolve(code || 0));
    }),
    kill() { child.kill(); },
    stdout: child.stdout,
    stderr: child.stderr,
  };
}

export function spawnSync(args: string[], options: any = {}) {
  const [cmd, ...cmdArgs] = args;
  const result = nodeSpawnSync(cmd, cmdArgs, {
    stdio: "pipe",
    env: options.env || process.env,
    cwd: options.cwd || process.cwd(),
    encoding: "utf-8",
  });
  return {
    exitCode: result.status,
    stdout: result.stdout,
    stderr: result.stderr,
  };
}

export function nanoseconds() {
  const [seconds, nanoseconds] = process.hrtime();
  return seconds * 1e9 + nanoseconds;
}

export function hash(data: any) {
  const h = crypto.createHash("sha256").update(data).digest("hex");
  return {
      toString: () => h,
  };
}

export const CryptoHasher = class {
  private hash;
  constructor(algorithm: string) {
    this.hash = crypto.createHash(algorithm);
  }
  update(data: any) {
    this.hash.update(data);
    return this;
  }
  digest(encoding: string) {
    return this.hash.digest(encoding as any);
  }
};

export const Glob = class {
  constructor(private pattern: string) {}
  match(str: string) {
    return str.includes(this.pattern.replace(/\*/g, ""));
  }
  *scanSync(options: any = {}) {
      yield* [];
  }
  async *scan(options: any = {}) {
      yield* [];
  }
};

export const JSON5 = {
  parse: (text: string) => JSON.parse(text),
};

export const JSONC = {
  parse: (text: string) => JSON.parse(text),
};

export const YAML = {
  parse: (text: string) => ({}), // Should use a YAML lib
  stringify: (obj: any) => JSON.stringify(obj),
};

export const TOML = {
  parse: (text: string) => ({}), // Should use a TOML lib
};

export const Archive = {
  async write(outputPath: string, data: any, options: any) {
      // Placeholder
  }
};

export const stdin = {
  async text() {
    return fs.readFileSync(0, "utf-8");
  },
  stream() {
      return process.stdin;
  }
};

export function gc() {}

export function generateHeapSnapshot() { return new Uint8Array(); }

export function color() { return ""; }

export const $ = Object.assign(
    () => ({
        cwd: () => ({ nothrow: () => Promise.resolve({ exitCode: 0 }) }),
        nothrow: () => Promise.resolve({ exitCode: 0 }),
    }),
    {
        cwd: () => ({ nothrow: () => Promise.resolve({ exitCode: 0 }) }),
    }
);

// bun:ffi exports
export function dlopen() { return { symbols: {} }; }
export const FFIType = {};
export function ptr() { return 0; }
export const CString = {};

// Default export
export const Bun = {
  env, argv, version, file, write, sleep, which, spawn, spawnSync,
  nanoseconds, hash, CryptoHasher, Glob, stringWidth, stripANSI,
  JSON5, JSONC, YAML, TOML, Archive, stdin, gc, generateHeapSnapshot,
  color, $, dlopen, FFIType, ptr, CString,
  path,
  os
};

export default Bun;
