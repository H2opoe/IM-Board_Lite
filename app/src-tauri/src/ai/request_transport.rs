pub(crate) fn describe_request_error(prefix: &str, err: reqwest::Error) -> String {
    let mut details = vec![err.to_string()];
    let mut source = err.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !details.iter().any(|detail| detail == &cause_text) {
            details.push(cause_text);
        }
        source = cause.source();
    }

    let hints = request_error_hints(&err, &details);

    let hint_text = if hints.is_empty() {
        String::new()
    } else {
        format!("；{}", hints.join("；"))
    };
    format!(
        "{prefix}：{}{}",
        truncate_text(details.join("；原因："), 700),
        hint_text
    )
}

fn request_error_hints(err: &reqwest::Error, details: &[String]) -> Vec<&'static str> {
    let mut hints = Vec::new();
    // rustls 的 unexpected-eof 常发生在 TLS 连接被服务商或中间网络提前断开时，
    // reqwest 可能把它归为 request/send 阶段，这里单独提示，避免误导用户只检查 Base URL。
    let tls_closed_early = details_contain(
        details,
        &[
            "peer closed connection without sending tls close_notify",
            "unexpected-eof",
            "unexpected eof",
        ],
    );
    if err.is_timeout() {
        hints.push("请求超时，请检查网络、代理或服务商响应速度");
    }
    if err.is_connect() {
        hints.push("连接失败，请检查 DNS、代理/VPN、防火墙或公司网络策略");
    }
    if tls_closed_early {
        hints.push("TLS 连接被对端或中间网络提前断开，通常是服务商、代理/VPN、网关或公司网络临时中断，可稍后重试或切换网络/代理");
    } else if err.is_request() {
        hints.push("请求未能发出，请确认 Base URL 可访问且系统时间正常");
    }
    hints
}

fn is_retryable_request_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || is_tls_closed_early(err)
}

fn is_tls_closed_early(err: &reqwest::Error) -> bool {
    let mut details = vec![err.to_string()];
    let mut source = err.source();
    while let Some(cause) = source {
        details.push(cause.to_string());
        source = cause.source();
    }
    details_contain(
        &details,
        &[
            "peer closed connection without sending tls close_notify",
            "unexpected-eof",
            "unexpected eof",
        ],
    )
}

fn details_contain(details: &[String], needles: &[&str]) -> bool {
    details.iter().any(|detail| {
        let detail = detail.to_lowercase();
        needles.iter().any(|needle| detail.contains(needle))
    })
}


async fn send_with_retry<F>(
    request_label: &str,
    mut build_request: F,
) -> anyhow::Result<reqwest::Response>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    let not_sent_prefix = format!("{request_label}请求未发出");
    let mut last_error = None;
    for attempt in 0..3 {
        match build_request().send().await {
            Ok(response) => return Ok(response),
            Err(err) if is_retryable_request_error(&err) && attempt < 2 => {
                last_error = Some(err);
                tokio::time::sleep(std::time::Duration::from_millis(600 * (attempt + 1) as u64))
                    .await;
            }
            Err(err) => {
                return Err(anyhow::anyhow!(describe_request_error(
                    &not_sent_prefix,
                    err
                )));
            }
        }
    }

    Err(anyhow::anyhow!(describe_request_error(
        &not_sent_prefix,
        last_error.expect("retry loop stores the last request error")
    )))
}
