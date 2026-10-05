#![cfg(unix)]

use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn one_spec_edit_updates_only_its_output() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("nat-incremental-{}-{stamp}", std::process::id()));
    let spec = root.join("spec");
    let bin = root.join("bin");
    fs::create_dir_all(&spec).unwrap();
    fs::create_dir(&bin).unwrap();
    fs::write(spec.join("a.nat"), "ORIGINAL_A_SOURCE\n").unwrap();
    fs::write(spec.join("b.nat"), "UNIQUE_B_SOURCE_CONTENT\n").unwrap();
    fs::write(spec.join("c.nat"), "UNIQUE_C_SOURCE_CONTENT\n").unwrap();

    let full = json!({
        "summary": "Three independent commands",
        "requirements": [
            {"id": "RA", "source": "a.nat", "statement": "A command"},
            {"id": "RB", "source": "b.nat", "statement": "B command"},
            {"id": "RC", "source": "c.nat", "statement": "C command"}
        ],
        "assumptions": [],
        "files": [
            {"path": "a.py", "content": "print('old a')\n", "sources": ["a.nat"]},
            {"path": "b.py", "content": "print('UNIQUE_B_OUTPUT')\n", "sources": ["b.nat"]},
            {"path": "c.py", "content": "print('UNIQUE_C_OUTPUT')\n", "sources": ["c.nat"]}
        ],
        "run": ["python3", "a.py"],
        "checks": [["python3", "-m", "compileall", "-q", "a.py", "b.py", "c.py"]]
    });
    let full_path = root.join("full.json");
    fs::write(&full_path, serde_json::to_vec(&full).unwrap()).unwrap();
    let binary = env!("CARGO_BIN_EXE_nat");
    let initial = Command::new(binary)
        .args(["build", "--proposal"])
        .arg(&full_path)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        initial.status.success(),
        "{}",
        String::from_utf8_lossy(&initial.stderr)
    );
    let b_inode = fs::metadata(root.join("generated/b.py")).unwrap().ino();
    let c_inode = fs::metadata(root.join("generated/c.py")).unwrap().ino();

    fs::write(spec.join("a.nat"), "CHANGED_A_SOURCE\n").unwrap();
    let patch = json!({
        "full_rebuild_required": false,
        "reason": "",
        "summary": "Three independent commands, A updated",
        "requirements": [{"id": "RA", "source": "a.nat", "statement": "A command updated"}],
        "assumptions": [],
        "files": [{"path": "a.py", "content": "print('new a')\n", "sources": ["a.nat"]}],
        "remove_files": [],
        "run": ["python3", "a.py"],
        "checks": [["python3", "-m", "compileall", "-q", "a.py", "b.py", "c.py"]]
    });
    let patch_path = root.join("patch.json");
    fs::write(&patch_path, serde_json::to_vec(&patch).unwrap()).unwrap();
    let fake_codex = bin.join("codex");
    fs::write(
        &fake_codex,
        "#!/bin/sh\nout=\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '-o' ]; then shift; out=\"$1\"; fi\n  shift\ndone\ncat >\"$NAT_CAPTURE\"\ncp \"$NAT_TEST_PROPOSAL\" \"$out\"\n",
    )
    .unwrap();
    fs::set_permissions(&fake_codex, fs::Permissions::from_mode(0o755)).unwrap();
    let captured = root.join("prompt.txt");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let updated = Command::new(binary)
        .arg("build")
        .current_dir(&root)
        .env("PATH", path)
        .env("NAT_CAPTURE", &captured)
        .env("NAT_TEST_PROPOSAL", &patch_path)
        .output()
        .unwrap();
    assert!(
        updated.status.success(),
        "{}",
        String::from_utf8_lossy(&updated.stderr)
    );
    assert_eq!(
        fs::read_to_string(root.join("generated/a.py")).unwrap(),
        "print('new a')\n"
    );
    assert_eq!(
        fs::metadata(root.join("generated/b.py")).unwrap().ino(),
        b_inode
    );
    assert_eq!(
        fs::metadata(root.join("generated/c.py")).unwrap().ino(),
        c_inode
    );
    let prompt = fs::read_to_string(&captured).unwrap();
    assert!(prompt.contains("CHANGED_A_SOURCE"));
    assert!(prompt.contains("print('old a')"));
    assert!(!prompt.contains("UNIQUE_B_SOURCE_CONTENT"));
    assert!(!prompt.contains("UNIQUE_C_SOURCE_CONTENT"));
    assert!(!prompt.contains("UNIQUE_B_OUTPUT"));
    assert!(!prompt.contains("UNIQUE_C_OUTPUT"));
    let ir: Value =
        serde_json::from_slice(&fs::read(root.join("generated/ir.json")).unwrap()).unwrap();
    assert_eq!(ir["requirements"].as_array().unwrap().len(), 3);
    assert!(ir["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == "RB"));
    let checked = Command::new(binary)
        .arg("check")
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    fs::remove_file(&captured).unwrap();
    let unchanged = Command::new(binary)
        .arg("build")
        .current_dir(&root)
        .env(
            "PATH",
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("NAT_CAPTURE", &captured)
        .env("NAT_TEST_PROPOSAL", &patch_path)
        .output()
        .unwrap();
    assert!(unchanged.status.success());
    assert!(!captured.exists(), "unchanged build should not call Codex");

    fs::write(spec.join("a.nat"), "A_SECOND_CHANGE\n").unwrap();
    let mut bad_patch = patch;
    bad_patch["files"] = json!([{
        "path": "b.py", "content": "print('wrong')\n", "sources": ["b.nat"]
    }]);
    let bad_path = root.join("bad-patch.json");
    fs::write(&bad_path, serde_json::to_vec(&bad_patch).unwrap()).unwrap();
    let rejected = Command::new(binary)
        .args(["build", "--proposal"])
        .arg(&bad_path)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert_eq!(
        fs::metadata(root.join("generated/b.py")).unwrap().ino(),
        b_inode
    );

    let mut replacement_patch = bad_patch;
    replacement_patch["summary"] = json!("A output renamed");
    replacement_patch["requirements"][0]["statement"] = json!("A output renamed");
    replacement_patch["files"] = json!([{
        "path": "new_a.py", "content": "print('renamed a')\n", "sources": ["a.nat"]
    }]);
    replacement_patch["remove_files"] = json!(["a.py"]);
    replacement_patch["run"] = json!(["python3", "new_a.py"]);
    replacement_patch["checks"] = json!([[
        "python3",
        "-m",
        "compileall",
        "-q",
        "new_a.py",
        "b.py",
        "c.py"
    ]]);
    let replacement_path = root.join("replacement-patch.json");
    fs::write(
        &replacement_path,
        serde_json::to_vec(&replacement_patch).unwrap(),
    )
    .unwrap();
    let replaced = Command::new(binary)
        .args(["build", "--proposal"])
        .arg(&replacement_path)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        replaced.status.success(),
        "{}",
        String::from_utf8_lossy(&replaced.stderr)
    );
    assert!(!root.join("generated/a.py").exists());
    assert_eq!(
        fs::read_to_string(root.join("generated/new_a.py")).unwrap(),
        "print('renamed a')\n"
    );
    assert_eq!(
        fs::metadata(root.join("generated/b.py")).unwrap().ino(),
        b_inode
    );

    let mut full_again = full;
    full_again["requirements"][0]["statement"] = json!("A command changed again");
    full_again["files"][0]["content"] = json!("print('full a')\n");
    let full_again_path = root.join("full-again.json");
    fs::write(&full_again_path, serde_json::to_vec(&full_again).unwrap()).unwrap();
    let forced = Command::new(binary)
        .args(["build", "--full"])
        .current_dir(&root)
        .env(
            "PATH",
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("NAT_CAPTURE", &captured)
        .env("NAT_TEST_PROPOSAL", &full_again_path)
        .output()
        .unwrap();
    assert!(
        forced.status.success(),
        "{}",
        String::from_utf8_lossy(&forced.stderr)
    );
    assert!(fs::read_to_string(&captured)
        .unwrap()
        .contains("UNIQUE_B_SOURCE_CONTENT"));
    assert_eq!(
        fs::read_to_string(root.join("generated/a.py")).unwrap(),
        "print('full a')\n"
    );

    fs::remove_dir_all(root).unwrap();
}
