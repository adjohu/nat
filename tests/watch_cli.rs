use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct WatchProcess(Child);

impl Drop for WatchProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn manifest_hash(root: &Path) -> Option<String> {
    let bytes = fs::read(root.join("generated/.nat-manifest.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value["input_hash"].as_str().map(str::to_owned)
}

fn wait_for_hash(root: &Path, different_from: Option<&str>) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(hash) = manifest_hash(root) {
            if different_from != Some(hash.as_str()) {
                return Some(hash);
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
fn watch_rebuilds_after_nat_edit() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("nat-watch-test-{}-{stamp}", std::process::id()));
    fs::create_dir_all(root.join("spec")).unwrap();
    fs::write(root.join("spec/hello.nat"), "Greet the world.\n").unwrap();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/hello/compilation.json");
    let child = Command::new(env!("CARGO_BIN_EXE_nat"))
        .args(["watch", "--project"])
        .arg(&root)
        .arg("--proposal")
        .arg(&fixture)
        .arg("--no-test")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let watcher = WatchProcess(child);
    let initial = wait_for_hash(&root, None).expect("watch should build at startup");
    fs::write(
        root.join("spec/hello.nat"),
        "# Greeting\n\nGreet the world and a friend.\n\n![Design](./design.png)\n",
    )
    .unwrap();
    fs::write(root.join("spec/design.png"), [0, 255, 1]).unwrap();
    let updated =
        wait_for_hash(&root, Some(&initial)).expect("watch should rebuild after a .nat edit");
    fs::write(root.join("spec/design.png"), [0, 255, 2]).unwrap();
    let image_updated = wait_for_hash(&root, Some(&updated));
    drop(watcher);
    fs::remove_dir_all(&root).unwrap();
    assert!(
        image_updated.is_some(),
        "watch should rebuild after an image edit"
    );
}
