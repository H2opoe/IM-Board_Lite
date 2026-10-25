use super::{Failure, Result};
use serde_json::Value;
use std::future::Future;
pub(super) trait Transport {
    fn post(&self, url: &str, body: Value) -> impl Future<Output = Result<Value>> + Send;
}
pub(super) struct Http(reqwest::Client);
impl Http {
    pub fn new() -> Result<Self> {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map(Self)
            .map_err(|_| Failure::new("WECOM_COMPAT_NETWORK", "企业微信自动读取网络初始化失败。"))
    }
}
impl Transport for Http {
    async fn post(&self, url: &str, body: Value) -> Result<Value> {
        let mut response = self
            .0
            .post(url)
            .header("Accept", "application/json")
            .header("User-Agent", super::user_agent())
            .json(&body)
            .send()
            .await
            // reqwest errors can contain an authenticated URL: never format them.
            .map_err(|_| {
                Failure::new(
                    "WECOM_COMPAT_NETWORK",
                    "企业微信自动读取请求失败，请检查网络后重试。",
                )
            })?;
        if !response.status().is_success() {
            return Err(Failure::new(
                "WECOM_COMPAT_HTTP",
                format!(
                    "企业微信自动读取接口返回 HTTP {}。",
                    response.status().as_u16()
                ),
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Failure::invalid())? {
            if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
                return Err(Failure::invalid());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| Failure::invalid())
    }
}
