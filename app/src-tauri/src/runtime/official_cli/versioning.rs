pub(super) fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    parse_version(left).cmp(&parse_version(right))
}

pub fn version_is_newer(latest: &str, current: &str) -> bool {
    let latest_parts = parse_version(latest);
    let current_parts = parse_version(current);
    latest_parts > current_parts
}

pub(super) fn parse_version(version: &str) -> Vec<u64> {
    version
        .trim_start_matches('v')
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}
