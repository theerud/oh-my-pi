import esbuild from 'esbuild';
import path from 'path';
import { fileURLToPath } from 'url';
import fs from 'fs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '../../..');
const shimPath = path.join(root, 'packages/utils/src/bun-shim.ts');

const pkgMap = {
    'pi-utils': 'packages/utils/src',
    'pi-ai': 'packages/ai/src',
    'pi-agent-core': 'packages/agent/src',
    'pi-tui': 'packages/tui/src',
    'omp-stats': 'packages/stats/src',
    'pi-natives': 'packages/natives/native',
};

/**
 * esbuild plugin to handle Bun's "with { type: 'text' }" and "with { type: 'json' }" imports.
 */
const bunTextPlugin = {
  name: 'bun-text',
  setup(build) {
    // Resolve workspace packages and sub-packages
    build.onResolve({ filter: /^@oh-my-pi\// }, args => {
        const parts = args.path.split('/');
        const pkgName = parts[1];
        
        if (pkgMap[pkgName]) {
            const subPath = parts.slice(2).join('/');
            const baseDir = path.join(root, pkgMap[pkgName]);
            
            const directCandidates = subPath
                ? [`${subPath}.ts`, `${subPath}.tsx`, `${subPath}.js`, `${subPath}.json`]
                : ['index.ts', 'index.tsx', 'index.js', 'index.json'];
            for (const candidate of directCandidates) {
                const candidatePath = path.join(baseDir, candidate);
                if (fs.existsSync(candidatePath)) return { path: candidatePath };
            }

            if (subPath) {
                const indexCandidates = ['index.ts', 'index.tsx', 'index.js', 'index.json'];
                for (const candidate of indexCandidates) {
                    const candidatePath = path.join(baseDir, subPath, candidate);
                    if (fs.existsSync(candidatePath)) return { path: candidatePath };
                }
            }
        }

        return null;
    });

    // Intercept .md, .py, .txt, .html, .css, .jsonl, .sql
    build.onResolve({ filter: /\.(md|py|txt|html|css|jsonl|sql)$/ }, args => {
      return { path: path.resolve(args.resolveDir, args.path), namespace: 'bun-text' };
    });

    // UNIFY ALL IMPORTS to avoid double bundling.
    build.onResolve({ filter: /^\./ }, args => {
        let absolutePath = path.resolve(args.resolveDir, args.path);
        
        if (args.path.endsWith('.js')) {
            const tsPath = absolutePath.slice(0, -3) + '.ts';
            if (fs.existsSync(tsPath)) return { path: tsPath };
        }

        if (fs.existsSync(absolutePath)) {
            if (fs.statSync(absolutePath).isFile()) return { path: absolutePath };
        }

        for (const ext of ['.ts', '.tsx', '.js', '.json']) {
            if (fs.existsSync(absolutePath + ext)) return { path: absolutePath + ext };
        }

        try {
            if (fs.existsSync(absolutePath) && fs.statSync(absolutePath).isDirectory()) {
                for (const ext of ['.ts', '.tsx', '.js', '.json']) {
                    const index = path.join(absolutePath, 'index' + ext);
                    if (fs.existsSync(index)) return { path: index };
                }
            }
        } catch {}

        return null;
    });

    build.onLoad({ filter: /.*/, namespace: 'bun-text' }, async args => {
      const content = await fs.promises.readFile(args.path, 'utf8');
      return {
        contents: content,
        loader: 'text',
        resolveDir: path.dirname(args.path),
      };
    });

    // Transform stage
    build.onLoad({ filter: /\.(ts|tsx|js|jsx)$/ }, async args => {
        if (args.path.includes('node_modules')) return;
        let contents = await fs.promises.readFile(args.path, 'utf8');
        
        // Strip shebang
        if (contents.startsWith('#!')) {
            contents = contents.slice(contents.indexOf('\n') + 1);
        }

        // Strip 'with { type: "text" }' and 'with { type: "json" }'
        contents = contents.replace(/\s+with\s*\{\s*type:\s*["'](text|json)["']\s*\}/g, '');
        
        // Rewrite bun:sqlite imports to namespace form for the shim-backed Node bundle.
        if (contents.includes('bun:sqlite')) {
             contents = contents.replace(/import\s*{\s*([^}]+)\s*}\s*from\s*["']bun:sqlite["'];?/g, (_match, imports) => {
                 const allImports = imports.split(',').map(s => s.trim()).filter(Boolean);
                 const runtimeImports = allImports.filter(s => !s.startsWith('type '));
                 const typeImports = allImports.filter(s => s.startsWith('type '));
                 const rewritten = [];
                 if (runtimeImports.length > 0) {
                     rewritten.push(`import * as __bunSqlite from "bun:sqlite"; const { ${runtimeImports.join(', ')} } = __bunSqlite;`);
                 }
                 if (typeImports.length > 0) {
                     rewritten.push(`import { ${typeImports.join(', ')} } from "bun:sqlite";`);
                 }
                 return rewritten.join(' ');
             });
        }

        return {
            contents,
            loader: args.path.endsWith('.ts') ? 'ts' : (args.path.endsWith('.tsx') ? 'tsx' : 'js'),
            resolveDir: path.dirname(args.path),
        };
    });
  },
};

async function build() {
  console.log('Building coding-agent for Node.js...');
  
  const outfile = path.join(__dirname, '../dist/omp.js');
  
  try {
    if (!fs.existsSync(path.dirname(outfile))) {
        fs.mkdirSync(path.dirname(outfile), { recursive: true });
    }

    await esbuild.build({
      entryPoints: [path.join(__dirname, '../src/cli.ts')],
      bundle: true,
      platform: 'node',
      target: 'node20',
      outfile: outfile,
      alias: {
        'bun': shimPath,
        'bun:sqlite': shimPath,
        'bun:ffi': shimPath,
      },
      // Inject the shim at the top of the bundle to define Bun global
      inject: [shimPath],
      define: {
        'PI_COMPILED': 'false',
        'globalThis.__OMP_NODE_BUNDLE__': 'true',
      },
      external: [
        'fsevents',
        'better-sqlite3',
        'mupdf',
      ],
      loader: {
        '.json': 'json',
      },
      plugins: [bunTextPlugin],
      sourcemap: true,
      minify: false,
      format: 'esm',
    });
    
    let content = await fs.promises.readFile(outfile, 'utf8');
    if (content.startsWith('#!')) {
        content = content.slice(content.indexOf('\n') + 1);
    }

    const banner = '#!/usr/bin/env node\nimport { createRequire as __createRequire } from "node:module"; const require = __createRequire(import.meta.url);\n';
    await fs.promises.writeFile(outfile, banner + content);
    await fs.promises.chmod(outfile, 0o755);

    console.log('Build successful: packages/coding-agent/dist/omp.js');
  } catch (err) {
    console.error('Build failed:', err);
    process.exit(1);
  }
}

build();
