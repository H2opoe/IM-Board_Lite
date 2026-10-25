import { createHash } from "node:crypto";
import { createReadStream, createWriteStream, lstatSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";

const manifest = JSON.parse(readFileSync(new URL("runtime-downloads.json", import.meta.url), "utf8"));

function existingFile(path) {
  try {
    const info = lstatSync(path);
    if (!info.isFile()) throw new Error(`归档缓存必须是普通文件：${path}`);
    return info;
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

async function matches(path, info, asset) {
  if (!info || info.size !== asset.sizeBytes) return false;
  const hash = createHash("sha256");
  let size = 0;
  for await (const chunk of createReadStream(path)) {
    size += chunk.length;
    if (size > asset.sizeBytes) return false;
    hash.update(chunk);
  }
  return size === asset.sizeBytes && hash.digest("hex") === asset.sha256;
}

export async function ensureRuntimeArchive(path, name, { inputs = manifest, fetcher = fetch } = {}) {
  const asset = inputs.assets?.[name];
  if (inputs.schemaVersion !== 1 || !asset || !/^[a-f0-9]{64}$/.test(asset.sha256) || !Number.isSafeInteger(asset.sizeBytes) || asset.sizeBytes <= 0 || asset.sizeBytes > 512 * 1024 * 1024) throw new Error(`缺少有效的固定归档清单：${name}`);
  const source = new URL(asset.url);
  if (source.protocol !== "https:" || source.hostname !== "github.com" || source.username || source.password || !source.pathname.includes("/releases/download/")) throw new Error(`归档必须来自固定官方 Release：${name}`);
  if (await matches(path, existingFile(path), asset)) return;
  mkdirSync(dirname(path), { recursive: true });
  const owned = mkdtempSync(join(dirname(path), ".imboard-download-"));
  const pending = join(owned, "archive");
  const failures = [];
  try {
    for (const url of [asset.url, `https://gh-proxy.com/${asset.url}`, `https://gh.llkk.cc/${asset.url}`]) {
      try {
        const response = await fetcher(url, { signal: AbortSignal.timeout(300_000) });
        if (!response.ok || !response.body) throw new Error(`HTTP ${response.status}`);
        let size = 0;
        const hash = createHash("sha256");
        const verify = new Transform({ transform(chunk, _encoding, next) {
          size += chunk.length;
          if (size > asset.sizeBytes) return next(new Error("下载长度超过固定大小"));
          hash.update(chunk);
          next(null, chunk);
        } });
        await pipeline(Readable.fromWeb(response.body), verify, createWriteStream(pending, { flags: "wx" }));
        if (size !== asset.sizeBytes || hash.digest("hex") !== asset.sha256) throw new Error("归档大小或 SHA-256 不匹配");
        // Publish only a complete verified download; failed mirrors preserve the cache.
        renameSync(pending, path);
        return;
      } catch (error) {
        failures.push(`${url}: ${error.message}`);
        rmSync(pending, { force: true });
      }
    }
    throw new Error(`无法下载固定归档 ${name}：${failures.join("; ")}`);
  } finally {
    rmSync(owned, { recursive: true, force: true });
  }
}
