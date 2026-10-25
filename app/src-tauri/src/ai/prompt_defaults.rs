use sha2::{Digest, Sha256};

// Exact complete defaults verified in reachable source history. The companion metadata
// records provenance without embedding historical prompt text in application source.
const HISTORICAL_ANALYSIS_DEFAULTS: &[&str] = &[
    "08cc847c40c285bdfb564e42f832f1e76610a335bd7ae5d45f36cb2be313bd5b",
    "10f8e5628fa340f32925700357273443e353f21df3d01626abeacab2c5702886",
    "1800d4387d5940097fa4a3f66efbbec4a13b04e799f0d6d96784eb69d1820e28",
    "6b14de2e705de27f86df06e524bf85497aefd4e8cdaa7e78afc78c75740e0bfb",
    "8e992ce0c0ea5ef4269030e77f3a8605522ad9c4696c2a444db56357c361569c",
    "cb94eb8398298bcad495444a6dd471c3d5bf68a9479458830ed61d74c995c2a4",
    "d9bbed9a408519ffd0a0146da9a712deae5191dca35c12ff042b74d4d71c5bc6",
    "f0772ae51380a6bf500bc88157bc5a3186590baae60bf3aade9e4e20b9f8b755",
    "ff5ec201c7970d530ef3eab921055101cd7111e85f22a9ea3510f11674075702",
];

const HISTORICAL_SUMMARY_DEFAULTS: &[&str] = &[
    "3e9c044848fdd98c1128a78c0c3ef63aae495ed77c190ab68c14ed632ebc4c18",
    "51f4e0e58782d22a1f406ef785ba8850120d9061054d6ada5f8bc09ada6d2ed0",
    "5df60035b6adc0507c80b9e5adc978d4af11b58ab468e9d5950704adec228d7a",
    "8c3ab792a6f0179ee16b7c34694ef992b38d2291068f18f15e054968aee027be",
    "8d115148fb35a33b74b729102c5b9959c0667c1512262241ac564b5f2cb1d82a",
    "968896e2d6694f74d5e09fcb36985b4846a84ecadcdf231d3a62f8be1ae4b223",
    "b3f56a7ed0d7c749c0b4fb56cda2b7e1ddb55120619b8ab4b18ff1ab5ef18480",
    "bba01b449ef6884ac23a0cb2c4d382e5f5d5ff15c718a80cb792c6210f2257cc",
];

fn fingerprint(value: &str) -> String {
    // Rust Unicode whitespace at the edges is the only normalization. Internal
    // whitespace and line endings are part of the exact source fingerprint.
    format!("{:x}", Sha256::digest(value.trim().as_bytes()))
}

fn is_known_default(value: &str, current: &str, historical: &[&str]) -> bool {
    let value_fingerprint = fingerprint(value);
    value_fingerprint == fingerprint(current) || historical.contains(&value_fingerprint.as_str())
}

pub(super) fn is_known_default_analysis_prompt(value: &str) -> bool {
    is_known_default(
        value,
        crate::ai::DEFAULT_ANALYSIS_PROMPT,
        HISTORICAL_ANALYSIS_DEFAULTS,
    )
}

pub(super) fn is_known_default_summary_prompt(value: &str) -> bool {
    is_known_default(
        value,
        crate::ai::DEFAULT_SUMMARY_PROMPT,
        HISTORICAL_SUMMARY_DEFAULTS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn prompt_defaults_registry_matches_verified_metadata() {
        let metadata: serde_json::Value = serde_json::from_str(include_str!(
            "../../../shared/ai/prompt-default-fingerprints.json"
        ))
        .expect("fingerprint metadata");
        assert_eq!(metadata["schemaVersion"], 1);
        let entries = metadata["historicalDefaults"].as_array().expect("defaults");
        assert_eq!(entries.len(), 17);
        for (role, registered) in [
            ("analysis", HISTORICAL_ANALYSIS_DEFAULTS),
            ("summary", HISTORICAL_SUMMARY_DEFAULTS),
        ] {
            let declared: BTreeSet<&str> = entries
                .iter()
                .filter(|entry| entry["role"].as_str() == Some(role))
                .map(|entry| {
                    let hash = entry["trimmedSha256"].as_str().expect("hash");
                    assert_eq!(hash.len(), 64);
                    assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
                    let sources = entry["sourceEvidence"].as_array().expect("sources");
                    assert!(!sources.is_empty());
                    for source in sources {
                        assert_eq!(source["commit"].as_str().expect("commit").len(), 40);
                        assert_eq!(source["blobOid"].as_str().expect("blob").len(), 40);
                        assert!(source["line"].as_u64().expect("line") > 0);
                    }
                    hash
                })
                .collect();
            assert_eq!(declared, registered.iter().copied().collect());
            assert_eq!(declared.len(), registered.len());
        }
    }

    #[test]
    fn prompt_defaults_verified_fixture_uses_rust_trim_fingerprint() {
        let fixture = include_str!("tests/fixtures/legacy_analysis_default.txt");
        assert_eq!(fingerprint(fixture), HISTORICAL_ANALYSIS_DEFAULTS[0]);
        assert_eq!(
            fingerprint(&format!("\u{2003}\r\n{fixture}\u{3000}")),
            HISTORICAL_ANALYSIS_DEFAULTS[0]
        );
        assert!(!is_known_default_analysis_prompt(&format!(
            "{fixture}\u{200b}"
        )));
        assert!(!is_known_default_summary_prompt(fixture));
    }
}
