#!/usr/bin/env node
/**
 * `ontok-builder` 的 bin 转发器（主包只带这一个文件）。
 *
 * 为什么是这个形状：
 * - 二进制按平台分成 3 个包（esbuild 那套）：主包靠 `optionalDependencies` 挑一个；
 * - ⛔ 不在 postinstall 里下载 —— 内网/代理常拿不到 GitHub Releases，而平台包走 npm registry 就能装，lockfile 还能锁版本。
 *
 * 解析顺序：
 *   1. `ONTOK_BUILDER_BIN`（显式覆盖：自编一份、或跑测试时用）
 *   2. 平台包 `@<scope>/ontok-builder-<platform>-<arch>/bin/<exe>`
 *   3. 报错：说清缺哪个包、当前 os/cpu 是什么、怎么补
 */
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const here = path.dirname(fileURLToPath(import.meta.url));
const selfPkg = require(path.join(here, "..", "package.json"));

/** 只发布这三个（用户口径：linux 不要 arm，mac 不要 x64） */
const PLATFORMS = {
  "win32-x64": { slug: "win32-x64", exe: "ontok-builder.exe" },
  "darwin-arm64": { slug: "darwin-arm64", exe: "ontok-builder" },
  "linux-x64": { slug: "linux-x64", exe: "ontok-builder" },
};

const fail = (msg) => {
  process.stderr.write(`✗ ontok-builder: ${msg}\n`);
  process.exit(2);
};

/** `@scope/ontok-builder` → `@scope/ontok-builder-win32-x64`；无 scope 时 `ontok-builder-win32-x64` */
function platformPackageName(slug) {
  const name = selfPkg.name;
  const at = name.startsWith("@") ? name.indexOf("/") : -1;
  return at > 0 ? `${name.slice(0, at)}/${name.slice(at + 1)}-${slug}` : `${name}-${slug}`;
}

function resolveBinary() {
  const override = process.env.ONTOK_BUILDER_BIN;
  if (override) return { bin: override, how: "ONTOK_BUILDER_BIN" };

  const key = `${process.platform}-${process.arch}`;
  const spec = PLATFORMS[key];
  if (!spec) {
    fail(
      `不支持的平台：${key}\n` +
        `本包只发布：${Object.keys(PLATFORMS).join(" · ")}\n` +
        `要用其它平台，用 ONTOK_BUILDER_BIN 指到你自己的 ontok-builder。`
    );
  }

  const pkgName = platformPackageName(spec.slug);
  let pkgJsonPath;
  try {
    pkgJsonPath = require.resolve(`${pkgName}/package.json`);
  } catch {
    fail(
      `缺平台包 ${pkgName}\n` +
        `常见原因：\n` +
        `  · 用了 --no-optional / --omit=optional（本包靠 optionalDependencies 挑平台）\n` +
        `  · 私有源没同步平台包\n` +
        `  · 在 ${key} 上装了，但那个平台的包没发出来\n` +
        `重试：npm i -g ${selfPkg.name} --include=optional`
    );
  }
  return { bin: path.join(path.dirname(pkgJsonPath), "bin", spec.exe), how: pkgName };
}

const { bin, how } = resolveBinary();
const child = spawn(bin, process.argv.slice(2), { stdio: "inherit" });
child.on("error", (e) => fail(`执行 ${bin} 失败（来自 ${how}）：${e.message}`));
child.on("exit", (code, signal) => {
  if (signal) return process.kill(process.pid, signal);
  process.exit(code ?? 1);
});
