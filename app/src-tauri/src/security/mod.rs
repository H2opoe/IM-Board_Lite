const SECRET_MARKERS: &[&str] = &[
    "enc_key",
    "api_key",
    "apiKey",
    "authorization",
    "token",
    "secret",
];

pub fn sanitize_log(input: &str) -> String {
    let mut output = input.to_owned();
    for marker in SECRET_MARKERS {
        output = mask_json_like_value(&output, marker);
    }
    output
}

fn mask_json_like_value(input: &str, key: &str) -> String {
    let mut out = Vec::new();
    for line in input.lines() {
        let lower = line.to_lowercase();
        if lower.contains(&key.to_lowercase()) {
            out.push(format!("{key}=***"));
        } else {
            out.push(line.to_owned());
        }
    }
    out.join("\n")
}
