#![cfg(unix)]

use fleet_daemon::{
    build_host_adapter, AntigravityDesktopAdapter, CindyAdapter, DeepSeekHarnessAdapter,
    DesktopHostAdapter, HostAdapterError, OpenAiDotsAdapter,
};
use fleet_proto::{
    CreateSessionRequest, DeliverTurnRequest, DeliveryTier, DesktopHostKind,
};
use std::{
    fs,
    os::unix::fs::symlink,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct HostFixture {
    root: PathBuf,
    projects_root: PathBuf,
    state_root: PathBuf,
    outside_dir: PathBuf,
}

impl HostFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "fleet-host-adapter-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let projects_root = root.join("Projects");
        let state_root = root.join("state");
        let outside_dir = root.join("outside");
        fs::create_dir_all(&projects_root).unwrap();
        fs::create_dir_all(&state_root).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        Self {
            root,
            projects_root,
            state_root,
            outside_dir,
        }
    }
}

impl Drop for HostFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn host_adapter_all_four_hosts_create_project_register_sidebar_and_deliver_turns() {
    let f = HostFixture::new();
    for kind in DesktopHostKind::ALL {
        let slug = format!("demo-{}", kind.as_str());
        let mut adapter =
            build_host_adapter(kind, f.projects_root.clone(), f.state_root.clone()).unwrap();
        assert_eq!(adapter.host_kind(), kind);
        assert_eq!(adapter.projects_root(), f.projects_root.as_path());

        let dir_first = adapter.ensure_project_dir(&slug).unwrap();
        assert!(dir_first.created);
        assert!(dir_first.path.is_dir());
        assert_eq!(dir_first.path, f.projects_root.join(&slug));

        let dir_second = adapter.ensure_project_dir(&slug).unwrap();
        assert!(!dir_second.created);
        assert_eq!(dir_second.path, dir_first.path);

        let binding = adapter
            .ensure_sidebar_project(&slug, Some("Self Dev Workspace"))
            .unwrap();
        assert_eq!(binding.host, kind);
        assert_eq!(binding.project_dir, dir_first.path);
        assert_ne!(binding.project_id, "default-cli-project");
        assert_eq!(
            binding.workspace_uris,
            vec![format!("file://{}", dir_first.path.display())]
        );

        let create_req = CreateSessionRequest {
            delivery_id: format!("deliv-create-{}", kind.as_str()),
            project_id: binding.project_id.clone(),
            project_dir: binding.project_dir.clone(),
            title: "Slice I Lifecycle".into(),
            prompt: "Initialize project structure and run checks.".into(),
            model: Some("dispatcher-configured-model".into()),
        };
        let created = adapter.create_session(&create_req).unwrap();
        assert_eq!(created.host, kind);
        assert_eq!(created.project_id, binding.project_id);
        assert_eq!(created.project_dir, binding.project_dir);
        assert_eq!(created.workspace_uris, binding.workspace_uris);
        assert_eq!(created.tier, DeliveryTier::Accepted);

        let followup_req = DeliverTurnRequest {
            delivery_id: format!("deliv-turn-{}", kind.as_str()),
            project_id: binding.project_id.clone(),
            session_id: created.session_id.clone(),
            project_dir: binding.project_dir.clone(),
            prompt: "Follow up on the same sidebar thread and verify receipt.".into(),
            model: None,
        };
        let followup = adapter.deliver_turn(&followup_req).unwrap();
        assert_eq!(followup.host, kind);
        assert_eq!(followup.session_id, created.session_id);
        assert_eq!(followup.project_id, binding.project_id);
        assert_ne!(followup.turn_id, created.turn_id);
        assert_eq!(followup.tier, DeliveryTier::Accepted);
        println!(
            "HOST={} project_id={} session_id={} turns=[{}, {}] dir={}",
            kind,
            binding.project_id,
            created.session_id,
            created.turn_id,
            followup.turn_id,
            binding.project_dir.display()
        );
    }
}

#[test]
fn host_adapter_rejects_traversal_symlinks_and_out_of_bounds_paths_across_all_hosts() {
    let f = HostFixture::new();
    let symlink_slug = "escape-link";
    symlink(&f.outside_dir, f.projects_root.join(symlink_slug)).unwrap();

    for kind in DesktopHostKind::ALL {
        let mut adapter =
            build_host_adapter(kind, f.projects_root.clone(), f.state_root.clone()).unwrap();

        for invalid_slug in [
            "",
            ".",
            "..",
            "../outside",
            "nested/child",
            "nested\\child",
            "/tmp/abs",
            ".hidden",
            ".cache",
            ".anet",
            "PRIVATE",
            symlink_slug,
        ] {
            let res = adapter.ensure_project_dir(invalid_slug);
            assert!(
                res.is_err(),
                "host {kind} unexpectedly allowed invalid slug {invalid_slug:?}"
            );
            assert!(
                adapter.ensure_sidebar_project(invalid_slug, None).is_err(),
                "host {kind} unexpectedly registered invalid slug {invalid_slug:?}"
            );
        }
        assert!(
            !f.root.join("outside-created").exists(),
            "no out-of-bounds directory may be created"
        );
    }
}

