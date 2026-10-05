#![cfg(unix)]

use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn run_repairs_missing_runtime_dependency_once() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("nat-repair-test-{}-{stamp}", std::process::id()));
    let bin = root.join("bin");
    fs::create_dir_all(root.join("spec")).unwrap();
    fs::create_dir(&bin).unwrap();
    fs::write(root.join("spec/app.nat"), "Print a greeting.\n").unwrap();

    let bad = json!({
        "summary": "A greeter with an unavailable dependency",
        "requirements": [{"id": "R1", "source": "app.nat", "statement": "Print a greeting."}],
        "assumptions": [],
        "files": [{"path": "bad.py", "content": "import __nat_missing_runtime_module__\n"}],
        "run": ["python3", "bad.py"],
        "checks": [["python3", "-c", "print('check')"]]
    });
    let good = json!({
        "summary": "A working greeter",
        "requirements": [{"id": "R1", "source": "app.nat", "statement": "Print a greeting."}],
        "assumptions": [],
        "files": [{"path": "app.py", "content": "print('Hello world!')\n"}],
        "run": ["python3", "app.py"],
        "checks": [["python3", "-c", "print('check')"]]
    });
    let bad_path = root.join("bad.json");
    let good_path = root.join("good.json");
    fs::write(&bad_path, serde_json::to_vec(&bad).unwrap()).unwrap();
    fs::write(&good_path, serde_json::to_vec(&good).unwrap()).unwrap();

    let executable = env!("CARGO_BIN_EXE_nat");
    let built = Command::new(executable)
        .args(["build", "--proposal"])
        .arg(&bad_path)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );

    let fake_codex = bin.join("codex");
    fs::write(
        &fake_codex,
        "#!/bin/sh\nout=\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '-o' ]; then shift; out=\"$1\"; fi\n  shift\ndone\ncat >/dev/null\ncp \"$NAT_TEST_PROPOSAL\" \"$out\"\n",
    )
    .unwrap();
    fs::set_permissions(&fake_codex, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let outcome = Command::new(executable)
        .arg("run")
        .current_dir(&root)
        .env("PATH", path)
        .env("NAT_TEST_PROPOSAL", &good_path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&outcome.stdout);
    let stderr = String::from_utf8_lossy(&outcome.stderr);
    assert!(outcome.status.success(), "{stderr}");
    assert!(stdout.contains("Hello world!"), "{stdout}");
    assert!(stderr.contains("recompiling for this machine"), "{stderr}");
    assert!(root.join("generated/app.py").exists());
    assert!(!root.join("generated/bad.py").exists());
    fs::remove_dir_all(root).unwrap();
}
