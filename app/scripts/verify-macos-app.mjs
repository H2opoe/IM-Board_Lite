import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync, readdirSync, readlinkSync, realpathSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { releaseConfig } from "./release-config.mjs";

const config = releaseConfig();
const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

const appPath = process.argv[2] ?? config.macosAppPath;

const expected = {
  bundleId: config.bundleId,
  name: config.productName,
  shortVersion: config.appVersion,
  bundleVersion: config.macosBundleVersion,
  releaseLabel: config.releaseLabel,
};

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    ...options,
  });
  return {
    status: result.status,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    output: `${result.stdout ?? ""}${result.stderr ?? ""}`.trim(),
  };
}

function requireSuccess(command, args, label) {
  const result = run(command, args);
  if (result.status !== 0) {
    throw new Error(`${label} 失败：${result.output}`);
  }
  return result;
}

function readPlistValue(key) {
  const result = requireSuccess(
    "/usr/libexec/PlistBuddy",
    ["-c", `Print :${key}`, join(appPath, "Contents", "Info.plist")],
    `读取 ${key}`,
  );
  return result.stdout.trim();
}

function assertEqual(label, actual, expectedValue) {
  if (actual !== expectedValue) {
    throw new Error(`${label} 不一致：实际为 ${actual}，期望为 ${expectedValue}`);
  }
}

function assertStableShortVersion(label, value) {
  if (!/^\d+\.\d+\.\d+$/.test(value)) {
    throw new Error(`${label} 必须是不带预发布后缀的三段版本号，当前为 ${value}`);
  }
}

function assertNumericBundleVersion(label, value) {
  if (!/^\d+$/.test(value)) {
    throw new Error(`${label} 必须是递增数字构建号，当前为 ${value}`);
  }
}

function failResource(message) {
  throw new Error(`macOS bundle resource verification failed: ${message}`);
}

function isGeneratedPath(name) {
  return name.split(sep).includes("__pycache__") || name.endsWith(".pyc") || name.endsWith(".pyo") || name.endsWith(".DS_Store");
}

function walkResourceTree(root, directory = root, output = []) {
  if (!existsSync(directory)) failResource(`missing ${relative(root, directory) || "."}`);
  for (const entry of readdirSync(directory).sort()) {
    const path = join(directory, entry);
    const name = relative(root, path);
    if (isGeneratedPath(name)) failResource(`generated/cache file present: ${name}`);
    const info = lstatSync(path);
    if (info.isSymbolicLink()) {
      const target = readlinkSync(path);
      const resolvedTarget = resolve(dirname(path), target);
      if (!resolvedTarget.startsWith(`${resolve(root)}${sep}`)) failResource(`symbolic link escapes component: ${name}`);
      const realTarget = realpathSync(path);
      if (!realTarget.startsWith(`${resolve(root)}${sep}`)) failResource(`symbolic link chain escapes component: ${name}`);
      output.push({ path, name, size: Buffer.byteLength(target), kind: "symlink", content: target });
    } else if (info.isDirectory()) {
      walkResourceTree(root, path, output);
    } else if (info.isFile()) {
      output.push({ path, name, size: info.size, kind: "file" });
    } else {
      failResource(`unsupported filesystem object: ${name}`);
    }
  }
  return output;
}

function fileContent(file) {
  return file.kind === "symlink" ? readFileSync(realpathSync(file.path)) : readFileSync(file.path);
}

function isRunnablePayload(content) {
  if (content.subarray(0, 2).toString("utf8") === "#!") return true;
  if (content.length < 4) return false;
  const magic = content.readUInt32BE(0);
  return [0xcafebabe, 0xbebafeca, 0xfeedface, 0xcefaedfe, 0xfeedfacf, 0xcffaedfe].includes(magic);
}

function assertBundledComponentMatchesSource(sourceRoot, packagedRoot, component) {
  const sourceFiles = new Map(walkResourceTree(sourceRoot).map((file) => [file.name, file]));
  const packagedFiles = new Map(walkResourceTree(packagedRoot).map((file) => [file.name, file]));
  if (sourceFiles.size !== packagedFiles.size) {
    failResource(`${component.id}/${component.platform} file count: actual ${packagedFiles.size}, expected ${sourceFiles.size}`);
  }
  for (const [name, source] of sourceFiles) {
    const packaged = packagedFiles.get(name);
    if (!packaged) failResource(`${component.id}/${component.platform} missing file: ${name}`);
    if (source.kind === "symlink" && packaged.kind === "symlink" && source.content !== packaged.content) {
      failResource(`${component.id}/${component.platform} symlink target differs: ${name}`);
    }
    if (source.kind !== "symlink" && packaged.kind !== "file") {
      failResource(`${component.id}/${component.platform} file type differs: ${name}`);
    }
    const sourceHash = createHash("sha256").update(fileContent(source)).digest("hex");
    const packagedHash = createHash("sha256").update(fileContent(packaged)).digest("hex");
    if (sourceHash !== packagedHash) {
      failResource(`${component.id}/${component.platform} content differs: ${name}`);
    }
    if ((lstatSync(source.path).mode & 0o111) !== 0 && isRunnablePayload(fileContent(source)) && (lstatSync(packaged.path).mode & 0o111) === 0) {
      failResource(`${component.id}/${component.platform} executable bit is missing: ${name}`);
    }
  }
  for (const name of packagedFiles.keys()) {
    if (!sourceFiles.has(name)) failResource(`${component.id}/${component.platform} unlisted bundled file: ${name}`);
  }
}

