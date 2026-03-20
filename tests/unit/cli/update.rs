use bendclaw::cli::update::select_asset;
use bendclaw::cli::update::tags_match;
use bendclaw::cli::update::GitHubRelease;
use bendclaw::cli::update::GitHubReleaseAsset;

#[test]
fn tags_match_ignores_v_prefix() {
    assert!(tags_match("v2026.3.13", "2026.3.13"));
    assert!(tags_match("2026.3.13", "v2026.3.13"));
    assert!(!tags_match("v2026.3.13", "v2026.3.14"));
}

#[test]
fn select_asset_prefers_exact_name() {
    let release = GitHubRelease {
        tag_name: "v2026.3.13".to_string(),
        assets: vec![
            GitHubReleaseAsset {
                name: "bendclaw-v2026.3.13-x86_64-unknown-linux-gnu.tar.gz".to_string(),
                url: "https://example.com/exact".to_string(),
            },
            GitHubReleaseAsset {
                name: "bendclaw-x86_64-unknown-linux-gnu.tar.gz".to_string(),
                url: "https://example.com/fallback".to_string(),
            },
        ],
    };

    let asset = select_asset(&release, "x86_64-unknown-linux-gnu").expect("asset");
    assert_eq!(asset.url, "https://example.com/exact");
}
