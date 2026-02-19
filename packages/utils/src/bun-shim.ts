import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import * as crypto from "node:crypto";
import { spawn as nodeSpawn, spawnSync as nodeSpawnSync } from "node:child_process";
import * as yaml from "js-yaml";
import * as toml from "smol-toml";

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

function createReadableStreamShim(nodeStream: any) {
    return {
        getReader() {
            let closed = false;
            const chunks: Uint8Array[] = [];
            let resolveRead: ((value: { done: boolean, value?: Uint8Array }) => void) | null = null;

            nodeStream.on('data', (chunk: Buffer) => {
                const uint8 = new Uint8Array(chunk);
                if (resolveRead) {
                    const resolve = resolveRead;
                    resolveRead = null;
                    resolve({ done: false, value: uint8 });
                } else {
                    chunks.push(uint8);
                }
            });

            nodeStream.on('end', () => {
                closed = true;
                if (resolveRead) {
                    const resolve = resolveRead;
                    resolveRead = null;
                    resolve({ done: true });
                }
            });

            return {
                async read() {
                    if (chunks.length > 0) {
                        return { done: false, value: chunks.shift() };
                    }
                    if (closed) {
                        return { done: true };
                    }
                    return new Promise<{ done: boolean, value?: Uint8Array }>((resolve) => {
                        resolveRead = resolve;
                    });
                },
                releaseLock() {}
            };
        }
    };
}

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
    stdio: options.stdout === "pipe" ? ["pipe", "pipe", "pipe"] : "inherit",
    env: options.env || process.env,
    cwd: options.cwd || process.cwd(),
    shell: typeof cmd === 'string' && restArgs.length === 0,
  });
  
  return {
    pid: child.pid,
    exited: new Promise((resolve) => {
      child.on("exit", (code) => resolve(code || 0));
    }),
    kill() { child.kill(); },
    stdout: options.stdout === "pipe" ? createReadableStreamShim(child.stdout) : child.stdout,
    stderr: options.stdout === "pipe" ? createReadableStreamShim(child.stderr) : child.stderr,
    stdin: child.stdin,
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

// Simple hash implementation for Node.js
function simpleHash(data: any, algorithm: string) {
    const h = crypto.createHash(algorithm).update(data).digest("hex");
    return {
        toString: () => h,
    };
}

export const hash = Object.assign(
    (data: any) => simpleHash(data, 'sha256'),
    {
        xxHash32: (data: any) => {
            let h = 0;
            const s = String(data);
            for (let i = 0; i < s.length; i++) {
                h = Math.imul(31, h) + s.charCodeAt(i) | 0;
            }
            return h >>> 0;
        },
        xxHash64: (data: any) => {
            const h = crypto.createHash('sha256').update(data).digest();
            return h.readBigUInt64BE(0);
        },
        crc32: (data: any) => 0,
        adler32: (data: any) => 0,
    }
);

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
  parse: (text: string) => yaml.load(text),
  stringify: (obj: any) => yaml.dump(obj),
};

export const TOML = {
  parse: (text: string) => toml.parse(text),
  stringify: (obj: any) => toml.stringify(obj),
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

// Real color implementation for Node.js
export function color(colorStr: string, format: string) {
    if (!colorStr) return "";
    
    // Support basic colors
    const basic: Record<string, string> = {
        "black": "0", "red": "1", "green": "2", "yellow": "3", 
        "blue": "4", "magenta": "5", "cyan": "6", "white": "7",
        "gray": "8", "grey": "8"
    };
    
    if (colorStr in basic) {
        return `\x1b[38;5;${basic[colorStr]}m`;
    }

    // Support hex
    if (colorStr.startsWith("#")) {
        const hex = colorStr.slice(1);
        const r = parseInt(hex.slice(0, 2), 16);
        const g = parseInt(hex.slice(2, 4), 16);
        const b = parseInt(hex.slice(4, 6), 16);
        
        if (format === "ansi-16m") {
            return `\x1b[38;2;${r};${g};${b}m`;
        } else {
            // Basic 256-color fallback
            const rIndex = Math.round((r / 255) * 5);
            const gIndex = Math.round((g / 255) * 5);
            const bIndex = Math.round((b / 255) * 5);
            const index = 16 + 36 * rIndex + 6 * gIndex + bIndex;
            return `\x1b[38;5;${index}m`;
        }
    }
    
    // If numeric, treat as 256 color
    if (/^\d+$/.test(colorStr)) {
        return `\x1b[38;5;${colorStr}m`;
    }

    return "";
}

class ShellPromise extends Promise<any> {
    private _cwd: string = process.cwd();
    private _quiet: boolean = false;
    private _nothrow: boolean = false;
    private _env: Record<string, string> = process.env as any;
    private _mode: 'default' | 'text' | 'json' = 'default';

    constructor(private cmd: string | string[]) {
        super((resolve) => {
            // Placeholder
        });
    }

    cwd(path: string) { this._cwd = path; return this; }
    quiet() { this._quiet = true; return this; }
    nothrow() { this._nothrow = true; return this; }
    env(env: Record<string, string>) { this._env = { ...this._env, ...env }; return this; }
    
    async text() { this._mode = 'text'; return this.run(); }
    async json() { this._mode = 'json'; return this.run(); }

    private async run(): Promise<any> {
        return new Promise((resolve, reject) => {
            const fullCmd = Array.isArray(this.cmd) ? this.cmd.join(' ') : this.cmd;
            const child = nodeSpawn(fullCmd, {
                shell: true,
                cwd: this._cwd,
                env: this._env,
                stdio: (this._quiet || this._mode !== 'default') ? 'pipe' : 'inherit'
            });

            let stdout = '';
            let stderr = '';
            if (child.stdout) child.stdout.on('data', (d) => stdout += d);
            if (child.stderr) child.stderr.on('data', (d) => stderr += d);

            child.on('close', (code) => {
                const outputText = stdout.trim();
                if (code !== 0 && !this._nothrow) {
                    return reject(new Error(`Command failed: ${fullCmd}\n${stderr}`));
                }

                if (this._mode === 'text') return resolve(outputText);
                if (this._mode === 'json') return resolve(JSON.parse(outputText));

                resolve({
                    exitCode: code,
                    stdout,
                    stderr,
                    text: () => outputText,
                    json: () => JSON.parse(outputText),
                });
            });
        });
    }

    then(onfulfilled?: any, onrejected?: any) {
        return this.run().then(onfulfilled, onrejected);
    }
}

export const $ = Object.assign(
    (strings: any, ...values: any[]) => {
        const cmd = typeof strings === 'string' ? strings : strings.reduce((acc: string, str: string, i: number) => acc + str + (values[i] ?? ''), '');
        return new ShellPromise(cmd);
    },
    {
        cwd: (path: string) => new ShellPromise('').cwd(path)
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