#[test]
fn host_adapter_enforces_sidebar_binding_and_session_continuity_invariants() {
    let f = HostFixture::new();
    let mut agy =
        AntigravityDesktopAdapter::new(f.projects_root.clone(), f.state_root.clone()).unwrap();
    let mut dots =
        OpenAiDotsAdapter::new(f.projects_root.clone(), f.state_root.clone()).unwrap();
    let mut dsh =
        DeepSeekHarnessAdapter::new(f.projects_root.clone(), f.state_root.clone()).unwrap();
    let mut cindy = CindyAdapter::new(f.projects_root.clone(), f.state_root.clone()).unwrap();

    let project_a = agy.ensure_sidebar_project("proj-a", Some("Project A")).unwrap();
    let project_b = agy.ensure_sidebar_project("proj-b", Some("Project B")).unwrap();

    // 1. Rejects default-cli-project or unregistered project_id
    for bad_project_id in ["default-cli-project", "outside-of-project", "unregistered-id"] {
        let err = agy
            .create_session(&CreateSessionRequest {
                delivery_id: format!("bad-{bad_project_id}"),
                project_id: bad_project_id.into(),
                project_dir: project_a.project_dir.clone(),
                title: "Bad Project".into(),
                prompt: "Run checks.".into(),
                model: None,
            })
            .unwrap_err();
        assert!(matches!(
            err,
            HostAdapterError::ForbiddenProjectId(_) | HostAdapterError::UnregisteredProject { .. }
        ));
    }

    // 2. Rejects mismatched project_dir vs registered project_id
    let mismatch_err = agy
        .create_session(&CreateSessionRequest {
            delivery_id: "deliv-mismatch".into(),
            project_id: project_a.project_id.clone(),
            project_dir: project_b.project_dir.clone(),
            title: "Wrong Dir".into(),
            prompt: "Run checks.".into(),
            model: None,
        })
        .unwrap_err();
    assert!(matches!(
        mismatch_err,
        HostAdapterError::ProjectDirMismatch { .. }
    ));

    // 3. Rejects out-of-bounds project_dir in create_session
    let outside_err = agy
        .create_session(&CreateSessionRequest {
            delivery_id: "deliv-outside".into(),
            project_id: project_a.project_id.clone(),
            project_dir: f.outside_dir.clone(),
            title: "Outside Dir".into(),
            prompt: "Run checks.".into(),
            model: None,
        })
        .unwrap_err();
    assert!(matches!(outside_err, HostAdapterError::Path(_)));

    // 4. Rejects brief policy violations (MODEL_WRONG / model self-attestation) before claiming delivery_id
    let brief_err = agy
        .create_session(&CreateSessionRequest {
            delivery_id: "deliv-valid-after-brief".into(),
            project_id: project_a.project_id.clone(),
            project_dir: project_a.project_dir.clone(),
            title: "Brief Gate".into(),
            prompt: "Confirm your model first or return MODEL_WRONG.".into(),
            model: None,
        })
        .unwrap_err();
    assert!(matches!(brief_err, HostAdapterError::Brief(_)));

    // Same delivery_id still succeeds when brief is valid
    let session = agy
        .create_session(&CreateSessionRequest {
            delivery_id: "deliv-valid-after-brief".into(),
            project_id: project_a.project_id.clone(),
            project_dir: project_a.project_dir.clone(),
            title: "Valid Session".into(),
            prompt: "Implement the feature and run tests.".into(),
            model: None,
        })
        .unwrap();

    // 5. Duplicate delivery_id is rejected
    let dup_err = agy
        .deliver_turn(&DeliverTurnRequest {
            delivery_id: "deliv-valid-after-brief".into(),
            project_id: project_a.project_id.clone(),
            session_id: session.session_id.clone(),
            project_dir: project_a.project_dir.clone(),
            prompt: "Another prompt.".into(),
            model: None,
        })
        .unwrap_err();
    assert!(matches!(dup_err, HostAdapterError::DuplicateDelivery(_)));

    // 6. Cross-project session continuation is rejected
    let cross_proj_err = agy
        .deliver_turn(&DeliverTurnRequest {
            delivery_id: "deliv-cross-project".into(),
            project_id: project_b.project_id.clone(),
            session_id: session.session_id.clone(),
            project_dir: project_b.project_dir.clone(),
            prompt: "Try to hijack session into project B.".into(),
            model: None,
        })
        .unwrap_err();
    assert!(matches!(
        cross_proj_err,
        HostAdapterError::SessionProjectMismatch { .. }
    ));

    // 7. Unknown session continuation is rejected
    let unknown_sess_err = agy
        .deliver_turn(&DeliverTurnRequest {
            delivery_id: "deliv-unknown-sess".into(),
            project_id: project_a.project_id.clone(),
            session_id: "nonexistent-session".into(),
            project_dir: project_a.project_dir.clone(),
            prompt: "Follow up on missing session.".into(),
            model: None,
        })
        .unwrap_err();
    assert!(matches!(
        unknown_sess_err,
        HostAdapterError::UnknownSession { .. }
    ));

    // Verify persistence across adapter reload and other typed adapters
    let reloaded =
        AntigravityDesktopAdapter::new(f.projects_root.clone(), f.state_root.clone()).unwrap();
    assert_eq!(
        reloaded
            .session(&session.session_id)
            .unwrap()
            .turns
            .len(),
        1
    );
    assert!(dots.ensure_sidebar_project("dots-ok", None).is_ok());
    assert!(dsh.ensure_sidebar_project("dsh-ok", None).is_ok());
    assert!(cindy.ensure_sidebar_project("cindy-ok", None).is_ok());
}
