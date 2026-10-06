#![cfg(unix)]

use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn compiler_sources_are_exact_paths_and_citations_remain_in_prose() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("nat-source-test-{}-{stamp}", std::process::id()));
    fs::create_dir_all(root.join("spec/concepts")).unwrap();
    fs::create_dir(root.join("bin")).unwrap();
    fs::write(root.join("spec/00-brain.nat"), "Preserve provenance.\n").unwrap();
    fs::write(
        root.join("spec/concepts/03-warrant.nat"),
        "[WA-01] Retain evidence.\n[WA-03] State inference assumptions.\n",
    )
    .unwrap();
    let source = "concepts/03-warrant.nat";
    let mut proposal = json!({
        "summary": "Evidence recorder",
        "requirements": [{"id": "WA-01", "source": source, "statement": "Retain evidence."}],
        "assumptions": [{
            "id": "A-01", "source": source,
            "statement": "Record uncertainty explicitly.",
            "reason": "Implements WA-01 and WA-03; uncertainty encoding was unspecified."
        }],
        "files": [{"path": "main.py", "content": "print('evidence')\n", "sources": [source, "00-brain.nat"]}],
        "run": ["python3", "main.py"],
        "checks": []
    });
    let proposal_path = root.join("proposal.json");
    fs::write(&proposal_path, serde_json::to_vec(&proposal).unwrap()).unwrap();
    let fake_codex = root.join("bin/codex");
    fs::write(
        &fake_codex,
        r#"#!/bin/sh
out=
schema=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) shift; out="$1" ;;
    --output-schema) shift; schema="$1" ;;
  esac
  shift
done
cat >"$NAT_PROMPT_CAPTURE"
cp "$schema" "$NAT_SCHEMA_CAPTURE"
cp "$NAT_TEST_PROPOSAL" "$out"
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_codex, fs::Permissions::from_mode(0o755)).unwrap();
    let binary = env!("CARGO_BIN_EXE_nat");
    let built = Command::new(binary)
        .arg("build")
        .current_dir(&root)
        .env(
            "PATH",
            format!(
                "{}:{}",
                root.join("bin").display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("NAT_TEST_PROPOSAL", &proposal_path)
        .env("NAT_SCHEMA_CAPTURE", root.join("schema.json"))
        .env("NAT_PROMPT_CAPTURE", root.join("prompt.txt"))
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let schema: Value =
        serde_json::from_slice(&fs::read(root.join("schema.json")).unwrap()).unwrap();
    let allowed = json!(["00-brain.nat", source]);
    for collection in ["requirements", "assumptions"] {
        assert_eq!(
            schema["properties"][collection]["items"]["properties"]["source"]["enum"],
            allowed
        );
    }
    assert_eq!(
        schema["properties"]["files"]["items"]["properties"]["sources"]["items"]["enum"],
        allowed
    );
    let inferences = fs::read_to_string(root.join("inferred/assumptions.inat")).unwrap();
    assert!(inferences.contains("Source: spec/concepts/03-warrant.nat\n"));
    assert!(inferences.contains("Implements WA-01 and WA-03"));

    // Imported proposals bypass structured output, so validation must still
    // reject the filename-plus-IDs form that caused the original build failure.
    proposal["assumptions"][0]["source"] = json!(format!("{source}:WA-01,WA-03,WA-05"));
    fs::write(&proposal_path, serde_json::to_vec(&proposal).unwrap()).unwrap();
    let rejected = Command::new(binary)
        .args(["build", "--proposal"])
        .arg(&proposal_path)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("invalid assumption A-01 or source"));
    assert_eq!(
        fs::read_to_string(root.join("inferred/assumptions.inat")).unwrap(),
        inferences
    );
    let checked = Command::new(binary)
        .arg("check")
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(checked.status.success());
    fs::remove_dir_all(root).unwrap();
}
