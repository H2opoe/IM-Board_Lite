import { test } from "node:test";
import assert from "node:assert/strict";
import { maskLitePresentationContent } from "./lite-presentation-content.mjs";

test("only the reviewed public presentation literals are exempt", () => {
  const path = "app/src/components/DeveloperFeedbackModal.tsx";
  const url = '"https://u.wechat.com/EDsLZ6LQyemJxtrM-PlvT-k?s=3"';
  assert.doesNotMatch(maskLitePresentationContent(path, url), /wechat/i);
  assert.match(maskLitePresentationContent("app/src/api.ts", url), /wechat/i);
  assert.match(maskLitePresentationContent(path, url + '; invoke("wechat_bind")'), /wechat_bind/);
  assert.match(maskLitePresentationContent(path, '"https://u.wechat.com/other"'), /wechat/i);
});
