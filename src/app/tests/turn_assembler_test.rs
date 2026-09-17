use std::sync::Arc;

use evot::agent::tools::ToolMode;
use evot::agent::turn_assembler::TurnAssembler;
use evot::agent::turn_assembler::TurnBuildRequest;
use evot::conf::Config;
use evot::conf::Protocol;
use evot::conf::ProviderProfile;
use evot::sessions::Session;
use evot::sessions::SessionLocator;
use evot::storage::MemoryStorage;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn assembles_a_turn_without_an_agent() -> TestResult {
    // Assembly ensures the memory vault exists; never write into the runner's HOME.
    const CHILD: &str = "EVOT_TURN_ASSEMBLER_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let home = tempfile::tempdir()?;
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "turn_assembler_test::assembles_a_turn_without_an_agent",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env(CHILD, "1")
            .output()?;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }
    let workspace = tempfile::tempdir()?;
    let cwd = workspace.path().to_str().ok_or("non-UTF8 workspace")?;
    let mut config = Config::new(workspace.path().to_path_buf());
    config.providers.insert("fixture".into(), ProviderProfile {
        protocol: Protocol::OpenAi,
        api_key: "fixture-key".into(),
        base_url: "https://example.invalid/v1".into(),
        models: vec!["fixture-model".into()],
        compat_caps: Default::default(),
        route_capabilities: Default::default(),
        thinking_level: None,
        context_window: None,
        max_tokens: None,
        supports_image: None,
    });
    let llm = config.build_llm("fixture", Some("fixture-model".into()))?;
    let locator = SessionLocator::new("test", "standalone-assembly");
    let session = Session::open_or_create_with_provider(
        &locator,
        cwd,
        &llm.provider,
        &llm.model,
        Arc::new(MemoryStorage::new()),
    )
    .await?;
    let assembler = TurnAssembler::new(&config);
    let turn = assembler
        .build_turn(
            &llm,
            ToolMode::Readonly,
            session.clone(),
            &locator.session_id(),
            TurnBuildRequest {
                input: vec![evot_engine::Content::Text {
                    text: "hello".into(),
                }],
                host_tools: None,
                consume_process_notifications: false,
            },
        )
        .await?;
    assert!(Arc::ptr_eq(&turn.session, &session));
    assert!(turn.history.is_empty());
    assert!(!turn.options.tools.is_empty());
    assert_eq!(turn.options.model, "fixture-model");
    assert_eq!(turn.options.cwd, workspace.path());
    assert!(turn.options.process_manager.is_none());
    assert!(turn.options.limits.is_some());
    assert!(
        matches!(turn.input.as_slice(), [evot_engine::Content::Text { text }] if text == "hello")
    );
    for mode in [
        ToolMode::Headless,
        ToolMode::Interactive,
        ToolMode::Planning,
    ] {
        let prepared = assembler
            .build_turn(
                &llm,
                mode,
                session.clone(),
                &locator.session_id(),
                TurnBuildRequest {
                    input: Vec::new(),
                    host_tools: None,
                    consume_process_notifications: true,
                },
            )
            .await?;
        let interactive = matches!(mode, ToolMode::Interactive | ToolMode::Planning);
        assert_eq!(prepared.options.process_manager.is_some(), interactive);
        assert_eq!(prepared.options.limits.is_none(), interactive);
        assert!(
            matches!(prepared.input.as_slice(), [evot_engine::Content::Text { text }]
            if text == "A background task finished, but its result was already delivered. Continue from where you left off, or wait for the user.")
        );
        assert_eq!(
            prepared.options.system_prompt,
            prepared
                .options
                .system_prompt_sections
                .iter()
                .map(|section| section.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n")
        );
    }
    for missing_provider in [true, false] {
        let mut invalid = llm.clone();
        if missing_provider {
            invalid.provider.clear();
        } else {
            invalid.api_key = "  ".into();
        }
        let result = assembler
            .build_turn(
                &invalid,
                ToolMode::Readonly,
                session.clone(),
                &locator.session_id(),
                TurnBuildRequest {
                    input: Vec::new(),
                    host_tools: None,
                    consume_process_notifications: false,
                },
            )
            .await;
        let error = match result {
            Ok(_) => return Err("invalid model accepted".into()),
            Err(error) => error,
        };
        assert!(error.to_string().contains(if missing_provider {
            "No model available"
        } else {
            "No API key set"
        }));
    }
    Ok(())
}
