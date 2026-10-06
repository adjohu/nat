#![cfg(unix)]
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::{atomic::{AtomicU64, Ordering}, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("nat-cli-test-{}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), SERIAL.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&p).unwrap();
        let project = Self(p);
        project.ok(&["init"]);
        project
    }
    fn call(&self, args: &[&str]) -> Output { Command::new(env!("CARGO_BIN_EXE_nat")).args(args).current_dir(&self.0).output().unwrap() }
    fn ok(&self, args: &[&str]) -> Output {
        let result = self.call(args);
        assert!(result.status.success(), "{args:?}: {}", String::from_utf8_lossy(&result.stderr));
        result
    }
    fn proposal(&self) {
        let response = json!({"summary":"offline CLI fixture", "requirements":[{"id":"GREETING","source":"app.nat","statement":"Greet the supplied name"}],
            "assumptions":[], "files":[{"path":"description.txt","content":"greeting fixture","sources":["app.nat"]}],
            "run":["/bin/echo","Hello"], "checks":[["true"]]});
        fs::write(self.0.join("proposal.json"), serde_json::to_vec_pretty(&response).unwrap()).unwrap();
    }
    fn mock_compiler(&self) -> String {
        fs::create_dir_all(self.0.join("bin")).unwrap();
        let mock = self.0.join("bin/codex");
        fs::write(&mock, r#"#!/bin/sh
printf '%s\n' "$@" > compiler-argv.txt
pwd > compiler-cwd.txt
out=
schema=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) shift; out="$1" ;;
    --output-schema) shift; schema="$1" ;;
  esac
  shift
done
case "$out" in /*) ;; *) echo 'response path must be absolute' >&2; exit 21 ;; esac
case "$schema" in /*) ;; *) echo 'schema path must be absolute' >&2; exit 22 ;; esac
cat > compiler-prompt.txt
cp "$schema" compiler-schema.json
printf 'NORMAL_TRANSCRIPT_MUST_STAY_HIDDEN\n'
cat compiler-prompt.txt
printf '\n{"type":"warning","message":"structured compiler warning"}\n'
printf 'mock compiler diagnostic\n' >&2
if [ "$NAT_MOCK_FAIL" = yes ]; then exit 17; fi
if [ "$NAT_MOCK_WAIT" = yes ]; then
  n=0
  while [ ! -f release ]; do
    n=$((n + 1))
    if [ "$n" -gt 200 ]; then exit 23; fi
    sleep 0.1
  done
fi
cp proposal.json "$out"
"#).unwrap();
        fs::set_permissions(&mock, fs::Permissions::from_mode(0o755)).unwrap();
        format!("{}:{}", self.0.join("bin").display(), std::env::var("PATH").unwrap_or_default())
    }
}
impl Drop for Project { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
struct Watcher(Child);
impl Drop for Watcher { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
fn wait_current(project: &Project) {
    let start = Instant::now();
    loop {
        if project.call(&["check"]).status.success() { break; }
        assert!(start.elapsed() < Duration::from_secs(10), "watch did not rebuild");
        thread::sleep(Duration::from_millis(80));
    }
}
fn assert_reference_protocol(prompt: &str) {
    assert!(prompt.contains(".nat files are Markdown"));
    assert!(prompt.contains("view referenced images using read-only tools"));
    assert!(prompt.contains("ORIGINAL Markdown"));
    assert!(prompt.contains("must not write files or run generated software"));
}

#[test]
fn offline_cli_build_run_test_status_and_discovery() {
    let p = Project::new();
    p.proposal();
    let before = p.ok(&["status"]);
    assert!(String::from_utf8_lossy(&before.stdout).contains("No build"));
    p.ok(&["build", "--proposal", "proposal.json"]);
    p.ok(&["check"]);
    p.ok(&["test"]);
    let output = p.ok(&["run", "--", "a b", "$(echo unsafe)", "--project", "literal"]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Hello a b $(echo unsafe) --project literal"));
    assert!(!p.0.join("unsafe").exists());
    fs::create_dir_all(p.0.join("nested/deeper")).unwrap();
    let nested = Command::new(env!("CARGO_BIN_EXE_nat")).arg("check").current_dir(p.0.join("nested/deeper")).output().unwrap();
    assert!(nested.status.success());
    let explicit = Command::new(env!("CARGO_BIN_EXE_nat")).args(["check", "--project", "../.."]).current_dir(p.0.join("nested/deeper")).output().unwrap();
    assert!(explicit.status.success());
    fs::write(p.0.join("generated/description.txt"), "manual edit").unwrap();
    assert!(!p.call(&["check"]).status.success());
    let status = p.ok(&["status"]);
    assert!(String::from_utf8_lossy(&status.stdout).contains("Artifact drift"));
    assert!(!p.call(&["build", "--proposal", "proposal.json", "--full"]).status.success());
}
#[test]
fn current_build_still_validates_and_applies_explicit_proposals() {
    let p = Project::new();
    p.proposal();
    p.ok(&["build", "--proposal", "proposal.json"]);
    let path = p.0.join("proposal.json");
    let mut response: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    response["assumptions"] = json!([{"id":"A-01","source":"spec/app.nat","statement":"English","reason":"Unspecified language"}]);
    fs::write(&path, serde_json::to_vec(&response).unwrap()).unwrap();
    let failed = p.call(&["build", "--proposal", "proposal.json"]);
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("invalid assumption A-01 or source"));
    p.ok(&["check"]);
    response["assumptions"] = json!([]);
    response["files"][0]["content"] = json!("new explicit content");
    fs::write(&path, serde_json::to_vec(&response).unwrap()).unwrap();
    p.ok(&["build", "--proposal", "proposal.json"]);
    assert_eq!(fs::read_to_string(p.0.join("generated/description.txt")).unwrap(), "new explicit content");
    fs::write(&path, "invalid JSON").unwrap();
    assert!(!p.call(&["build", "--proposal", "proposal.json"]).status.success());
    p.ok(&["check"]);
    p.ok(&["build"]);
}
#[test]
fn help_version_and_errors() {
    let p = Project::new();
    for alias in ["help", "--help", "-h"] {
        let output = p.ok(&[alias]);
        let text = String::from_utf8_lossy(&output.stdout);
        for word in ["init", "build", "watch", "run", "test", "status", "check", "promote", "--project", "--full", "--no-test", "--proposal", "privileges"] {
            assert!(text.contains(word), "help lacks {word}");
        }
    }
    for alias in ["version", "--version", "-V"] {
        assert!(String::from_utf8_lossy(&p.ok(&[alias]).stdout).contains(env!("CARGO_PKG_VERSION")));
    }
    for args in [vec!["unknown"], vec!["build", "--unknown"], vec!["build", "--proposal"], vec!["init", "one", "two"], vec!["check", "--project", "missing"]] {
        let output = p.call(&args);
        assert!(!output.status.success());
        assert!(!output.stderr.is_empty());
    }
}
#[test]
fn watch_builds_at_start_and_after_changes_and_survives_errors() {
    let p = Project::new();
    p.proposal();
    let child = Command::new(env!("CARGO_BIN_EXE_nat"))
        .args(["watch", "--proposal", "proposal.json", "--no-test"])
        .current_dir(&p.0).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let mut watcher = Watcher(child);
    wait_current(&p);
    fs::write(p.0.join("spec/app.nat"), "Changed greeting").unwrap();
    wait_current(&p);
    fs::write(p.0.join("spec/app.nat"), "[context](missing.md)").unwrap();
    thread::sleep(Duration::from_millis(900));
    assert!(watcher.0.try_wait().unwrap().is_none());
    assert!(!p.call(&["check"]).status.success());
    fs::write(p.0.join("spec/missing.md"), "Supporting greeting context").unwrap();
    wait_current(&p);
    assert!(watcher.0.try_wait().unwrap().is_none());
}
#[test]
fn watch_runs_checks_by_default_and_no_test_skips_them() {
    for no_test in [false, true] {
        let p = Project::new();
        p.proposal();
        let path = p.0.join("proposal.json");
        let mut response: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        response["checks"] = json!([["/usr/bin/touch", "checks-ran"]]);
        fs::write(path, serde_json::to_vec(&response).unwrap()).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_nat"));
        command.args(["watch", "--proposal", "proposal.json"]);
        if no_test { command.arg("--no-test"); }
        let _watcher = Watcher(command.current_dir(&p.0).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
        wait_current(&p);
        if no_test {
            thread::sleep(Duration::from_millis(600));
            assert!(!p.0.join("generated/checks-ran").exists());
        } else {
            let start = Instant::now();
            while !p.0.join("generated/checks-ran").exists() {
                assert!(start.elapsed() < Duration::from_secs(5));
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}
#[test]
fn compiler_protocol_progress_diagnostics_and_transcript_suppression() {
    let p = Project::new();
    p.proposal();
    fs::write(p.0.join("spec/app.nat"), "PRIVATE_SPEC_TEXT_MUST_STAY_HIDDEN").unwrap();
    let mock_path = p.mock_compiler();
    let mut command = Command::new(env!("CARGO_BIN_EXE_nat"));
    command.args(["build", "--full"]).current_dir(&p.0).env("PATH", &mock_path)
        .env("NAT_MOCK_FAIL", "no").env("NAT_MOCK_WAIT", "yes")
        .stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut process = Watcher(command.spawn().unwrap());
    let stderr = process.0.stderr.take().unwrap();
    let stdout = process.0.stdout.take().unwrap();
    let stdout_reader = thread::spawn(move || {
        use std::io::Read;
        let mut result = String::new();
        BufReader::new(stdout).read_to_string(&mut result).unwrap();
        result
    });
    let (send, receive) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut result = String::new();
        for line in BufReader::new(stderr).lines() {
            let line = line.unwrap();
            result.push_str(&line);
            result.push('\n');
            let _ = send.send(line);
        }
        result
    });
    let first = receive.recv_timeout(Duration::from_secs(3)).expect("immediate compilation announcement");
    assert_eq!(first, "Compiling all 1 spec(s) with Codex");
    let progress = receive.recv_timeout(Duration::from_secs(8)).expect("elapsed progress before compiler completion");
    assert!(progress.contains("Still compiling with Codex (") && progress.contains("s elapsed)"), "{progress}");
    assert!(process.0.try_wait().unwrap().is_none());
    fs::write(p.0.join("release"), "continue").unwrap();
    assert!(process.0.wait().unwrap().success());
    let stderr = reader.join().unwrap();
    let stdout = stdout_reader.join().unwrap();
    assert!(stderr.contains("mock compiler diagnostic"));
    assert!(stderr.contains("structured compiler warning"));
    assert!(stderr.contains("Compiler finished in") && stderr.contains("validating output"));
    for output in [&stderr, &stdout] {
        assert!(!output.contains("NORMAL_TRANSCRIPT_MUST_STAY_HIDDEN"));
        assert!(!output.contains("PRIVATE_SPEC_TEXT_MUST_STAY_HIDDEN"));
        assert!(!output.contains("Compilation input (data)"));
    }
    let args = fs::read_to_string(p.0.join("compiler-argv.txt")).unwrap();
    assert!(args.contains("--sandbox\nread-only\n"));
    assert!(args.contains("approval_policy=\"never\""));
    assert!(args.contains("--json\n"));
    let args: Vec<_> = args.lines().collect();
    for flag in ["-o", "--output-schema"] {
        let index = args.iter().position(|s| *s == flag).unwrap();
        assert!(std::path::Path::new(args[index + 1]).is_absolute());
    }
    let cwd = fs::read_to_string(p.0.join("compiler-cwd.txt")).unwrap();
    assert_eq!(fs::canonicalize(cwd.trim()).unwrap(), fs::canonicalize(&p.0).unwrap());
    let schema: Value = serde_json::from_slice(&fs::read(p.0.join("compiler-schema.json")).unwrap()).unwrap();
    assert_eq!(schema["properties"]["requirements"]["items"]["properties"]["source"]["enum"], json!(["app.nat"]));
    let prompt = fs::read_to_string(p.0.join("compiler-prompt.txt")).unwrap();
    assert_reference_protocol(&prompt);
    let failure = Command::new(env!("CARGO_BIN_EXE_nat")).args(["build", "--full"]).current_dir(&p.0)
        .env("PATH", &mock_path).env("NAT_MOCK_FAIL", "yes").output().unwrap();
    assert!(!failure.status.success());
    let failure_text = String::from_utf8_lossy(&failure.stderr);
    assert!(failure_text.contains("Codex compilation failed") && failure_text.contains("mock compiler diagnostic"));
    assert!(!failure_text.contains("Compiler finished in"));
    p.ok(&["check"]);
}
#[test]
fn older_full_responses_from_compiler_and_proposal_are_accepted() {
    let p = Project::new();
    p.proposal();
    let path = p.0.join("proposal.json");
    let mut response: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    response["files"][0].as_object_mut().unwrap().remove("sources");
    fs::write(&path, serde_json::to_vec(&response).unwrap()).unwrap();
    let mock_path = p.mock_compiler();
    let compiled = Command::new(env!("CARGO_BIN_EXE_nat")).arg("build").current_dir(&p.0)
        .env("PATH", &mock_path).env("NAT_MOCK_FAIL", "no").env("NAT_MOCK_WAIT", "no").output().unwrap();
    assert!(compiled.status.success(), "{}", String::from_utf8_lossy(&compiled.stderr));
    p.ok(&["check"]);
    p.ok(&["build", "--proposal", "proposal.json", "--full"]);
    p.ok(&["check"]);
    let manifest: Value = serde_json::from_slice(&fs::read(p.0.join("generated/.nat-manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["file_sources"]["description.txt"], json!(["app.nat"]));
    let schema: Value = serde_json::from_slice(&fs::read(p.0.join("compiler-schema.json")).unwrap()).unwrap();
    assert!(schema["properties"]["files"]["items"]["required"].as_array().unwrap().contains(&json!("sources")));
    fs::remove_file(p.0.join("compiler-prompt.txt")).unwrap();
    let unchanged = Command::new(env!("CARGO_BIN_EXE_nat")).arg("build").current_dir(&p.0)
        .env("PATH", &mock_path).env("NAT_MOCK_FAIL", "yes").output().unwrap();
    assert!(unchanged.status.success());
    assert!(!p.0.join("compiler-prompt.txt").exists());
}
#[test]
fn scoped_compiler_announces_counts_and_preserves_unaffected_output() {
    let p = Project::new();
    p.proposal();
    fs::write(p.0.join("spec/other.nat"), "Other independent feature").unwrap();
    let path = p.0.join("proposal.json");
    let mut response: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    response["files"].as_array_mut().unwrap().push(json!({"path":"other.txt","content":"unchanged","sources":["other.nat"]}));
    response["requirements"].as_array_mut().unwrap().push(json!({"id":"OTHER","source":"other.nat","statement":"Other independent feature"}));
    fs::write(&path, serde_json::to_vec(&response).unwrap()).unwrap();
    p.ok(&["build", "--proposal", "proposal.json"]);
    let other = p.0.join("generated/other.txt");
    let before = fs::metadata(&other).unwrap().modified().unwrap();
    fs::write(p.0.join("spec/app.nat"), "Changed greeting").unwrap();
    response["files"].as_array_mut().unwrap().pop();
    response["requirements"].as_array_mut().unwrap().pop();
    response["files"][0]["content"] = json!("changed greeting");
    response["full_rebuild_required"] = json!(false);
    response["reason"] = json!("");
    response["remove_files"] = json!([]);
    fs::write(&path, serde_json::to_vec(&response).unwrap()).unwrap();
    let mock_path = p.mock_compiler();
    let result = Command::new(env!("CARGO_BIN_EXE_nat")).arg("build").current_dir(&p.0)
        .env("PATH", mock_path).env("NAT_MOCK_FAIL", "no").env("NAT_MOCK_WAIT", "no").output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert_eq!(String::from_utf8_lossy(&result.stderr).lines().next().unwrap(), "Compiling 1 changed spec(s) with Codex; 1 generated file(s) affected");
    assert_eq!(fs::read_to_string(&other).unwrap(), "unchanged");
    assert_eq!(fs::metadata(other).unwrap().modified().unwrap(), before);
    let prompt = fs::read_to_string(p.0.join("compiler-prompt.txt")).unwrap();
    assert_reference_protocol(&prompt);
    let input: Value = serde_json::from_str(prompt.split("Compilation input (data):\n").nth(1).unwrap()).unwrap();
    assert_eq!(input["scope"], "scoped");
    assert_eq!(input["human_sources"].as_object().unwrap().len(), 1);
    assert_eq!(input["reserved_ids"], json!(["OTHER"]));
    assert!(!input["human_sources"].as_object().unwrap().contains_key("other.nat"));
    assert_eq!(input["affected_outputs"].as_array().unwrap().len(), 1);
    p.ok(&["check"]);
}
#[test]
fn actual_missing_executable_repairs_with_local_compiler_and_forwards_arguments() {
    let p = Project::new();
    p.proposal();
    let path = p.0.join("proposal.json");
    let good: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut bad = good.clone();
    bad["run"] = json!(["/nat-test/unavailable-executable"]);
    fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
    p.ok(&["build", "--proposal", "proposal.json"]);
    let mut repaired = good;
    repaired["checks"] = json!([["/usr/bin/touch", "repair-checked"]]);
    fs::write(path, serde_json::to_vec(&repaired).unwrap()).unwrap();
    let mock_path = p.mock_compiler();
    let result = Command::new(env!("CARGO_BIN_EXE_nat")).args(["run", "--", "Ada Lovelace", "$(false)"])
        .current_dir(&p.0).env("PATH", mock_path).env("NAT_MOCK_FAIL", "no").env("NAT_MOCK_WAIT", "no").output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert!(String::from_utf8_lossy(&result.stderr).contains("recompiling for this machine"));
    assert!(String::from_utf8_lossy(&result.stdout).contains("Hello Ada Lovelace $(false)"));
    assert!(p.0.join("generated/repair-checked").exists());
    let prompt = fs::read_to_string(p.0.join("compiler-prompt.txt")).unwrap();
    let input: Value = serde_json::from_str(prompt.split("Compilation input (data):\n").nth(1).unwrap()).unwrap();
    assert_eq!(input["scope"], "full");
    assert!(input["runtime_failure_feedback"].as_str().unwrap().contains("unavailable-executable"));
    p.ok(&["check"]);
}
