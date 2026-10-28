#!/usr/bin/env node
/**
 * 生成 npm 包（CI 与本机都能跑）。
 *
 * 为什么不把 package.json 手写在仓库里：
 *   版本必须跟着 `Cargo.toml` 走 —— 手写就会出现"二进制 0.3.1 / npm 0.3.0"这种鬼。
 *   这里**唯一版本源**是 `code/Cargo.toml`，主包与 3 个平台包的 version 都由它算出来。
 *
 * 用法：
 *   node npm/prepare.mjs platform --slug win32-x64 --bin <编好的 exe 路径> [--scope @anspire]
 *   node npm/prepare.mjs main     [--scope @anspire]          # 生成主包（依赖上面 3 个平台包）
 *   node npm/prepare.mjs --help
 *
 * 产物统一落在 `npm/out/<scope>/<包名>/`（git 忽略）。
 */
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..");
const outRoot = path.join(here, "out");

/** 只发这三个平台（用户口径：linux 不要 arm，mac 不要 x64） */
const PLATFORMS = {
  "win32-x64": { os: "win32", cpu: "x64", exe: "ontok-builder.exe" },
  "darwin-arm64": { os: "darwin", cpu: "arm64", exe: "ontok-builder" },
  "linux-x64": { os: "linux", cpu: "x64", exe: "ontok-builder" },
};

function arg(name, fallback = undefined) {
  const i = process.argv.indexOf(`--${name}`);
  if (i < 0) return fallback;
  const v = process.argv[i + 1];
  if (v === undefined || v.startsWith("--")) throw new Error(`--${name} 缺值`);
  return v;
}

/** 唯一版本源：Cargo.toml 的 [package] version */
function cargoVersion() {
  const text = readFileSync(path.join(repoRoot, "Cargo.toml"), "utf8");
  const m = text.match(/^version\s*=\s*"([^"]+)"/m);
  if (!m) throw new Error("Cargo.toml 里找不到 version");
  return m[1];
}

const scope = arg("scope", "@anspire-ai");
const base = "ontok-builder";
const mainName = `${scope}/${base}`;
const version = cargoVersion();

function writePkg(name, pkg, extraFiles = []) {
  const dir = path.join(outRoot, ...name.split("/"));
  mkdirSync(path.join(dir, "bin"), { recursive: true });
  writeFileSync(path.join(dir, "package.json"), JSON.stringify(pkg, null, 2) + "\n", "utf8");
  for (const f of extraFiles) {
    const dst = path.join(dir, f.dst);
    mkdirSync(path.dirname(dst), { recursive: true });
    copyFileSync(f.src, dst);
  }
  return dir;
}

function platform(slug, binPath) {
  const spec = PLATFORMS[slug];
  if (!spec) throw new Error(`未知平台 ${slug}（可选：${Object.keys(PLATFORMS).join(", ")}）`);
  if (!existsSync(binPath)) throw new Error(`找不到二进制：${binPath}`);
  const name = `${mainName}-${slug}`;
  const exe = spec.exe;
  const pkg = {
    name,
    version,
    description: `ontok-builder 的 ${slug} 二进制（由主包按平台自动安装，⛔ 不要直接装它）`,
    license: "UNLICENSED",
    os: [spec.os],
    cpu: [spec.cpu],
    files: [`bin/${exe}`],
    preferUnplugged: true,
  };
  const dir = writePkg(name, pkg, [{ src: binPath, dst: `bin/${exe}` }]);
  process.stdout.write(`✓ ${name}@${version} → ${dir}\n`);
}

function main() {
  const deps = {};
  for (const slug of Object.keys(PLATFORMS)) deps[`${mainName}-${slug}`] = version;
  const pkg = {
    name: mainName,
    version,
    description:
      "ontok 构建端 CLI（本体构建：doc ingest / propose / console / bundle / access / relation）。" +
      "本包只带一个转发器，真二进制在平台包 ontok-builder-<platform>-<arch> 里。",
    license: "UNLICENSED",
    type: "module",
    bin: { "ontok-builder": "bin/ontok-builder.js" },
    files: ["bin/ontok-builder.js"],
    engines: { node: ">=18" },
    optionalDependencies: deps,
  };
  const dir = writePkg(mainName, pkg, [
    { src: path.join(here, "main", "bin", "ontok-builder.js"), dst: "bin/ontok-builder.js" },
  ]);
  process.stdout.write(`✓ ${mainName}@${version} → ${dir}\n`);
  process.stdout.write(`  optionalDependencies: ${Object.keys(deps).join(", ")}\n`);
}

function help() {
  process.stdout.write(
    [
      "node npm/prepare.mjs platform --slug <win32-x64|darwin-arm64|linux-x64> --bin <路径> [--scope @anspire]",
      "node npm/prepare.mjs main [--scope @anspire]",
      "",
      `版本来自 Cargo.toml：${version}`,
      `产物目录：${outRoot}`,
    ].join("\n") + "\n"
  );
}

try {
  const cmd = process.argv[2];
  if (!cmd || cmd === "--help" || cmd === "-h") help();
  else if (cmd === "platform") platform(arg("slug"), arg("bin"));
  else if (cmd === "main") main();
  else throw new Error(`未知子命令 ${cmd}`);
} catch (e) {
  process.stderr.write(`✗ prepare: ${e.message}\n`);
  process.exit(2);
}
