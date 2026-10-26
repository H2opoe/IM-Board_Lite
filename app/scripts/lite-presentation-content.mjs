// Only reviewed presentation strings and the public developer contact URL are
// allowed. Connector identifiers, commands, dependencies and paths remain blocked.
const presentationStrings = {
  "app/src/components/PaidFeatureCard.tsx": ["微信", "添加开发者微信"],
  "app/src/components/DeveloperFeedbackModal.tsx": [
    "https://u.wechat.com/EDsLZ6LQyemJxtrM-PlvT-k?s=3",
    "开发者微信二维码", "复制失败，请手动复制微信号。",
    "扫码添加微信，或复制微信号联系", "微信号", "复制微信号"
  ]
};

export function maskLitePresentationContent(path, text) {
  path = path.replaceAll("\\", "/");
  for (const literal of presentationStrings[path] ?? []) {
    // Match complete quoted literals or JSX text, never arbitrary code tokens.
    text = text.replaceAll(JSON.stringify(literal), '"public-contact-presentation"');
    text = text.replaceAll(`>${literal}<`, ">public-contact-presentation<");
    if (path.endsWith("/PaidFeatureCard.tsx") && literal === "添加开发者微信") {
      text = text.replaceAll("          添加开发者微信\n", "          public-contact-presentation\n");
    }
  }
  return text;
}
