#![cfg(unix)]
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{atomic::{AtomicU64, Ordering}, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("nat-cli-{}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), SERIAL.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&path).unwrap(); let p = Self(path); p.ok(&["init"]); p
    }
    fn call(&self, args: &[&str]) -> Output { Command::new(env!("CARGO_BIN_EXE_nat")).args(args).current_dir(&self.0).output().unwrap() }
    fn ok(&self, args: &[&str]) -> Output { let out = self.call(args); success(&out); out }
    fn save(&self, response: &Value) { fs::write(self.0.join("proposal.json"), serde_json::to_vec(response).unwrap()).unwrap(); }
    fn mock_path(&self) -> String {
        fs::create_dir_all(self.0.join("bin")).unwrap(); let path = self.0.join("bin/codex");
        fs::write(&path, r#"#!/bin/sh
printf 'call\n' >> compiler-calls.txt
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
case "$out" in /*) ;; *) exit 21 ;; esac
case "$schema" in /*) ;; *) exit 22 ;; esac
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
case "$NAT_MOCK_MUTATE" in
  source) printf '\nconcurrent source edit\n' >> spec/app.nat ;;
  reference) printf 'changed context' > context.md ;;
  removed) rm generated/keep.txt ;;
  edited) printf 'concurrent edit' > generated/keep.txt ;;
  symlink) rm generated/keep.txt; ln -s ../outside.txt generated/keep.txt ;;
  inferred) printf 'concurrent inference edit' > inferred/assumptions.inat ;;
  manifest) printf ' ' >> generated/.nat-manifest.json ;;
esac
cp proposal.json "$out"
"#).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        format!("{}:{}", self.0.join("bin").display(), std::env::var("PATH").unwrap_or_default())
    }
    fn compiler_command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nat"));
        command.args(args).current_dir(&self.0).env("PATH", self.mock_path())
            .env("NAT_MOCK_FAIL", "no").env("NAT_MOCK_WAIT", "no").env("NAT_MOCK_MUTATE", "none"); command
    }
    fn compiled(&self, response: &Value) -> Output {
        self.save(response); self.compiler_command(&["build","--full"]).output().unwrap()
    }
    fn install(&self) { self.save(&full()); self.ok(&["build","--proposal","proposal.json"]); }
    fn input(&self) -> Value {
        let prompt = fs::read_to_string(self.0.join("compiler-prompt.txt")).unwrap();
        for text in [".nat files are Markdown","view referenced images using read-only tools","ORIGINAL Markdown","must not write files or run generated software"] { assert!(prompt.contains(text)); }
        serde_json::from_str(prompt.split("Compilation input (data):\n").nth(1).unwrap()).unwrap()
    }
    fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, path: &Path, result: &mut BTreeMap<String, Vec<u8>>) {
            for entry in fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path(); let m = fs::symlink_metadata(&path).unwrap();
                if m.is_dir() { walk(root, &path, result); }
                else {
                    let bytes = if m.file_type().is_symlink() { format!("symlink:{}", fs::read_link(&path).unwrap().display()).into_bytes() } else { fs::read(&path).unwrap() };
                    result.insert(path.strip_prefix(root).unwrap().to_str().unwrap().to_owned(), bytes);
                }
            }
        }
        let mut result = BTreeMap::new();
        for directory in ["generated","inferred"] { walk(&self.0, &self.0.join(directory), &mut result); } result
    }
}
impl Drop for Project { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
struct Process(Child);
impl Drop for Process { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
fn success(out: &Output) { assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr)); }
fn full() -> Value {
    json!({"summary":"offline fixture","requirements":[{"id":"GREETING","source":"app.nat","statement":"Greet the supplied name"}],
        "assumptions":[],"files":[{"path":"change.txt","content":"initial","sources":["app.nat"]},{"path":"keep.txt","content":"unchanged\r\nUnicode: λ\n","sources":["app.nat"]}],
        "run":["/bin/echo","Hello"],"checks":[["true"]]})
}
fn reused() -> Value {
    let mut v = full(); v["status"] = json!("complete"); v["diagnostic"] = json!("");
    for f in v["files"].as_array_mut().unwrap() { f["content"] = Value::Null; } v
}
fn identity(path: &Path) -> (u64, u32, SystemTime) {
    let m = fs::metadata(path).unwrap(); (m.ino(), m.permissions().mode(), m.modified().unwrap())
}
fn retained(p: &Project, out: &Output) -> Value {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let line = stderr.lines().find_map(|s| s.strip_prefix("Rejected response retained at ")).expect("retained response path");
    let path = PathBuf::from(line);
    assert!(path.starts_with(&p.0)); assert!(!path.starts_with(p.0.join("generated"))); assert!(!path.starts_with(p.0.join("inferred")));
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn wait_current(p: &Project) {
    let start = Instant::now();
    while !p.call(&["check"]).status.success() { assert!(start.elapsed() < Duration::from_secs(10)); thread::sleep(Duration::from_millis(80)); }
}

#[test]
fn partial_then_all_reuse_through_real_cli_and_schema() {
    let p = Project::new(); p.install();
    let keep = p.0.join("generated/keep.txt"); fs::set_permissions(&keep, fs::Permissions::from_mode(0o640)).unwrap();
    let bytes = fs::read(&keep).unwrap(); let metadata = identity(&keep);
    fs::write(p.0.join("spec/app.nat"), "Revised greeting semantics").unwrap();
    let mut response = reused(); response["files"][0]["content"] = json!("changed");
    response["requirements"][0]["statement"] = json!("Revised greeting semantics");
    response["checks"] = json!([["/bin/sh","-c","test \"$(cat change.txt)\" = changed && test -s keep.txt && printf checked > ../checks-ran"]]);
    let out = p.compiled(&response); success(&out);
    assert_eq!(fs::read(&keep).unwrap(), bytes); assert_eq!(identity(&keep), metadata);
    assert_eq!(fs::read_to_string(p.0.join("generated/change.txt")).unwrap(), "changed");
    assert!(!p.0.join("checks-ran").exists()); p.ok(&["check"]); p.ok(&["test"]);
    assert_eq!(fs::read_to_string(p.0.join("checks-ran")).unwrap(), "checked");
    let input = p.input(); assert_eq!(input["scope"], "full");
    let catalog = input["reuse_catalog"].as_array().unwrap(); assert_eq!(catalog.len(), 2);
    for item in catalog { assert!(item.get("content").is_none()); assert_eq!(item["hash"].as_str().unwrap().len(), 64); assert_eq!(item["sources"], json!(["app.nat"])); }
    let schema: Value = serde_json::from_slice(&fs::read(p.0.join("compiler-schema.json")).unwrap()).unwrap();
    assert_eq!(schema["properties"]["files"]["items"]["properties"]["content"]["type"], json!(["string","null"]));
    assert_eq!(schema["properties"]["status"]["enum"], json!(["complete","incomplete"]));
    for field in ["status","diagnostic"] { assert!(schema["required"].as_array().unwrap().contains(&json!(field))); }
    let change_metadata = identity(&p.0.join("generated/change.txt"));
    let old_manifest = fs::read(p.0.join("generated/.nat-manifest.json")).unwrap();
    fs::write(p.0.join("spec/app.nat"), "Clarified semantics; same implementation").unwrap();
    response["requirements"][0]["statement"] = json!("Clarified semantics; same implementation");
    response["files"][0]["content"] = Value::Null; response["summary"] = json!("all reused");
    success(&p.compiled(&response)); p.ok(&["check"]); p.ok(&["test"]);
    assert_ne!(fs::read(p.0.join("generated/.nat-manifest.json")).unwrap(), old_manifest);
    assert_eq!(identity(&keep), metadata); assert_eq!(identity(&p.0.join("generated/change.txt")), change_metadata);
    assert_eq!(fs::read(&keep).unwrap(), bytes);
}
#[test]
fn imported_null_proposal_basis_and_full_omission() {
    let p = Project::new(); p.install(); let keep = p.0.join("generated/keep.txt"); let before = identity(&keep);
    let mut response = reused(); response["files"].as_array_mut().unwrap().remove(0); p.save(&response);
    let out = p.ok(&["build","--proposal","proposal.json","--full"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("full proposal reuse basis: current tracked manifest SHA-256"));
    assert!(stderr.contains("2 outputs verified at invocation"));
    assert!(!p.0.join("generated/change.txt").exists()); assert_eq!(identity(&keep), before); p.ok(&["check"]);
}
#[test]
fn invalid_reuse_responses_never_publish() {
    for case in ["fresh","missing","untracked","wrong-case","duplicate","reserved","escape","no-sources","bad-source","bad-id","empty-argv","missing-content","bad-status","missing-diagnostic","bad-diagnostic"] {
        let p = Project::new(); if case != "fresh" { p.install(); }
        let mut response = reused();
        match case {
            "fresh" => {},
            "missing" => response["files"][1]["path"] = json!("missing.txt"),
            "untracked" => { fs::write(p.0.join("generated/untracked.txt"), "keep").unwrap(); response["files"][1]["path"] = json!("untracked.txt"); },
            "wrong-case" => response["files"][1]["path"] = json!("KEEP.txt"),
            "duplicate" => { let duplicate = response["files"][1].clone(); response["files"].as_array_mut().unwrap().push(duplicate); },
            "reserved" => response["files"][1]["path"] = json!("ir.json"),
            "escape" => response["files"][1]["path"] = json!("../outside.txt"),
            "no-sources" => { response["files"][1].as_object_mut().unwrap().remove("sources"); },
            "bad-source" => response["files"][1]["sources"] = json!(["spec/app.nat"]),
            "bad-id" => response["requirements"][0]["id"] = json!("bad ID"),
            "empty-argv" => response["run"] = json!(["/bin/echo",""]),
            "missing-content" => { response["files"][1].as_object_mut().unwrap().remove("content"); },
            "bad-status" => response["status"] = json!("maybe"),
            "missing-diagnostic" => { response.as_object_mut().unwrap().remove("diagnostic"); },
            "bad-diagnostic" => response["diagnostic"] = Value::Null,
            _ => unreachable!(),
        }
        let before = p.snapshot(); let out = p.compiled(&response);
        assert!(!out.status.success(), "{case}"); assert_eq!(p.snapshot(), before, "{case}");
        assert_eq!(retained(&p, &out), response); assert!(!String::from_utf8_lossy(&out.stdout).contains("Built:"));
        assert_eq!(fs::read_to_string(p.0.join("compiler-calls.txt")).unwrap().lines().count(), 1);
    }
}
#[test]
fn drift_before_invocation_refuses_reuse_without_touching_build() {
    for case in ["edited","removed","symlink","inferred","ir"] {
        let p = Project::new(); p.install(); let target = p.0.join("generated/keep.txt");
        match case {
            "edited" => fs::write(&target, "human edit").unwrap(),
            "removed" => fs::remove_file(&target).unwrap(),
            "symlink" => { fs::write(p.0.join("outside.txt"), "unchanged\r\nUnicode: λ\n").unwrap(); fs::remove_file(&target).unwrap(); symlink("../outside.txt", &target).unwrap(); },
            "inferred" => fs::write(p.0.join("inferred/assumptions.inat"), "human edit").unwrap(),
            "ir" => fs::remove_file(p.0.join("generated/ir.json")).unwrap(),
            _ => unreachable!(),
        }
        let before = p.snapshot(); let out = p.compiled(&reused());
        assert!(!out.status.success()); assert_eq!(p.snapshot(), before); assert!(!p.0.join("compiler-calls.txt").exists());
    }
}
#[test]
fn concurrent_source_reference_and_artifact_changes_refuse_publication() {
    for mutation in ["source","reference","removed","edited","symlink","inferred","manifest"] {
        let p = Project::new(); fs::write(p.0.join("context.md"), "context").unwrap();
        fs::write(p.0.join("spec/app.nat"), "Greeting [context](../context.md)").unwrap(); p.install();
        fs::write(p.0.join("outside.txt"), "unchanged\r\nUnicode: λ\n").unwrap();
        let mut expected = p.snapshot();
        match mutation {
            "removed" => { expected.remove("generated/keep.txt"); },
            "edited" => { expected.insert("generated/keep.txt".into(), b"concurrent edit".to_vec()); },
            "symlink" => { expected.insert("generated/keep.txt".into(), b"symlink:../outside.txt".to_vec()); },
            "inferred" => { expected.insert("inferred/assumptions.inat".into(), b"concurrent inference edit".to_vec()); },
            "manifest" => { expected.get_mut("generated/.nat-manifest.json").unwrap().push(b' '); },
            _ => {},
        }
        let response = reused(); p.save(&response);
        let out = p.compiler_command(&["build","--full"]).env("NAT_MOCK_MUTATE", mutation).output().unwrap();
        assert!(!out.status.success(), "{mutation}"); assert_eq!(p.snapshot(), expected, "{mutation}");
        assert_eq!(retained(&p, &out), response); assert!(!String::from_utf8_lossy(&out.stdout).contains("Built:"));
    }
}
#[test]
fn incomplete_with_placeholder_or_empty_inventory_preserves_build_and_does_not_retry() {
    for placeholder in [false, true] {
        let p = Project::new(); p.install(); let before = p.snapshot(); let mut response = full();
        response["status"] = json!("incomplete"); response["diagnostic"] = json!("Unable to complete the requested implementation.");
        response["files"] = if placeholder { json!([{"path":"diagnostic.txt","content":"not a program","sources":["app.nat"]}]) } else { json!([]) };
        response["run"] = json!(["/bin/echo","plausible"]); response["checks"] = json!([["true"]]);
        let out = p.compiled(&response); assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("incomplete compilation: Unable to complete"));
        assert!(!String::from_utf8_lossy(&out.stdout).contains("Built:"));
        assert_eq!(retained(&p, &out), response); assert_eq!(p.snapshot(), before);
        assert_eq!(fs::read_to_string(p.0.join("compiler-calls.txt")).unwrap().lines().count(), 1); p.ok(&["check"]);
    }
}
#[test]
fn legacy_explicit_proposals_and_scoped_protocol_remain_supported() {
    let p = Project::new(); let mut response = full();
    response["files"][0].as_object_mut().unwrap().remove("sources");
    success(&p.compiled(&response)); p.ok(&["build","--proposal","proposal.json","--full"]); p.ok(&["check"]);
    let manifest: Value = serde_json::from_slice(&fs::read(p.0.join("generated/.nat-manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["file_sources"]["change.txt"], json!(["app.nat"]));
    response["assumptions"] = json!([{"id":"A-01","source":"spec/app.nat","statement":"English","reason":"No language specified"}]); p.save(&response);
    let before = p.snapshot(); let out = p.call(&["build","--proposal","proposal.json"]);
    assert!(!out.status.success()); assert!(String::from_utf8_lossy(&out.stderr).contains("invalid assumption A-01 or source")); assert_eq!(p.snapshot(), before);
    response["assumptions"] = json!([]); response["files"][0]["content"] = json!("explicit update"); p.save(&response);
    p.ok(&["build","--proposal","proposal.json"]); assert_eq!(fs::read_to_string(p.0.join("generated/change.txt")).unwrap(), "explicit update");
    fs::write(p.0.join("proposal.json"), "invalid JSON").unwrap(); assert!(!p.call(&["build","--proposal","proposal.json"]).status.success()); p.ok(&["check"]);
    fs::remove_file(p.0.join("compiler-prompt.txt")).unwrap();
    success(&p.compiler_command(&["build"]).env("NAT_MOCK_FAIL", "yes").output().unwrap()); assert!(!p.0.join("compiler-prompt.txt").exists());

    fs::write(p.0.join("spec/other.nat"), "Independent feature").unwrap();
    let mut response = full(); response["files"][1]["sources"] = json!(["other.nat"]);
    response["requirements"].as_array_mut().unwrap().push(json!({"id":"OTHER","source":"other.nat","statement":"Independent feature"}));
    p.save(&response); p.ok(&["build","--proposal","proposal.json","--full"]);
    let keep = identity(&p.0.join("generated/keep.txt"));
    fs::write(p.0.join("spec/app.nat"), "Changed greeting").unwrap();
    response["files"].as_array_mut().unwrap().pop(); response["requirements"].as_array_mut().unwrap().pop();
    response["files"][0]["content"] = json!("scoped change");
    response["full_rebuild_required"] = json!(false); response["reason"] = json!(""); response["remove_files"] = json!([]); p.save(&response);
    let out = p.compiler_command(&["build"]).output().unwrap(); success(&out);
    assert_eq!(String::from_utf8_lossy(&out.stderr).lines().next().unwrap(), "Compiling 1 changed spec(s) with Codex; 1 generated file(s) affected");
    let input = p.input(); assert_eq!(input["scope"], "scoped"); assert!(input.get("reuse_catalog").is_none());
    assert_eq!(input["human_sources"].as_object().unwrap().len(), 1); assert_eq!(input["reserved_ids"], json!(["OTHER"]));
    assert_eq!(input["affected_outputs"].as_array().unwrap().len(), 1);
    let schema: Value = serde_json::from_slice(&fs::read(p.0.join("compiler-schema.json")).unwrap()).unwrap();
    assert_eq!(schema["properties"]["files"]["items"]["properties"]["content"]["type"], "string");
    assert!(schema["properties"].get("status").is_none());
    assert_eq!(identity(&p.0.join("generated/keep.txt")), keep); p.ok(&["check"]);
}
#[test]
fn protocol_progress_diagnostics_and_success_transcript_suppression() {
    let p = Project::new(); p.save(&full()); fs::write(p.0.join("spec/app.nat"), "PRIVATE_SPEC_TEXT_MUST_STAY_HIDDEN").unwrap();
    let child = p.compiler_command(&["build","--full"]).env("NAT_MOCK_WAIT", "yes").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut process = Process(child); let stderr = process.0.stderr.take().unwrap(); let stdout = process.0.stdout.take().unwrap();
    let stdout_reader = thread::spawn(move || { let mut s = String::new(); BufReader::new(stdout).read_to_string(&mut s).unwrap(); s });
    let (send, receive) = mpsc::channel();
    let stderr_reader = thread::spawn(move || { let mut s = String::new(); for line in BufReader::new(stderr).lines() { let line = line.unwrap(); s.push_str(&line); s.push('\n'); let _ = send.send(line); } s });
    assert_eq!(receive.recv_timeout(Duration::from_secs(3)).unwrap(), "Compiling all 1 spec(s) with Codex");
    let progress = receive.recv_timeout(Duration::from_secs(8)).unwrap(); assert!(progress.contains("Still compiling with Codex (") && progress.contains("s elapsed)"));
    assert!(process.0.try_wait().unwrap().is_none()); fs::write(p.0.join("release"), "go").unwrap(); assert!(process.0.wait().unwrap().success());
    let stderr = stderr_reader.join().unwrap(); let stdout = stdout_reader.join().unwrap();
    assert!(stderr.contains("mock compiler diagnostic") && stderr.contains("structured compiler warning"));
    assert!(stderr.contains("Compiler finished in") && stderr.contains("validating output"));
    for s in [&stderr, &stdout] { for hidden in ["NORMAL_TRANSCRIPT_MUST_STAY_HIDDEN","PRIVATE_SPEC_TEXT_MUST_STAY_HIDDEN","Compilation input (data)"] { assert!(!s.contains(hidden)); } }
    let argv = fs::read_to_string(p.0.join("compiler-argv.txt")).unwrap(); assert!(argv.contains("--sandbox\nread-only\n") && argv.contains("approval_policy=\"never\""));
    let args: Vec<_> = argv.lines().collect(); for flag in ["-o","--output-schema"] { let index = args.iter().position(|s| *s == flag).unwrap(); assert!(Path::new(args[index + 1]).is_absolute()); }
    let cwd = fs::read_to_string(p.0.join("compiler-cwd.txt")).unwrap(); assert_eq!(fs::canonicalize(cwd.trim()).unwrap(), fs::canonicalize(&p.0).unwrap()); p.input();
    let before = p.snapshot(); let out = p.compiler_command(&["build","--full"]).env("NAT_MOCK_FAIL", "yes").output().unwrap();
    assert!(!out.status.success()); assert!(String::from_utf8_lossy(&out.stderr).contains("Codex compilation failed")); assert_eq!(p.snapshot(), before);
    fs::write(p.0.join("proposal.json"), b"invalid compiler JSON").unwrap(); let out = p.compiler_command(&["build","--full"]).output().unwrap();
    assert!(!out.status.success()); let text = String::from_utf8_lossy(&out.stderr);
    let path = text.lines().find_map(|s| s.strip_prefix("Rejected response retained at ")).unwrap();
    assert_eq!(fs::read(path).unwrap(), b"invalid compiler JSON"); assert_eq!(p.snapshot(), before);
}
#[test]
fn cli_discovery_forwarding_status_help_and_runtime_repair() {
    let p = Project::new(); assert!(String::from_utf8_lossy(&p.ok(&["status"]).stdout).contains("No build")); p.install(); p.ok(&["check"]); p.ok(&["test"]);
    let out = p.ok(&["run","--","a b","$(echo unsafe)","--project","literal"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Hello a b $(echo unsafe) --project literal")); assert!(!p.0.join("unsafe").exists());
    fs::create_dir_all(p.0.join("nested/deeper")).unwrap();
    for args in [vec!["check"],vec!["check","--project","../.."]] { success(&Command::new(env!("CARGO_BIN_EXE_nat")).args(args).current_dir(p.0.join("nested/deeper")).output().unwrap()); }
    for alias in ["help","--help","-h"] {
        let out = p.ok(&[alias]); let text = String::from_utf8_lossy(&out.stdout);
        for word in ["init","build","watch","run","test","status","check","promote","--project","--full","--no-test","--proposal","privileges"] { assert!(text.contains(word)); }
    }
    for alias in ["version","--version","-V"] { assert!(String::from_utf8_lossy(&p.ok(&[alias]).stdout).contains(env!("CARGO_PKG_VERSION"))); }
    for args in [vec!["unknown"],vec!["build","--unknown"],vec!["build","--proposal"],vec!["init","one","two"],vec!["check","--project","missing"]] { assert!(!p.call(&args).status.success()); }
    let mut bad = full(); bad["run"] = json!(["/nat-test/unavailable-executable"]); p.save(&bad); p.ok(&["build","--proposal","proposal.json"]);
    let mut repaired = reused(); repaired["checks"] = json!([["/usr/bin/touch","../repair-checked"]]); p.save(&repaired);
    let out = p.compiler_command(&["run","--","Ada Lovelace","$(false)"]).output().unwrap(); success(&out);
    assert!(String::from_utf8_lossy(&out.stderr).contains("recompiling for this machine"));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Hello Ada Lovelace $(false)")); assert!(p.0.join("repair-checked").exists());
    let input = p.input(); assert_eq!(input["scope"], "full"); assert!(input["runtime_failure_feedback"].as_str().unwrap().contains("unavailable-executable"));
    p.ok(&["check"]); fs::write(p.0.join("generated/keep.txt"), "edit").unwrap();
    assert!(!p.call(&["check"]).status.success()); assert!(String::from_utf8_lossy(&p.ok(&["status"]).stdout).contains("Artifact drift"));
}
#[test]
fn watch_startup_changes_errors_and_check_policy() {
    for no_test in [false, true] {
        let p = Project::new(); let mut response = full(); response["checks"] = json!([["/usr/bin/touch","../watch-checked"]]); p.save(&response);
        let mut command = Command::new(env!("CARGO_BIN_EXE_nat")); command.args(["watch","--proposal","proposal.json"]);
        if no_test { command.arg("--no-test"); }
        let mut process = Process(command.current_dir(&p.0).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap()); wait_current(&p);
        if no_test { thread::sleep(Duration::from_millis(600)); assert!(!p.0.join("watch-checked").exists()); }
        else { let start = Instant::now(); while !p.0.join("watch-checked").exists() { assert!(start.elapsed() < Duration::from_secs(5)); thread::sleep(Duration::from_millis(50)); } }
        fs::write(p.0.join("spec/app.nat"), "Changed greeting").unwrap(); wait_current(&p);
        fs::write(p.0.join("spec/app.nat"), "[context](missing.md)").unwrap(); thread::sleep(Duration::from_millis(900));
        assert!(process.0.try_wait().unwrap().is_none()); assert!(!p.call(&["check"]).status.success());
        fs::write(p.0.join("spec/missing.md"), "Supporting context").unwrap(); wait_current(&p); assert!(process.0.try_wait().unwrap().is_none());
    }
}
