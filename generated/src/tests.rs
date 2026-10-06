use super::*;

fn fixture() -> Scratch {
    let t = Scratch::new().unwrap();
    init(&t.0).unwrap();
    fs::remove_file(t.0.join("spec/app.nat")).unwrap();
    fs::write(t.0.join("spec/a.nat"), "First feature").unwrap();
    fs::write(t.0.join("spec/b.nat"), "Second feature").unwrap();
    t
}
fn full() -> Compilation {
    serde_json::from_value(json!({
        "summary":"fixture", "requirements":[
            {"id":"REQ-A", "source":"a.nat", "statement":"First feature"},
            {"id":"REQ-B", "source":"b.nat", "statement":"Second feature"}],
        "assumptions":[{"id":"ASSUME-A", "source":"a.nat", "statement":"Use English greetings.", "reason":"No greeting language specified."}],
        "files":[{"path":"a.txt", "content":"first", "sources":["a.nat"]},
            {"path":"b.txt", "content":"second", "sources":["b.nat"]}],
        "run":["/bin/echo","greeting"], "checks":[["true"]]
    })).unwrap()
}
fn install(root: &Path) -> Manifest {
    build_using(root, false, None, &mut |r| {
        assert_eq!(r["scope"], "full");
        Ok(serde_json::to_value(full()).unwrap())
    }).unwrap()
}
fn patch() -> Value {
    json!({"full_rebuild_required":false,"reason":"","summary":"scoped fixture",
        "requirements":[{"id":"REQ-A","source":"a.nat","statement":"Changed first feature"}],
        "assumptions":[],"files":[{"path":"a.txt","content":"updated","sources":["a.nat"]}],
        "remove_files":[],"run":["/bin/echo","updated"],"checks":[["true"]]})
}
fn never(_: &Value) -> Result<Value> { panic!("unexpected compiler invocation") }
fn strings(items: &[&str]) -> Vec<String> { items.iter().map(|s| s.to_string()).collect() }

