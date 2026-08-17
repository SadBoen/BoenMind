#!/usr/bin/env node
// 统一 DSH_HOME 启动器：所有 dsh 操作必须指向项目内 DSH_HOME，
// 否则 dsh 默认落 ~/.dsh（2026-08-17 毛玻璃插件装错地方的教训）。
const { spawn } = require("node:child_process");
const path = require("node:path");

const root = path.resolve(__dirname, "..");
const dshBin = path.join(root, "node_modules", "@deepseek-ai", "dsh", "lib", "bin.js");

const env = {
  ...process.env,
  DSH_HOME: path.join(root, "dsh-home"),
};

const child = spawn(process.execPath, [dshBin, ...process.argv.slice(2)], {
  stdio: "inherit",
  env,
});

child.on("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  process.exit(code ?? 1);
});
