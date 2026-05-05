#!/usr/bin/env node

const { execFileSync } = require('child_process');
const path = require('path');
const fs = require('fs');

const PLATFORM_PACKAGES = {
  'darwin-arm64': '@canghe_ai/wechat-cli-darwin-arm64',
  'darwin-x64':   '@canghe_ai/wechat-cli-darwin-x64',
  'linux-x64':    '@canghe_ai/wechat-cli-linux-x64',
  'linux-arm64':  '@canghe_ai/wechat-cli-linux-arm64',
  'win32-x64':    '@canghe_ai/wechat-cli-win32-x64',
};

const platformKey = `${process.platform}-${process.arch}`;
const ext = process.platform === 'win32' ? '.exe' : '';

function getBinaryPath() {
  // 1. 环境变量覆盖
  if (process.env.WECHAT_CLI_BINARY) {
    return process.env.WECHAT_CLI_BINARY;
  }

  // 2. 从平台包解析
  const pkg = PLATFORM_PACKAGES[platformKey];
  if (!pkg) {
    console.error(`wechat-cli: unsupported platform ${platformKey}`);
    process.exit(1);
  }

  try {
    return require.resolve(`${pkg}/bin/wechat-cli${ext}`);
  } catch {
    // 3. fallback: 直接找 node_modules 下的路径
    const modPath = path.join(
      path.dirname(require.resolve(`${pkg}/package.json`)),
      `bin/wechat-cli${ext}`
    );
    if (fs.existsSync(modPath)) return modPath;
  }

  console.error(`wechat-cli: binary not found for ${platformKey}`);
  console.error('Try: npm install --force @canghe/wechat-cli');
  process.exit(1);
}

try {
  execFileSync(getBinaryPath(), process.argv.slice(2), {
    stdio: 'inherit',
    env: { ...process.env },
  });
} catch (e) {
  if (e && e.status != null) process.exit(e.status);
  throw e;
}