#[test]
fn dependencies_link_and_markdown_parses() {
    assert_eq!(hash(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert!(Parser::new("[name](file.md)").any(|e| matches!(e, Event::Start(Tag::Link { .. }))));
    assert_eq!(percent_decode_str("a%20b").decode_utf8().unwrap(), "a b");
    let value: Value = serde_json::from_str("{\"available\":true}").unwrap();
    assert_eq!(value["available"], true);
}
#[test]
fn init_preserves_sources_and_creates_greeting() {
    let t = Scratch::new().unwrap();
    init(&t.0).unwrap();
    let path = t.0.join("spec/app.nat");
    assert!(fs::read_to_string(&path).unwrap().contains("defaulting to the world"));
    fs::write(&path, "Custom behavior").unwrap();
    init(&t.0).unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "Custom behavior");
    assert!(t.0.join("generated").is_dir());
    assert!(t.0.join("inferred").is_dir());
    fs::create_dir_all(t.0.join("spec/nested")).unwrap();
    fs::rename(t.0.join("spec/app.nat"), t.0.join("spec/nested/custom.nat")).unwrap();
    init(&t.0).unwrap();
    assert!(!t.0.join("spec/app.nat").exists());
}
#[test]
fn full_noop_force_and_removal() {
    let t = fixture();
    install(&t.0);
    let original = fs::metadata(t.0.join("generated/a.txt")).unwrap().modified().unwrap();
    build_using(&t.0, false, None, &mut never).unwrap();
    let mut count = 0;
    build_using(&t.0, true, None, &mut |r| {
        count += 1;
        assert_eq!(r["scope"], "full");
        assert_eq!(r["human_sources"]["a.nat"]["content"], "First feature");
        assert_eq!(r["previous_semantic_record"]["requirements"][0]["id"], "REQ-A");
        assert!(r["previous_inferences"].as_str().unwrap().contains("ASSUME-A"));
        Ok(serde_json::to_value(full()).unwrap())
    }).unwrap();
    assert_eq!(count, 1);
    assert_eq!(fs::metadata(t.0.join("generated/a.txt")).unwrap().modified().unwrap(), original);
    let mut replacement = full();
    replacement.files.remove(1);
    build_using(&t.0, true, None, &mut |_| Ok(serde_json::to_value(&replacement).unwrap())).unwrap();
    assert!(!t.0.join("generated/b.txt").exists());
    current(&t.0).unwrap();
}
#[test]
fn explicit_proposals_apply_on_current_builds_and_keep_validation() {
    let t = fixture();
    install(&t.0);
    let path = t.0.join("proposal.json");
    let before = fs::read(t.0.join("generated").join(MANIFEST)).unwrap();
    fs::write(&path, b"not JSON").unwrap();
    assert!(build(&t.0, Some(&path), false).is_err());
    assert_eq!(fs::read(t.0.join("generated").join(MANIFEST)).unwrap(), before);
    let mut c = full();
    c.assumptions[0].source = "spec/a.nat".into();
    fs::write(&path, pretty(&c).unwrap()).unwrap();
    let error = build(&t.0, Some(&path), false).unwrap_err().to_string();
    assert!(error.contains("invalid assumption ASSUME-A or source"), "{error}");
    assert_eq!(fs::read(t.0.join("generated").join(MANIFEST)).unwrap(), before);
    c.assumptions[0].source = "a.nat".into();
    c.files[0].content = "explicit update".into();
    fs::write(&path, pretty(&c).unwrap()).unwrap();
    build(&t.0, Some(&path), false).unwrap();
    assert_eq!(fs::read_to_string(t.0.join("generated/a.txt")).unwrap(), "explicit update");
    current(&t.0).unwrap();
    fs::remove_file(&path).unwrap();
    assert!(build(&t.0, Some(&path), false).is_err());
    build_using(&t.0, false, None, &mut never).unwrap();
}
#[test]
fn invalid_semantic_diagnostics_name_kind_and_id() {
    let t = fixture();
    let s = read_sources(&t.0).unwrap();
    for kind in ["requirement", "assumption"] {
        for bad_id in [false, true] {
            let mut c = full();
            let (id, source) = if kind == "requirement" {
                let record = &mut c.requirements[0];
                (&mut record.id, &mut record.source)
            } else {
                let record = &mut c.assumptions[0];
                (&mut record.id, &mut record.source)
            };
            if bad_id { *id = "bad ID!".into(); } else { *source = "a.nat#citation".into(); }
            let expected = format!("invalid {kind} {id} or source");
            let error = validate(&c, &s).unwrap_err().to_string();
            assert!(error.contains(&expected), "{error}");
        }
    }
}
#[test]
fn scoped_build_preserves_unaffected_bytes_time_and_records() {
    let t = fixture();
    install(&t.0);
    let b = t.0.join("generated/b.txt");
    let time = fs::metadata(&b).unwrap().modified().unwrap();
    fs::write(t.0.join("spec/a.nat"), "Changed first feature").unwrap();
    build_using(&t.0, false, None, &mut |r| {
        assert_eq!(r["scope"], "scoped");
        assert_eq!(r["human_sources"].as_object().unwrap().len(), 1);
        assert!(r["human_sources"].get("b.nat").is_none());
        assert_eq!(r["affected_outputs"].as_array().unwrap().len(), 1);
        assert_eq!(r["affected_outputs"][0]["content"], "first");
        assert_eq!(r["previous_dependencies"]["b.txt"], json!(["b.nat"]));
        assert!(r["reserved_ids"].as_array().unwrap().contains(&json!("REQ-B")));
        Ok(patch())
    }).unwrap();
    assert_eq!(fs::read_to_string(t.0.join("generated/a.txt")).unwrap(), "updated");
    assert_eq!(fs::read_to_string(&b).unwrap(), "second");
    assert_eq!(fs::metadata(b).unwrap().modified().unwrap(), time);
    let ir = semantic(&t.0).unwrap();
    assert!(ir.requirements.iter().any(|r| r.id == "REQ-B" && r.statement == "Second feature"));
    assert!(ir.assumptions.is_empty());
    current(&t.0).unwrap();
}
#[test]
fn explicit_scoped_proposal_preserves_unaffected_output() {
    let t = fixture();
    install(&t.0);
    let b = t.0.join("generated/b.txt");
    let before = fs::metadata(&b).unwrap().modified().unwrap();
    fs::write(t.0.join("spec/a.nat"), "Changed first feature").unwrap();
    let path = t.0.join("patch.json");
    fs::write(&path, pretty(&patch()).unwrap()).unwrap();
    build(&t.0, Some(&path), false).unwrap();
    assert_eq!(fs::read_to_string(t.0.join("generated/a.txt")).unwrap(), "updated");
    assert_eq!(fs::metadata(&b).unwrap().modified().unwrap(), before);
    assert_eq!(fs::read_to_string(b).unwrap(), "second");
    current(&t.0).unwrap();
}
#[test]
fn scoped_removal_and_addition() {
    let t = fixture();
    install(&t.0);
    fs::write(t.0.join("spec/a.nat"), "New representation").unwrap();
    build_using(&t.0, false, None, &mut |_| {
        let mut p = patch();
        p["files"][0]["path"] = json!("new.txt");
        p["remove_files"] = json!(["a.txt"]);
        Ok(p)
    }).unwrap();
    assert!(!t.0.join("generated/a.txt").exists());
    assert_eq!(fs::read_to_string(t.0.join("generated/new.txt")).unwrap(), "updated");
    assert_eq!(fs::read_to_string(t.0.join("generated/b.txt")).unwrap(), "second");
    current(&t.0).unwrap();
}
#[test]
fn invalid_scope_falls_back_before_writing() {
    for corruption in ["unaffected", "reserved", "incomplete", "escalate", "source", "malformed", "remove", "decode", "dependencies"] {
        let t = fixture();
        install(&t.0);
        fs::write(t.0.join("spec/a.nat"), "Edited").unwrap();
        let mut calls = 0;
        build_using(&t.0, false, None, &mut |r| {
            calls += 1;
            assert_eq!(fs::read_to_string(t.0.join("generated/a.txt")).unwrap(), "first");
            if calls == 1 {
                assert_eq!(r["scope"], "scoped");
                let mut p = patch();
                match corruption {
                    "unaffected" => p["files"][0]["path"] = json!("b.txt"),
                    "reserved" => p["requirements"][0]["id"] = json!("REQ-B"),
                    "incomplete" => p["files"] = json!([]),
                    "escalate" => p["full_rebuild_required"] = json!(true),
                    "source" => p["requirements"][0]["source"] = json!("b.nat"),
                    "malformed" => p["files"] = json!(false),
                    "remove" => p["remove_files"] = json!(["b.txt"]),
                    "decode" => return Err("invalid compiler JSON".into()),
                    "dependencies" => { p["files"][0].as_object_mut().unwrap().remove("sources"); },
                    _ => unreachable!(),
                }
                Ok(p)
            } else {
                assert_eq!(r["scope"], "full");
                Ok(serde_json::to_value(full()).unwrap())
            }
        }).unwrap();
        assert_eq!(calls, 2);
    }
}
#[test]
fn legacy_manifest_and_missing_dependencies_fall_back() {
    for version in [1, 2] {
        let t = fixture();
        install(&t.0);
        let path = t.0.join("generated").join(MANIFEST);
        let mut old: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        old["version"] = json!(version);
        old.as_object_mut().unwrap().remove("file_sources");
        if version == 1 { old.as_object_mut().unwrap().remove("source_hashes"); }
        fs::write(&path, pretty(&old).unwrap()).unwrap();
        current(&t.0).unwrap();
        build_using(&t.0, false, None, &mut never).unwrap();
        fs::write(t.0.join("spec/a.nat"), "Edited").unwrap();
        build_using(&t.0, false, None, &mut |r| {
            assert_eq!(r["scope"], "full");
            Ok(serde_json::to_value(full()).unwrap())
        }).unwrap();
    }
}
#[test]
fn older_full_compiler_and_saved_responses_default_only_missing_dependencies() {
    for saved in [false, true] {
        let t = fixture();
        let path = t.0.join("proposal.json");
        let mut old = serde_json::to_value(full()).unwrap();
        old["files"][0].as_object_mut().unwrap().remove("sources");
        fs::write(&path, pretty(&old).unwrap()).unwrap();
        let m = if saved { build(&t.0, Some(&path), false).unwrap() }
            else { build_using(&t.0, false, None, &mut |_| Ok(old.clone())).unwrap() };
        assert_eq!(m.file_sources["a.txt"], strings(&["a.nat", "b.nat"]));
        assert_eq!(m.file_sources["b.txt"], strings(&["b.nat"]));
        current(&t.0).unwrap();
        for invalid in [json!([]), Value::Null, json!(["missing.nat"])] {
            old["files"][0]["sources"] = invalid;
            fs::write(&path, pretty(&old).unwrap()).unwrap();
            let result = if saved { build(&t.0, Some(&path), true) }
                else { build_using(&t.0, true, None, &mut |_| Ok(old.clone())) };
            assert!(result.is_err());
            assert_eq!(fs::read_to_string(t.0.join("generated/a.txt")).unwrap(), "first");
            current(&t.0).unwrap();
        }
        let mut scoped = patch();
        scoped["files"][0].as_object_mut().unwrap().remove("sources");
        fs::write(&path, pretty(&scoped).unwrap()).unwrap();
        let response = saved_response(&path).unwrap();
        assert!(response["files"][0].get("sources").is_none());
        assert!(serde_json::from_value::<Patch>(response).is_err());
    }
}
#[test]
fn changes_to_source_set_and_all_affected_force_full() {
    let t = fixture();
    let m = install(&t.0);
    fs::write(t.0.join("spec/c.nat"), "Third feature").unwrap();
    assert!(scope_for(&m, &read_sources(&t.0).unwrap()).is_none());
    assert!(current(&t.0).is_err());
    fs::remove_file(t.0.join("spec/c.nat")).unwrap();
    fs::remove_file(t.0.join("spec/b.nat")).unwrap();
    assert!(scope_for(&m, &read_sources(&t.0).unwrap()).is_none());
    fs::write(t.0.join("spec/a.nat"), "changed a").unwrap();
    fs::write(t.0.join("spec/b.nat"), "changed b").unwrap();
    assert!(scope_for(&m, &read_sources(&t.0).unwrap()).is_none());
}
#[test]
fn modified_and_missing_artifacts_are_refused_before_compiler() {
    for (path, remove) in [("generated/a.txt", false), ("generated/b.txt", true), ("inferred/assumptions.inat", false), ("generated/ir.json", true)] {
        let t = fixture();
        install(&t.0);
        if remove { fs::remove_file(t.0.join(path)).unwrap(); }
        else { fs::write(t.0.join(path), "human edit").unwrap(); }
        assert!(current(&t.0).is_err());
        assert!(build_using(&t.0, true, None, &mut never).is_err());
    }
}
#[test]
fn full_validation_is_all_or_nothing() {
    for invalid in ["escape", "reserved", "parent", "id", "source", "argv", "untracked", "empty-check", "empty-id", "duplicate-path"] {
        let t = fixture();
        let mut c = full();
        match invalid {
            "escape" => c.files[1].path = "../outside".into(),
            "reserved" => c.files[1].path = IR.into(),
            "parent" => c.files[1].path = "a.txt/child".into(),
            "id" => c.requirements[1].id = c.requirements[0].id.clone(),
            "source" => c.files[1].sources = vec!["spec/a.nat".into()],
            "argv" => c.run.push(String::new()),
            "untracked" => fs::write(t.0.join("generated/b.txt"), "keep me").unwrap(),
            "empty-check" => c.checks.push(vec![]),
            "empty-id" => c.requirements[0].id.clear(),
            "duplicate-path" => c.files[1].path = c.files[0].path.clone(),
            _ => unreachable!(),
        }
        assert!(build_using(&t.0, true, None, &mut |_| Ok(serde_json::to_value(&c).unwrap())).is_err());
        assert!(!t.0.join("generated/a.txt").exists());
        assert!(!t.0.join("generated").join(MANIFEST).exists());
        if invalid == "untracked" { assert_eq!(fs::read_to_string(t.0.join("generated/b.txt")).unwrap(), "keep me"); }
    }
}
#[test]
fn references_are_relative_direct_and_stale() {
    let t = fixture();
    fs::create_dir_all(t.0.join("spec/nested")).unwrap();
    fs::create_dir_all(t.0.join("docs")).unwrap();
    fs::write(t.0.join("docs/a b.md"), "Supporting text [ignored](missing.md)").unwrap();
    let content = "# Intent\n\n[context][c]\n\n[c]: ../../docs/a%20b.md#section\n\n![image](../../docs/a%20b.md)\n[remote](https://example.invalid/a) [mail](mailto:a@example.invalid) [anchor](#x)\n<img src=\"missing.png\">\n`[code](missing.md)`\n";
    fs::write(t.0.join("spec/nested/view.nat"), content).unwrap();
    let before = read_sources(&t.0).unwrap();
    assert_eq!(before["nested/view.nat"].content, content);
    assert_eq!(before["nested/view.nat"].references.len(), 1);
    assert!(before["nested/view.nat"].references.contains_key("docs/a b.md"));
    fs::write(t.0.join("docs/a b.md"), "Changed context").unwrap();
    let after = read_sources(&t.0).unwrap();
    assert_ne!(input_hash(&before), input_hash(&after));
    assert_eq!(source_hashes(&before)["a.nat"], source_hashes(&after)["a.nat"]);
    assert_ne!(source_hashes(&before)["nested/view.nat"], source_hashes(&after)["nested/view.nat"]);
    fs::write(t.0.join("spec/nested/view.nat"), "[missing](../../docs/no.md)").unwrap();
    let error = read_sources(&t.0).unwrap_err().to_string();
    assert!(error.contains("nested/view.nat") && error.contains("no.md"));
    for link in ["../../../outside", "../../generated/a.txt", "../../inferred/assumptions.inat"] {
        fs::write(t.0.join("spec/nested/view.nat"), format!("[context]({link})")).unwrap();
        assert!(read_sources(&t.0).is_err());
    }
}
fn assert_alias_references(root: &Path) {
    init(root).unwrap();
    fs::create_dir_all(root.join("spec/nested")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("docs/context.md"), "supporting context").unwrap();
    fs::write(root.join("spec/app.nat"), "[ordinary](../docs/context.md)").unwrap();
    fs::write(root.join("spec/nested/view.nat"), "![nested](../../docs/context.md)").unwrap();
    let canonical = fs::canonicalize(root).unwrap();
    let sources = read_sources(root).unwrap();
    let canonical_sources = read_sources(&canonical).unwrap();
    assert_eq!(input_hash(&sources), input_hash(&canonical_sources));
    for name in ["app.nat", "nested/view.nat"] {
        assert_eq!(sources[name].references.len(), 1);
        assert_eq!(sources[name].references["docs/context.md"], hash(b"supporting context"));
    }
    fs::write(root.join("spec/nested/view.nat"), format!("[absolute](<{}>)", root.join("docs/context.md").display())).unwrap();
    let absolute_sources = read_sources(root).unwrap();
    assert!(absolute_sources["nested/view.nat"].references.contains_key("docs/context.md"));
    fs::write(root.join("docs/context.md"), "changed context").unwrap();
    let after = read_sources(root).unwrap();
    assert_ne!(source_hashes(&sources)["app.nat"], source_hashes(&after)["app.nat"]);
    assert_ne!(source_hashes(&absolute_sources)["nested/view.nat"], source_hashes(&after)["nested/view.nat"]);
}
#[cfg(unix)]
#[test]
fn project_root_alias_accepts_ordinary_nested_and_absolute_references() {
    let target = Scratch::new().unwrap();
    let holder = Scratch::new().unwrap();
    let alias = holder.0.join("project-alias");
    std::os::unix::fs::symlink(fs::canonicalize(&target.0).unwrap(), &alias).unwrap();
    assert_ne!(alias, fs::canonicalize(&alias).unwrap());
    assert_alias_references(&alias);
}
#[cfg(target_os = "macos")]
#[test]
fn macos_var_private_var_reference_regression() {
    let t = Scratch(PathBuf::from("/var/tmp").join(format!("nat-var-reference-{}", unique())));
    fs::create_dir(&t.0).unwrap();
    assert_ne!(t.0, fs::canonicalize(&t.0).unwrap());
    assert_alias_references(&t.0);
}
#[test]
fn linked_file_edit_changes_each_referring_spec() {
    let t = fixture();
    fs::write(t.0.join("context.md"), "before").unwrap();
    for p in ["a.nat", "b.nat"] { fs::write(t.0.join("spec").join(p), "[context](../context.md)").unwrap(); }
    let m = install(&t.0);
    fs::write(t.0.join("context.md"), "after").unwrap();
    let s = read_sources(&t.0).unwrap();
    assert!(current(&t.0).is_err());
    assert!(source_hashes(&s).iter().all(|(k, v)| m.source_hashes.get(k) != Some(v)));
    assert!(scope_for(&m, &s).is_none());
}
#[cfg(unix)]
#[test]
fn canonical_containment_still_rejects_escaping_and_generated_references() {
    use std::os::unix::fs::symlink;
    let t = fixture();
    let outside = Scratch::new().unwrap();
    fs::write(outside.0.join("secret.txt"), "outside").unwrap();
    symlink(fs::canonicalize(&outside.0).unwrap(), t.0.join("outside-link")).unwrap();
    fs::write(t.0.join("spec/a.nat"), "[escape](../outside-link/secret.txt)").unwrap();
    let message = read_sources(&t.0).unwrap_err().to_string();
    assert!(message.contains("a.nat") && message.contains("outside-link") && message.contains("escapes"));
    fs::write(t.0.join("generated/private.txt"), "generated context").unwrap();
    symlink(fs::canonicalize(t.0.join("generated")).unwrap(), t.0.join("generated-link")).unwrap();
    fs::write(t.0.join("spec/a.nat"), "[bad](../generated-link/private.txt)").unwrap();
    assert!(read_sources(&t.0).unwrap_err().to_string().contains("generated/"));
}
#[cfg(unix)]
#[test]
fn symlink_sources_and_outputs_are_refused() {
    use std::os::unix::fs::symlink;
    let t = fixture();
    symlink(t.0.join("spec/a.nat"), t.0.join("spec/link.nat")).unwrap();
    assert!(read_sources(&t.0).is_err());
    fs::remove_file(t.0.join("spec/link.nat")).unwrap();
    symlink(t.0.join("spec/a.nat"), t.0.join("generated/a.txt")).unwrap();
    assert!(build_using(&t.0, true, None, &mut |_| Ok(serde_json::to_value(full()).unwrap())).is_err());
    assert_eq!(fs::read_to_string(t.0.join("spec/a.nat")).unwrap(), "First feature");
}
#[test]
fn inference_keeps_exact_source_and_complete_reason() {
    let t = fixture();
    fs::create_dir_all(t.0.join("spec/nested")).unwrap();
    fs::write(t.0.join("spec/nested/ui.nat"), "A greeting UI").unwrap();
    let mut c = full();
    c.assumptions[0].source = "nested/ui.nat".into();
    c.assumptions[0].reason = "First paragraph.\n\nSecond paragraph with detailed context.".into();
    build_using(&t.0, false, None, &mut |_| Ok(serde_json::to_value(&c).unwrap())).unwrap();
    let text = fs::read_to_string(t.0.join("inferred").join(INAT)).unwrap();
    assert!(text.lines().any(|line| line == "Source: spec/nested/ui.nat"));
    assert!(text.contains(&format!("Reason: {}", c.assumptions[0].reason)));
    assert_eq!(semantic(&t.0).unwrap().assumptions[0].source, "nested/ui.nat");
    current(&t.0).unwrap();
}
#[test]
fn promotion_preserves_current_build_and_semantic_id() {
    let t = fixture();
    fs::write(t.0.join("spec/promoted.nat"), "Existing human text.\n").unwrap();
    install(&t.0);
    promote(&t.0, "ASSUME-A").unwrap();
    let manifest = current(&t.0).unwrap();
    assert_eq!(manifest.file_sources["a.txt"], strings(&["a.nat", "promoted.nat"]));
    assert_eq!(manifest.file_sources["b.txt"], strings(&["b.nat"]));
    let content = fs::read_to_string(t.0.join("spec/promoted.nat")).unwrap();
    assert!(content.starts_with("Existing human text.\n"));
    assert!(content.contains("Use English greetings."));
    assert_eq!(fs::read_to_string(t.0.join("spec/a.nat")).unwrap(), "First feature");
    let ir = semantic(&t.0).unwrap();
    assert!(ir.assumptions.is_empty());
    assert!(ir.requirements.iter().any(|r| r.id == "ASSUME-A" && r.source == "promoted.nat"));
    assert!(!fs::read_to_string(t.0.join("inferred").join(INAT)).unwrap().contains("## ASSUME-A"));
    build_using(&t.0, false, None, &mut never).unwrap();
    assert!(promote(&t.0, "missing").is_err());
    assert_eq!(fs::read_to_string(t.0.join("spec/promoted.nat")).unwrap(), content);
}
#[test]
fn promotion_refuses_stale_build() {
    let t = fixture();
    install(&t.0);
    fs::write(t.0.join("spec/a.nat"), "changed").unwrap();
    assert!(promote(&t.0, "ASSUME-A").is_err());
    assert!(!t.0.join("spec/promoted.nat").exists());
}
#[test]
fn input_edits_during_compilation_do_not_commit() {
    let t = fixture();
    assert!(build_using(&t.0, true, None, &mut |_| {
        fs::write(t.0.join("spec/a.nat"), "racing edit").unwrap();
        Ok(serde_json::to_value(full()).unwrap())
    }).is_err());
    assert!(!t.0.join("generated/a.txt").exists());
}
#[test]
fn parsing_and_argument_forwarding() {
    let c = parse(strings(&["run", "--project", "project", "--", "--project", "literal", "a b", "", "$(false)"])).unwrap();
    assert_eq!(c.project, Some(PathBuf::from("project")));
    assert_eq!(c.positional, strings(&["--project", "literal", "a b", "", "$(false)"]));
    for args in [vec!["bad"], vec!["build", "--project"], vec!["build", "--project", "--full"], vec!["watch", "--full"], vec!["init", "a", "b"], vec!["promote"], vec!["test", "--proposal", "x"], vec!["help", "extra"]] {
        assert!(parse(strings(&args)).is_err(), "{args:?}");
    }
    for alias in ["help", "--help", "-h", "version", "--version", "-V"] { assert!(parse(strings(&[alias])).is_ok()); }
}
#[test]
fn watch_debounce_waits_after_last_change_and_recovers() {
    let now = Instant::now();
    let mut d = Debounce::new("initial".into());
    assert!(!d.observe("edited".into(), now));
    assert!(!d.observe("edited".into(), now + Duration::from_millis(399)));
    assert!(!d.observe("error:missing context".into(), now + Duration::from_millis(400)));
    assert!(d.observe("error:missing context".into(), now + Duration::from_millis(850)));
    assert!(!d.observe("fixed".into(), now + Duration::from_millis(900)));
    assert!(d.observe("fixed".into(), now + Duration::from_millis(1300)));
    assert!(!d.observe("fixed".into(), now + Duration::from_secs(5)));
}
#[test]
fn missing_executable_and_ordinary_failure_are_distinct() {
    let t = fixture();
    let missing = execute(&t.0, &strings(&["/nat-test/nonexistent-interpreter"]), &[]).unwrap();
    assert!(missing.dependency_missing && !missing.success);
    let failure = execute(&t.0, &strings(&["false"]), &[]).unwrap();
    assert!(!failure.dependency_missing && !failure.success);
    for text in ["ModuleNotFoundError: No module named 'tkinter'", "dyld: Library not loaded: X", "TclError: no display name and no $DISPLAY environment variable", "error while loading shared libraries: x.so", "Error: Cannot find module 'missing'"] {
        assert!(dependency_diagnostic(text));
    }
    assert!(!dependency_diagnostic("ValueError: invalid name"));
}
#[test]
fn runtime_repair_is_full_checked_and_retried_once_with_original_arguments() {
    for retry_succeeds in [true, false] {
        let t = fixture();
        install(&t.0);
        let args = strings(&["a b", "--name", "$(echo no)"]);
        let mut compiler_calls = 0;
        let mut executions = 0;
        let result = run_using(&t.0, &args, &mut |r| {
            compiler_calls += 1;
            assert_eq!(r["scope"], "full");
            assert!(r["runtime_failure_feedback"].as_str().unwrap().contains("ModuleNotFoundError"));
            Ok(serde_json::to_value(full()).unwrap())
        }, &mut |root, command, forwarded| {
            assert_eq!(root, t.0.as_path());
            executions += 1;
            if executions == 2 {
                assert_eq!(command, &strings(&["true"]));
                assert!(forwarded.is_empty());
                return Ok(Execution { success: true, dependency_missing: false, diagnostic: String::new() });
            }
            assert_eq!(forwarded, args.as_slice());
            Ok(Execution { success: executions == 3 && retry_succeeds, dependency_missing: true,
                diagnostic: "ModuleNotFoundError: unavailable".into() })
        });
        assert_eq!(result.is_ok(), retry_succeeds);
        assert_eq!(compiler_calls, 1);
        assert_eq!(executions, 3);
    }
}
#[test]
fn ordinary_run_failure_does_not_regenerate_and_failed_checks_prevent_retry() {
    let t = fixture();
    install(&t.0);
    assert!(run_using(&t.0, &[], &mut never, &mut |_, _, _| Ok(Execution {
        success: false, dependency_missing: false, diagnostic: "application failure".into(),
    })).is_err());
    let mut calls = 0;
    assert!(run_using(&t.0, &[], &mut |_| Ok(serde_json::to_value(full()).unwrap()), &mut |_, _, _| {
        calls += 1;
        Ok(Execution { success: false, dependency_missing: calls == 1, diagnostic: "failure".into() })
    }).is_err());
    assert_eq!(calls, 2);
}
#[test]
fn test_requires_checks_and_executes_all_checks() {
    let t = fixture();
    let mut m = install(&t.0);
    m.checks.clear();
    assert!(checks_using(&t.0, &m, &mut execute).is_err());
    m.checks = vec![strings(&["false"]), strings(&["true"])];
    let mut calls = 0;
    assert!(checks_using(&t.0, &m, &mut |_, _, _| {
        calls += 1;
        Ok(Execution { success: calls != 1, dependency_missing: false, diagnostic: "failed".into() })
    }).is_err());
    assert_eq!(calls, 2);
}