function verifyBundledResources() {
  const runtimeRoot = join(appRoot, "runtime");
  const packagedRoot = join(appPath, "Contents", "Resources", "_up_");
  const manifest = JSON.parse(readFileSync(join(runtimeRoot, "manifest.json"), "utf8"));
  if (!Array.isArray(manifest.components) || manifest.components.length === 0) {
    failResource("runtime manifest has no components");
  }

  for (const component of manifest.components) {
    const sourceComponentRoot = resolve(runtimeRoot, component.path);
    const packagedComponentRoot = resolve(packagedRoot, "runtime", component.path);
    const sourceRuntimeRoot = resolve(runtimeRoot);
    const packagedRuntimeRoot = resolve(packagedRoot, "runtime");
    if (!sourceComponentRoot.startsWith(`${sourceRuntimeRoot}${sep}`) || !packagedComponentRoot.startsWith(`${packagedRuntimeRoot}${sep}`)) {
      failResource(`component escapes package runtime root: ${component.path}`);
    }
    assertBundledComponentMatchesSource(sourceComponentRoot, packagedComponentRoot, component);
  }

  const packagedBridge = join(packagedRoot, "bridges", "wechat", "bridge_main");
  if (manifest.edition === "full") {
    const sourceBridge = join(appRoot, "bridges", "wechat", "bridge_main");
    if (!existsSync(packagedBridge)) failResource("missing WeChat bridge_main");
    const sourceBridgeHash = createHash("sha256").update(readFileSync(sourceBridge)).digest("hex");
    const packagedBridgeHash = createHash("sha256").update(readFileSync(packagedBridge)).digest("hex");
    if (packagedBridgeHash !== sourceBridgeHash) failResource("WeChat bridge_main content differs from source");
    if ((lstatSync(packagedBridge).mode & 0o111) === 0) failResource("WeChat bridge_main is not executable");
  } else if (existsSync(packagedBridge)) {
    failResource("Lite bundle must not contain WeChat bridge_main");
  }
  console.log(`[macos-app] bundled resources verified: ${manifest.components.length} runtime components${manifest.edition === "full" ? " and bridge_main" : ""}`);
}

if (!existsSync(appPath)) {
  throw new Error(`找不到 macOS App：${appPath}`);
}

verifyBundledResources();

const plistValues = {
  bundleId: readPlistValue("CFBundleIdentifier"),
  displayName: readPlistValue("CFBundleDisplayName"),
  bundleName: readPlistValue("CFBundleName"),
  shortVersion: readPlistValue("CFBundleShortVersionString"),
  bundleVersion: readPlistValue("CFBundleVersion"),
};

assertEqual("CFBundleIdentifier", plistValues.bundleId, expected.bundleId);
assertEqual("CFBundleDisplayName", plistValues.displayName, expected.name);
assertEqual("CFBundleName", plistValues.bundleName, expected.name);
assertEqual("CFBundleShortVersionString", plistValues.shortVersion, expected.shortVersion);
assertEqual("CFBundleVersion", plistValues.bundleVersion, expected.bundleVersion);
assertStableShortVersion("CFBundleShortVersionString", plistValues.shortVersion);
assertNumericBundleVersion("CFBundleVersion", plistValues.bundleVersion);
if (plistValues.shortVersion.includes("beta") || plistValues.bundleVersion.includes("beta")) {
  throw new Error(`macOS Bundle 版本字段不能包含 beta 标签，发布标签 ${expected.releaseLabel} 只能用于展示和安装包命名`);
}

const carbon = run("/usr/libexec/PlistBuddy", [
  "-c",
  "Print :LSRequiresCarbon",
  join(appPath, "Contents", "Info.plist"),
]);
if (carbon.status === 0) {
  throw new Error("Info.plist 不能包含 LSRequiresCarbon，旧字段会干扰系统按现代 bundle 身份记录权限");
}

const codesignInfo = requireSuccess("codesign", ["-dv", appPath], "读取 codesign 信息");
const identifierMatch = codesignInfo.output.match(/Identifier=(.+)/);
if (!identifierMatch) {
  throw new Error(`codesign 输出缺少 Identifier：${codesignInfo.output}`);
}
assertEqual("codesign Identifier", identifierMatch[1].trim(), expected.bundleId);

requireSuccess("codesign", ["--verify", "--deep", "--strict", "--verbose=4", appPath], "codesign 严格校验");

const spctl = run("spctl", ["--assess", "--type", "execute", "--verbose=4", appPath]);
if (spctl.status !== 0) {
  console.warn(`[macos-app] spctl 未通过：${spctl.output}`);
  console.warn("[macos-app] 如果这是本地 ad-hoc 包，spctl 可能因未使用 Developer ID/notarize 而失败；正式发布包必须修复。");
} else {
  console.log(`[macos-app] spctl 通过：${spctl.output}`);
}

requireSuccess(
  "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister",
  ["-f", appPath],
  "LaunchServices 注册",
);

console.log("[macos-app] IM-Board.app 身份、版本、签名和 LaunchServices 注册检查完成");
