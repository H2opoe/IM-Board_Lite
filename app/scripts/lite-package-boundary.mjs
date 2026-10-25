import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";

export function assertLiteResources(path) {
  const forbidden = /(^|[\\/])(wechat(?:-cli|-cli-source|_cli|_bridge)?|weixin|bridge_main|wx_key|frida|python)([\\/._-]|$)/i;
  const forbiddenContent = [/wechat/i, /weixin/i, /wx[_-]?key/i, /(^|[^a-z])frida([^a-z]|$)/i, /bridge_main/i, /wxid/i, /(?<![a-z])filehelper(?![a-z])/i, /strcontent/i, /msgsvrid/i, /(?<![a-z])talker(?![a-z])/i, /@chatroom/i, /文件传输助手/u, /拍了拍/u, /(?<!企业)微信/u];
  const pending = [path];
  while (pending.length > 0) {
    const current = pending.pop();
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const entryPath = join(current, entry.name);
      if (forbidden.test(entryPath)) {
        throw new Error(`Lite 包禁止包含微信源码或运行时：${entryPath}`);
      }
      if (entry.isDirectory()) pending.push(entryPath);
      else if (entry.isFile()) {
        const bytes = readFileSync(entryPath);
        let content = bytes.toString("utf8");
        // Only our own compiled application contains the reviewed enterprise
        // API adapter. Keep every other source/runtime marker and file gated.
        if (/^(?:IM-Board Lite\/)?(?:IM-Board-Lite\.exe|im_board_lib\.dll)$/.test(relative(path, entryPath).replaceAll("\\", "/"))) {
          content = content.replaceAll("weixin.qq.com", "enterprise-api-domain");
        }
        // Reviewed DWS 1.0.65 metadata describes DingTalk enterprise invitation
        // links. It is not a personal-WeChat connector or runtime. Scope this
        // text exception to the exact audited executable; all other markers,
        // paths, dependencies and tampered/new binaries retain the normal gate.
        if (entryPath.replaceAll("\\", "/").endsWith("/runtime/dws/1.0.65-imboard.1/win-x64/dws.exe") &&
            createHash("sha256").update(bytes).digest("hex") === "36587fa53af02944f0da725ec08ccfb95b4f3a19db247f8815ac238ebf426592") {
          content = content.replaceAll("微信邀请链接", "企业外部邀请链接");
        }
        if (forbiddenContent.some((pattern) => pattern.test(content))) {
          throw new Error(`Lite 包禁止包含微信源码标记：${entryPath}`);
        }
      }
    }
  }
}

