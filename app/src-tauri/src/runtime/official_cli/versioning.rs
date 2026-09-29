pub(super) fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    parse_version(left).cmp(&parse_version(right))
}

pub(super) fn parse_version(version: &str) -> Vec<u64> {
    version
        .trim_start_matches('v')
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}
