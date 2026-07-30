export const THIRD_PARTY_NOTICES = `# THIRD_PARTY_NOTICES

IM-Board·聊天汇总看板包含或调用以下第三方开源组件。完整许可文本应随正式发行包一并提供。

## CLI Components

### @wecom/cli
- License: MIT
- Source: https://github.com/WecomTeam/wecom-cli
- Distribution: hot-updated into the application support directory when needed; not bundled in the base installer.

### @larksuite/cli
- License: MIT
- Source: https://github.com/larksuite/cli
- Distribution: hot-updated into the application support directory when needed; not bundled in the base installer.

### @DingTalk-Real-AI/dingtalk-workspace-cli
- License: Apache-2.0
- Source: https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli
- Distribution: hot-updated into the application support directory when needed; not bundled in the base installer.

## Optional Hot-Updated AI Models

### DeepSeek-R1-Distill-Qwen-7B GGUF
- License: MIT, inherited from the upstream DeepSeek-R1distilled model where applicable.
- Source: https://huggingface.co/deepseek-ai/DeepSeek-R1-Distill-Qwen-7B
- GGUF distribution: https://huggingface.co/bartowski/DeepSeek-R1-Distill-Qwen-7B-GGUF
- Mainland download source: https://modelscope.cn/models/unsloth/DeepSeek-R1-Distill-Qwen-7B-GGUF
- Download mirror: https://hf-mirror.com/bartowski/DeepSeek-R1-Distill-Qwen-7B-GGUF
- Distribution: downloaded on demand into the local IMBoard model directory; not bundled in the base installer. Mainland China downloads prefer ModelScope, then HF-Mirror, and finally fall back to Hugging Face.

### llama.cpp
- License: MIT.
- Source: https://github.com/ggml-org/llama.cpp
- Distribution: bundled inside the IM-Board app package; used to launch the local OpenAI-compatible inference service for GGUF models.

## Application Dependencies

The application also uses frontend and Tauri/Rust ecosystem dependencies such as React, Tauri, Vite, Recharts, lucide-react, rusqlite, tokio, reqwest, and related transitive packages. Current dependency metadata is primarily MIT, Apache-2.0, ISC, BSD-3-Clause, and compatible permissive licenses. Before a public release, regenerate this notice from package-lock.json and Cargo.lock to match the exact release artifact.`;
