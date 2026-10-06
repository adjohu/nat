#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct BuildProcess(Child);

impl Drop for BuildProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn reports_progress_before_compiler_finishes_and_preserves_errors() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("nat-progress-{}-{stamp}", std::process::id()));
    let bin = root.join("bin");
    fs::create_dir_all(root.join("spec")).unwrap();
    fs::create_dir(&bin).unwrap();
    fs::write(
        root.join("spec/hello.nat"),
        "# Greeting\n\nGreet the world.\n",
    )
    .unwrap();
    let fake_codex = bin.join("codex");
    fs::write(
        &fake_codex,
        r#"#!/bin/sh
out=
while [ "$#" -gt 0 ]; do
  if [ "$1" = '-o' ]; then shift; out="$1"; fi
  shift
done
cat >/dev/null
if [ "$NAT_FAIL" = 1 ]; then
  echo 'compiler diagnostic for the user' >&2
  exit 9
fi
attempts=0
while [ ! -f "$NAT_RELEASE" ]; do
  attempts=$((attempts + 1))
  if [ "$attempts" -gt 200 ]; then exit 1; fi
  sleep 0.1
done
cp "$NAT_TEST_PROPOSAL" "$out"
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_codex, fs::Permissions::from_mode(0o755)).unwrap();
    let release = root.join("release");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/hello/compilation.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_nat"));
    command
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
        .env("NAT_RELEASE", &release)
        .env("NAT_TEST_PROPOSAL", fixture)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut build = BuildProcess(command.spawn().unwrap());
    let stderr = build.0.stderr.take().unwrap();
    let (send, receive) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut output = String::new();
        for line in BufReader::new(stderr).lines() {
            let line = line.unwrap();
            output.push_str(&line);
            output.push('\n');
            let _ = send.send(line);
        }
        output
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let line = receive
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("build should immediately report its phase");
        if line.contains("Compiling all 1 spec(s) with Codex") {
            break;
        }
    }
    let line = receive
        .recv_timeout(Duration::from_secs(15))
        .expect("a silent compiler should still produce elapsed-time updates");
    assert!(line.contains("Still compiling with Codex"), "{line}");
    assert!(line.contains("s elapsed"), "{line}");
    assert!(
        build.0.try_wait().unwrap().is_none(),
        "progress must arrive before completion"
    );
    fs::write(release, "continue").unwrap();
    assert!(build.0.wait().unwrap().success());
    let output = reader.join().unwrap();
    assert!(output.contains("Compiler finished in"), "{output}");
    assert!(output.contains("validating output"), "{output}");

    let failed = command.env("NAT_FAIL", "1").output().unwrap();
    assert!(!failed.status.success());
    let error = String::from_utf8_lossy(&failed.stderr);
    assert!(error.contains("Codex compilation failed"), "{error}");
    assert!(
        error.contains("compiler diagnostic for the user"),
        "{error}"
    );
    assert!(!error.contains("Compiler finished in"), "{error}");
    fs::remove_dir_all(root).unwrap();
}
