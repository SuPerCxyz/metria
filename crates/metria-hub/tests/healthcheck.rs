use metria_hub::config::HubConfig;
use metria_hub::db::HubDb;

fn config(dir: &std::path::Path) -> HubConfig {
    HubConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        data_dir: dir.to_path_buf(),
        database_url: format!("sqlite://{}/hub.db", dir.display()),
        content_mode: metria_core::ContentMode::Metadata,
        timezone: chrono_tz::Tz::UTC,
        log_filter: "error".into(),
        demo: false,
        oidc: None,
    }
}

#[test]
fn healthcheck_reads_schema_without_full_integrity_scan() {
    let dir = std::env::temp_dir().join(format!("metria-healthcheck-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = config(&dir);
    let db = HubDb::open(&cfg).unwrap();
    db.apply_migrations().unwrap();
    drop(db);

    metria_hub::healthcheck(&cfg).unwrap();
}
