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

test("reviewed JSX presentation literals work with Windows paths", () => {
  const path = "app\\src\\components\\PaidFeatureCard.tsx";
  const text = "          添加开发者微信\n";
  assert.doesNotMatch(maskLitePresentationContent(path, text), /微信/u);
  assert.equal(maskLitePresentationContent(path, text), maskLitePresentationContent(path.replaceAll("\\", "/"), text));
  assert.match(maskLitePresentationContent("app\\src\\api.ts", text), /微信/u);
  assert.match(maskLitePresentationContent(path, text + 'invoke("wechat_bind")'), /wechat_bind/);
});
