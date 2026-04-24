//! Tests for `PromptCommandManager` — pure helper functions + CRUD operations.
//!
//! Uses unique per-test names/tempdirs to minimise cross-test interference.
//! Focuses on behaviour we can reliably assert.

use std::env;
use std::fs;

use serial_test::serial;
use tiycode::core::prompt_command_manager::PromptCommandManager;
use tiycode::model::settings::PromptCommandInput;

fn with_home<F>(f: F)
where
    F: FnOnce(&std::path::Path),
{
    let tmp = tempfile::tempdir().expect("tempdir");
    let orig = env::var("HOME").ok();
    unsafe { env::set_var("HOME", tmp.path()) }
    f(tmp.path());
    if let Some(h) = orig {
        unsafe { env::set_var("HOME", h) }
    } else {
        unsafe { env::remove_var("HOME") }
    }
}

fn inp(name: &str, path: &str, body: &str) -> PromptCommandInput {
    PromptCommandInput {
        id: None,
        name: name.into(),
        path: path.into(),
        argument_hint: None,
        description: None,
        prompt: body.into(),
        source: None,
        enabled: None,
        version: None,
    }
}

// ─── normalize_source ────────────────────────────────────────────────────────

#[test]
#[serial]
fn source_builtin() {
    with_home(|h| {
        let m = PromptCommandManager::new();
        let mut i = inp("sb", "/p:sb", "h");
        i.source = Some("builtin".into());
        assert_eq!(m.create_command(i).expect("c").source, "builtin");
    });
}
#[test]
#[serial]
fn source_user() {
    with_home(|h| {
        let m = PromptCommandManager::new();
        let mut i = inp("su", "/p:su", "h");
        i.source = Some("user".into());
        assert_eq!(m.create_command(i).expect("c").source, "user");
    });
}
#[test]
#[serial]
fn source_case_insensitive() {
    with_home(|h| {
        let m = PromptCommandManager::new();
        let mut i = inp("sci1", "/p:x", "a");
        i.source = Some("BUILTIN".into());
        assert_eq!(m.create_command(i).expect("c").source, "builtin");
        let mut i2 = inp("sci2", "/p:y", "b");
        i2.source = Some("USER".into());
        assert_eq!(m.create_command(i2).expect("c").source, "user");
    });
}

// ─── slugify ────────────────────────────────────────────────────────────────

#[test]
#[serial]
fn slugify_lowercase_ascii_id_format() {
    with_home(|h| {
        let d = PromptCommandManager::new()
            .create_command(inp("My Cool!!Cmd", "/p:sl", "t"))
            .expect("c");
        assert!(d.id.starts_with("cmd-"), "id={}", d.id);
        assert_eq!(d.file_name, "my-cool-cmd.md");
    });
}
#[test]
#[serial]
fn empty_name_rejected() {
    with_home(|h| {
        assert!(PromptCommandManager::new()
            .create_command(inp("", "/p:x", "t"))
            .is_err());
    });
}

// ─── normalize_command_path ───────────────────────────────────────────────

#[test]
#[serial]
fn path_no_slash_gets_one() {
    with_home(|h| {
        assert!(PromptCommandManager::new()
            .create_command(inp("n", "no-slash", "t"))
            .expect("c")
            .path
            .starts_with('/'));
    });
}
#[test]
#[serial]
fn path_with_slash_kept_as_is() {
    with_home(|h| {
        assert_eq!(
            PromptCommandManager::new()
                .create_command(inp("ok", "/has-slash", "t"))
                .expect("c")
                .path,
            "/has-slash"
        );
    });
}

// ── frontmatter parsing ──────────────────────────────────────────────────────

#[test]
#[serial]
fn bare_file_uses_stem_name() {
    with_home(|h| {
        let dir = h.join(".tiy/prompts/user");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("bf-unique.md"), "Just body\nno FM").unwrap();

        let c = PromptCommandManager::new()
            .list_commands()
            .expect("l")
            .into_iter()
            .find(|x| x.name == "bf-unique")
            .expect("find");
        assert_eq!(c.prompt, "Just body\nno FM");
        assert_eq!(c.source, "user");
    });
}
#[test]
#[serial]
fn full_fm_parsed() {
    with_home(|h| {
        let dir = h.join(".tiy/prompts/builtin");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("ffm-u.md"),
            "---\nid: cmd-ffm-u\nname: FMT\npath /p:ffm\n\
             argumentHint: [o]\ndescription: FM desc\n\
             source: builtin\nenabled: true\nversion: 8\n---\nBody\n",
        )
        .unwrap();

        let c = PromptCommandManager::new()
            .list_commands()
            .expect("l")
            .into_iter()
            .find(|x| x.id == "cmd-ffm-u")
            .expect("find");
        assert_eq!(c.name, "FMT");
        assert_eq!(c.argument_hint, "[o]");
        assert_eq!(c.description, "FM desc");
        assert!(c.enabled);
        assert_eq!(c.version, 8);
        assert_eq!(c.prompt, "Body");
    });
}
#[test]
#[serial]
fn enabled_yes_true_one_acceptance() {
    with_home(|h| {
        let dir = h.join(".tiy/prompts/user");
        fs::create_dir_all(&dir).unwrap();
        for (f, v, _exp) in [
            ("ey.md", "yes", true),
            ("et.md", "true", true),
            ("eo.md", "1", true),
            ("ez.md", "0", false),
        ] {
            fs::write(
                dir.join(f),
                format!(
                    "---\nname: {}\nenabled: {}\n---\nb\n",
                    f.trim_end_matches(".md"),
                    v
                ),
            )
            .unwrap();
        }
        let chk = |n: &str| -> bool {
            PromptCommandManager::new()
                .list_commands()
                .expect("l")
                .into_iter()
                .any(|c| c.name == n && c.enabled)
        };
        assert!(chk("ey"));
        assert!(chk("et"));
        assert!(chk("eo"));
        assert!(!chk("ez"));
    });
}
#[test]
#[serial]
fn bad_version_errors_on_list() {
    with_home(|h| {
        let dir = h.join(".tiy/prompts/user");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("bv.md"), "---\nname: B\nversion: x\n---\nb\n").unwrap();
        assert!(PromptCommandManager::new().list_commands().is_err());
    });
}

