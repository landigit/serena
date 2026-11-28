use codemode_memories::{MemoryStore, ReferenceGraph};
use codemode_store::ContextDb;

#[test]
fn crud_indexes_into_context_db_detects_dangling_and_propagates_rename() {
    let root = std::env::temp_dir().join(format!("codemode-memories-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let db = ContextDb::open(&root.join("context.db")).unwrap();
    let store = MemoryStore::new(&root);

    store
        .write("auth/login", "# Login Flow\nUses JWT tokens.\n", Some(&db))
        .unwrap();
    store
        .write(
            "architecture",
            "# Architecture\nSee mem:auth/login and mem:missing_topic.\nDo not touch mem:auth/login_v2.\n",
            Some(&db),
        )
        .unwrap();
    store
        .write("auth/login_v2", "# Login v2\n", Some(&db))
        .unwrap();

    // Indexed into ContextDb and searchable.
    let hits = db
        .search_fts("JWT tokens", None, Some("memory:auth/login"), 5)
        .unwrap();
    assert_eq!(hits.len(), 1);

    // List and read.
    let listed = store.list(None).unwrap();
    assert_eq!(
        listed.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["architecture", "auth/login", "auth/login_v2"]
    );
    assert!(
        store
            .read("mem:auth/login.md")
            .unwrap()
            .contains("JWT tokens")
    );

    // Reference graph detects dangling mem:missing_topic.
    let graph: ReferenceGraph = store.reference_graph().unwrap();
    assert_eq!(graph.dangling.len(), 1);
    assert_eq!(graph.dangling[0].source, "architecture");
    assert_eq!(graph.dangling[0].target, "missing_topic");

    // Rename propagates mem:auth/login -> mem:security/session without touching mem:auth/login_v2.
    let updated = store
        .rename("auth/login", "security/session", Some(&db))
        .unwrap();
    assert_eq!(updated.rewritten_references, 1);

    let arch = store.read("architecture").unwrap();
    assert!(arch.contains("mem:security/session"));
    assert!(!arch.contains("mem:auth/login "));
    assert!(arch.contains("mem:auth/login_v2"));
    assert!(
        root.join(".codemode/memories/security/session.md")
            .is_file()
    );
    assert!(!root.join(".codemode/memories/auth/login.md").exists());

    // Path traversal is rejected.
    assert!(store.write("../escape", "bad", None).is_err());

    // Delete removes file.
    store.delete("auth/login_v2", Some(&db)).unwrap();
    assert!(!root.join(".codemode/memories/auth/login_v2.md").exists());
}