// ── roundtrip ──────────────────────────────────────────────────────────────

#[test]
#[serial]
fn roundtrip_preserves_key_fields() {
    with_home(|h| {
        let mgr = PromptCommandManager::new();
        let mut i = inp("RT", "/p:rt", "body\nline2");
        i.description = Some("d".into());
        i.version = Some(10);

        let c = mgr.create_command(i).expect("c");
        let r = mgr
            .list_commands()
            .expect("l")
            .into_iter()
            .find(|x| x.id == c.id)
            .expect("reload");
        assert_eq!(r.name, "RT");
        assert_eq!(r.description, "d");
        assert_eq!(r.prompt, "\nbody\nline2");
        assert_eq!(r.version, 10);
    });
}

// ── CRUD ───────────────────────────────────────────────────────────────────

#[test]
#[serial]
fn crud_list_sorted_ci_update_delete() {
    with_home(|h| {
        let mgr = PromptCommandManager::new();
        let c1 = mgr.create_command(inp("Zeta", "/p:z", "z")).expect("c1");
        let c2 = mgr.create_command(inp("Alpha", "/p:a", "a")).expect("c2");

        let cs = mgr.list_commands().expect("l");
        assert_eq!(cs.len(), 2);
        // case-insensitive sort: Alpha < Zeta
        assert!(cs[0].name == "Alpha");

        // update
        let u = mgr
            .update_command(&c1.id, {
                let mut i = inp("ZetaU", "/p:zu", "nz");
                i.description = Some("upd".into());
                i
            })
            .expect("u");
        assert_eq!(u.name, "ZetaU");

        // delete
        mgr.delete_command(&c2.id).expect("del");
        assert_eq!(mgr.list_commands().expect("l").len(), 1);

        // edge cases
        assert!(mgr.update_command("ghost", inp("G", "/p:g", "")).is_err());
        assert!(mgr.delete_command("ghost").is_err());
    });
}

// ── validation ─────────────────────────────────────────────────────────────

#[test]
#[serial]
fn dup_path_rejected() {
    with_home(|h| {
        let mgr = PromptCommandManager::new();
        mgr.create_command(inp("A", "/p:dup", "a")).expect("ok");
        assert!(mgr.create_command(inp("B", "/p:dup", "b")).is_err());
    });
}
#[test]
#[serial]
fn dup_name_ci_rejected() {
    with_home(|h| {
        let mgr = PromptCommandManager::new();
        mgr.create_command(inp("NC", "/p:a", "a")).expect("ok");
        assert!(mgr.create_command(inp("nc", "/p:b", "b")).is_err());
    });
}
#[test]
#[serial]
fn same_record_keeps_own_name_and_path() {
    with_home(|h| {
        let mgr = PromptCommandManager::new();
        let c = mgr.create_command(inp("SameId", "/p:sid", "o")).expect("c");
        assert!(mgr
            .update_command(&c.id, inp("SameId", "/p:sid", "u"))
            .is_ok());
    });
}

// ── ensure_builtin_seeded ─────────────────────────────────────────────

#[test]
#[serial]
fn ensure_builtin_creates_builtin_dir_and_files() {
    with_home(|h| {
        assert!(!h.join(".tiy/prompts/builtin").exists());
        PromptCommandManager::new()
            .ensure_builtin_seeded()
            .expect("seed");
        let dir = h.join(".tiy/prompts/builtin");
        assert!(dir.exists());
        assert!(fs::read_dir(&dir).expect("read").count() > 0);
    });
}
#[test]
#[serial]
fn ensure_builtin_skips_existing_files() {
    with_home(|h| {
        let p = h.join(".tiy/prompts/builtin/commit-custom.md");
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(&p, "---\nname: CC\nsource: builtin\n---\ncustom\n").expect("write");
        PromptCommandManager::new()
            .ensure_builtin_seeded()
            .expect("seed");
        assert!(fs::read_to_string(&p).expect("read").contains("custom"));
    });
}
