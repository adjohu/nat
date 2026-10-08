use super::*;

fn strings(items: &[&str]) -> Vec<String> { items.iter().map(|s| s.to_string()).collect() }
fn fixture() -> Scratch {
    let t = Scratch::new().unwrap(); init(&t.0).unwrap();
    fs::remove_file(t.0.join("spec/app.nat")).unwrap();
    fs::write(t.0.join("spec/a.nat"), "First feature").unwrap();
    fs::write(t.0.join("spec/b.nat"), "Second feature").unwrap(); t
}
fn full() -> Value {
    json!({"summary":"fixture","requirements":[
        {"id":"REQ-A","source":"a.nat","statement":"First feature"},
        {"id":"REQ-B","source":"b.nat","statement":"Second feature"}],
        "assumptions":[{"id":"ASSUME-A","source":"a.nat","statement":"Use English greetings.","reason":"No language specified.\n\nFull second paragraph."}],
        "files":[{"path":"a.txt","content":"first","sources":["a.nat"]},{"path":"b.txt","content":"second","sources":["b.nat"]}],
        "run":["/bin/echo","greeting"],"checks":[["true"]]})
}
fn install(root: &Path) -> Manifest { build_using(root, false, None, &mut |_| Ok(full())).unwrap() }
fn never(_: &Value) -> Result<Value> { panic!("unexpected compiler invocation") }
fn patch() -> Value {
    json!({"full_rebuild_required":false,"reason":"","summary":"scoped",
        "requirements":[{"id":"REQ-A","source":"a.nat","statement":"Changed feature"}],"assumptions":[],
        "files":[{"path":"a.txt","content":"updated","sources":["a.nat"]}],"remove_files":[],"run":["/bin/echo","updated"],"checks":[["true"]]})
}
#[test]
fn dependencies_and_initialization() {
    assert_eq!(hash(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert!(Parser::new("[x](file.md)").any(|e| matches!(e, Event::Start(Tag::Link { .. }))));
    assert_eq!(percent_decode_str("a%20b").decode_utf8().unwrap(), "a b");
    let t = Scratch::new().unwrap(); init(&t.0).unwrap();
    assert!(fs::read_to_string(t.0.join("spec/app.nat")).unwrap().contains("defaulting to the world"));
    fs::create_dir_all(t.0.join("spec/nested")).unwrap();
    fs::rename(t.0.join("spec/app.nat"), t.0.join("spec/nested/custom.nat")).unwrap();
    fs::write(t.0.join("spec/nested/custom.nat"), "Custom").unwrap(); init(&t.0).unwrap();
    assert!(!t.0.join("spec/app.nat").exists());
    assert_eq!(fs::read_to_string(t.0.join("spec/nested/custom.nat")).unwrap(), "Custom");
}
#[test]
fn full_noop_inventory_and_legacy_dependencies() {
    let t = fixture(); install(&t.0);
    build_using(&t.0, false, None, &mut never).unwrap();
    let before = fs::metadata(t.0.join("generated/a.txt")).unwrap().modified().unwrap();
    build_using(&t.0, true, None, &mut |r| {
        assert_eq!(r["scope"], "full"); assert_eq!(r["human_sources"].as_object().unwrap().len(), 2);
        assert!(r["previous_inferences"].as_str().unwrap().contains("ASSUME-A"));
        assert_eq!(r["reuse_catalog"].as_array().unwrap().len(), 2);
        assert!(r["reuse_catalog"][0].get("content").is_none()); Ok(full())
    }).unwrap();
    assert_eq!(fs::metadata(t.0.join("generated/a.txt")).unwrap().modified().unwrap(), before);
    let mut response = full(); response["files"].as_array_mut().unwrap().pop();
    response["files"][0].as_object_mut().unwrap().remove("sources");
    let m = build_using(&t.0, true, None, &mut |_| Ok(response.clone())).unwrap();
    assert_eq!(m.file_sources["a.txt"], strings(&["a.nat","b.nat"]));
    assert!(!t.0.join("generated/b.txt").exists()); current(&t.0).unwrap();
}
#[test]
fn scope_preserves_records_and_allows_removal_addition() {
    for rename in [false, true] {
        let t = fixture(); install(&t.0);
        let b = t.0.join("generated/b.txt"); let time = fs::metadata(&b).unwrap().modified().unwrap();
        fs::write(t.0.join("spec/a.nat"), "Changed").unwrap();
        build_using(&t.0, false, None, &mut |r| {
            assert_eq!(r["scope"], "scoped"); assert!(r.get("reuse_catalog").is_none());
            assert_eq!(r["human_sources"].as_object().unwrap().len(), 1);
            assert!(r["human_sources"].get("b.nat").is_none());
            assert_eq!(r["affected_outputs"][0]["content"], "first");
            assert_eq!(r["reserved_ids"], json!(["REQ-B"]));
            let mut p = patch();
            if rename { p["files"][0]["path"] = json!("new.txt"); p["remove_files"] = json!(["a.txt"]); }
            Ok(p)
        }).unwrap();
        assert_eq!(fs::read_to_string(&b).unwrap(), "second");
        assert_eq!(fs::metadata(b).unwrap().modified().unwrap(), time);
        assert_eq!(t.0.join("generated/a.txt").exists(), !rename);
        let ir = semantic(&t.0).unwrap(); assert!(ir.assumptions.is_empty());
        assert!(ir.requirements.iter().any(|r| r.id == "REQ-B" && r.statement == "Second feature"));
        current(&t.0).unwrap();
    }
}
#[test]
fn invalid_scopes_fall_back_before_writing() {
    for bad in ["unaffected","reserved","incomplete","escalate","source","malformed","remove","decode","dependencies","null"] {
        let t = fixture(); install(&t.0); fs::write(t.0.join("spec/a.nat"), "Changed").unwrap();
        let mut calls = 0;
        build_using(&t.0, false, None, &mut |r| {
            calls += 1; assert_eq!(fs::read_to_string(t.0.join("generated/a.txt")).unwrap(), "first");
            if calls > 1 { assert_eq!(r["scope"], "full"); return Ok(full()); }
            assert_eq!(r["scope"], "scoped"); let mut p = patch();
            match bad {
                "unaffected" => p["files"][0]["path"] = json!("b.txt"),
                "reserved" => p["requirements"][0]["id"] = json!("REQ-B"),
                "incomplete" => p["files"] = json!([]),
                "escalate" => p["full_rebuild_required"] = json!(true),
                "source" => p["requirements"][0]["source"] = json!("b.nat"),
                "malformed" => p["files"] = json!(false),
                "remove" => p["remove_files"] = json!(["b.txt"]),
                "decode" => return Err("invalid JSON".into()),
                "dependencies" => { p["files"][0].as_object_mut().unwrap().remove("sources"); },
                "null" => p["files"][0]["content"] = Value::Null,
                _ => unreachable!(),
            } Ok(p)
        }).unwrap(); assert_eq!(calls, 2);
    }
}
#[test]
fn old_manifests_and_source_set_changes_force_full() {
    for version in [1, 2] {
        let t = fixture(); let m = install(&t.0);
        fs::write(t.0.join("spec/c.nat"), "Third").unwrap();
        assert!(scope_for(&m, &read_sources(&t.0).unwrap()).is_none());
        fs::remove_file(t.0.join("spec/c.nat")).unwrap();
        let path = t.0.join("generated").join(MANIFEST);
        let mut old: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        old["version"] = json!(version); old.as_object_mut().unwrap().remove("file_sources");
        if version == 1 { old.as_object_mut().unwrap().remove("source_hashes"); }
        fs::write(&path, pretty(&old).unwrap()).unwrap(); current(&t.0).unwrap();
        fs::write(t.0.join("spec/a.nat"), "Changed").unwrap();
        build_using(&t.0, false, None, &mut |r| { assert_eq!(r["scope"], "full"); Ok(full()) }).unwrap();
    }
}
#[test]
fn whole_result_validation_and_semantic_diagnostics() {
    for bad in ["escape","reserved","parent","id","source","argv","untracked","empty-check","empty-id","duplicate","assumption"] {
        let t = fixture(); let mut c: Compilation = serde_json::from_value(full()).unwrap();
        match bad {
            "escape" => c.files[1].path = "../outside".into(),
            "reserved" => c.files[1].path = IR.into(),
            "parent" => c.files[1].path = "a.txt/child".into(),
            "id" => c.requirements[1].id = c.requirements[0].id.clone(),
            "source" => c.files[1].sources = strings(&["spec/a.nat"]),
            "argv" => c.run.push(String::new()),
            "untracked" => fs::write(t.0.join("generated/b.txt"), "keep").unwrap(),
            "empty-check" => c.checks.push(vec![]),
            "empty-id" => c.requirements[0].id.clear(),
            "duplicate" => c.files[1].path = c.files[0].path.clone(),
            "assumption" => { let record = &mut c.assumptions[0]; record.id = "A-01".into(); record.source = "spec/a.nat".into(); },
            _ => unreachable!(),
        }
        let error = build_using(&t.0, true, None, &mut |_| Ok(serde_json::to_value(&c).unwrap())).unwrap_err().to_string();
        if bad == "assumption" { assert!(error.contains("invalid assumption A-01 or source")); }
        assert!(!t.0.join("generated/a.txt").exists()); assert!(!t.0.join("generated").join(MANIFEST).exists());
        if bad == "untracked" { assert_eq!(fs::read_to_string(t.0.join("generated/b.txt")).unwrap(), "keep"); }
    }
}
#[test]
fn drift_is_refused_before_compiler() {
    for (path, remove) in [("generated/a.txt",false),("generated/b.txt",true),("generated/ir.json",true),("inferred/assumptions.inat",false)] {
        let t = fixture(); install(&t.0);
        if remove { fs::remove_file(t.0.join(path)).unwrap(); } else { fs::write(t.0.join(path), "edit").unwrap(); }
        assert!(current(&t.0).is_err()); assert!(build_using(&t.0, true, None, &mut never).is_err());
    }
}
#[test]
fn references_are_direct_relative_and_change_each_referring_source() {
    let t = fixture(); fs::create_dir_all(t.0.join("spec/nested")).unwrap();
    fs::write(t.0.join("context file.md"), "[indirect](missing.md)").unwrap();
    fs::write(t.0.join("spec/a.nat"), "[context](../context%20file.md)").unwrap();
    let markdown = "# View\n![image](../../context%20file.md)\n[remote](https://example.invalid) [mail](mailto:a@b) [anchor](#x)\n<img src='missing.png'>\n`[code](missing.md)`\n";
    fs::write(t.0.join("spec/nested/view.nat"), markdown).unwrap();
    let before = read_sources(&t.0).unwrap();
    assert_eq!(before["nested/view.nat"].content, markdown);
    assert_eq!(before["nested/view.nat"].references.len(), 1);
    assert!(before["nested/view.nat"].references.contains_key("context file.md"));
    fs::write(t.0.join("context file.md"), "changed").unwrap(); let after = read_sources(&t.0).unwrap();
    for name in ["a.nat","nested/view.nat"] { assert_ne!(source_hashes(&before)[name], source_hashes(&after)[name]); }
    assert_eq!(source_hashes(&before)["b.nat"], source_hashes(&after)["b.nat"]);
    for destination in ["missing.md","../../../outside","../../generated/a.txt","../../inferred/assumptions.inat"] {
        fs::write(t.0.join("spec/nested/view.nat"), format!("[x]({destination})")).unwrap();
        let message = read_sources(&t.0).unwrap_err().to_string();
        assert!(message.contains("nested/view.nat") && message.contains(destination));
    }
}
fn alias_references(root: &Path) {
    init(root).unwrap(); fs::create_dir_all(root.join("spec/nested")).unwrap();
    fs::write(root.join("context.md"), "context").unwrap();
    fs::write(root.join("spec/app.nat"), "[ordinary](../context.md)").unwrap();
    fs::write(root.join("spec/nested/view.nat"), "![nested](../../context.md)").unwrap();
    let a = read_sources(root).unwrap(); let b = read_sources(&fs::canonicalize(root).unwrap()).unwrap();
    assert_eq!(input_hash(&a), input_hash(&b));
    for name in ["app.nat","nested/view.nat"] { assert_eq!(a[name].references["context.md"], hash(b"context")); }
    fs::write(root.join("spec/nested/view.nat"), format!("[absolute](<{}>)", root.join("context.md").display())).unwrap();
    assert!(read_sources(root).unwrap()["nested/view.nat"].references.contains_key("context.md"));
}
#[cfg(unix)]
#[test]
fn aliases_and_symlink_containment() {
    use std::os::unix::fs::symlink;
    let t = Scratch::new().unwrap(); let holder = Scratch::new().unwrap(); let alias = holder.0.join("alias");
    symlink(&t.0, &alias).unwrap(); alias_references(&alias);
    symlink(t.0.join("spec/app.nat"), t.0.join("spec/link.nat")).unwrap(); assert!(read_sources(&t.0).is_err());
    fs::remove_file(t.0.join("spec/link.nat")).unwrap();
    fs::write(holder.0.join("outside.md"), "outside").unwrap();
    symlink(&holder.0, t.0.join("outside-link")).unwrap();
    fs::write(t.0.join("spec/app.nat"), "[bad](../outside-link/outside.md)").unwrap();
    assert!(read_sources(&t.0).unwrap_err().to_string().contains("escapes"));
    fs::write(t.0.join("generated/private.txt"), "private").unwrap();
    symlink(t.0.join("generated"), t.0.join("generated-link")).unwrap();
    fs::write(t.0.join("spec/app.nat"), "[bad](../generated-link/private.txt)").unwrap();
    assert!(read_sources(&t.0).unwrap_err().to_string().contains("generated/"));
}
#[cfg(target_os = "macos")]
#[test]
fn macos_var_alias() {
    let t = Scratch(PathBuf::from("/var/tmp").join(format!("nat-alias-{}", unique())));
    fs::create_dir(&t.0).unwrap(); assert_ne!(t.0, fs::canonicalize(&t.0).unwrap()); alias_references(&t.0);
}
#[test]
fn inference_promotion_and_stale_refusal() {
    let t = fixture(); fs::write(t.0.join("spec/promoted.nat"), "Human text.\n").unwrap(); install(&t.0);
    let text = fs::read_to_string(t.0.join("inferred").join(INAT)).unwrap();
    assert!(text.contains("Source: spec/a.nat\n")); assert!(text.contains("No language specified.\n\nFull second paragraph."));
    promote(&t.0, "ASSUME-A").unwrap(); let m = current(&t.0).unwrap();
    assert_eq!(m.file_sources["a.txt"], strings(&["a.nat","promoted.nat"]));
    assert_eq!(m.file_sources["b.txt"], strings(&["b.nat"]));
    assert!(fs::read_to_string(t.0.join("spec/promoted.nat")).unwrap().starts_with("Human text."));
    assert!(semantic(&t.0).unwrap().requirements.iter().any(|r| r.id == "ASSUME-A" && r.source == "promoted.nat"));
    assert!(semantic(&t.0).unwrap().assumptions.is_empty()); build_using(&t.0, false, None, &mut never).unwrap();
    assert!(promote(&t.0, "missing").is_err()); fs::write(t.0.join("spec/a.nat"), "changed").unwrap();
    assert!(promote(&t.0, "ASSUME-A").is_err());
}
#[test]
fn parsing_debounce_and_dependency_errors() {
    let c = parse(strings(&["run","--project","project","--","--project","literal","a b","","$(false)"])).unwrap();
    assert_eq!(c.project, Some(PathBuf::from("project")));
    assert_eq!(c.positional, strings(&["--project","literal","a b","","$(false)"]));
    for args in [vec!["bad"],vec!["build","--project"],vec!["build","--project","--full"],vec!["watch","--full"],vec!["init","a","b"],vec!["promote"],vec!["test","--proposal","x"],vec!["help","extra"]] { assert!(parse(strings(&args)).is_err()); }
    let now = Instant::now(); let mut d = Debounce::new("initial".into());
    assert!(!d.observe("edit".into(), now)); assert!(!d.observe("edit".into(), now + Duration::from_millis(399)));
    assert!(!d.observe("error".into(), now + Duration::from_millis(400)));
    assert!(d.observe("error".into(), now + Duration::from_millis(800)));
    assert!(!d.observe("fixed".into(), now + Duration::from_millis(900)));
    assert!(d.observe("fixed".into(), now + Duration::from_millis(1300)));
    for text in ["ModuleNotFoundError: No module named 'tkinter'","dyld: Library not loaded: X","TclError: no display name and no $DISPLAY environment variable","error while loading shared libraries: x.so"] { assert!(dependency_diagnostic(text)); }
    assert!(!dependency_diagnostic("ValueError: invalid name"));
}
#[test]
fn runtime_repair_checks_and_retry_limit() {
    for succeeds in [false, true] {
        let t = fixture(); install(&t.0); let args = strings(&["a b","$(false)"]);
        let mut compiled = 0; let mut executed = 0;
        let result = run_using(&t.0, &args, &mut |r| {
            compiled += 1; assert_eq!(r["scope"], "full");
            assert!(r["runtime_failure_feedback"].as_str().unwrap().contains("missing")); Ok(full())
        }, &mut |_, command, extra| {
            executed += 1;
            if executed == 2 { assert_eq!(command, strings(&["true"])); assert!(extra.is_empty()); }
            else { assert_eq!(extra, args); }
            Ok(Execution { success: executed == 2 || (executed == 3 && succeeds), dependency_missing: true, diagnostic: "missing module".into() })
        });
        assert_eq!(result.is_ok(), succeeds); assert_eq!(compiled, 1); assert_eq!(executed, 3);
    }
    let t = fixture(); let mut m = install(&t.0);
    assert!(run_using(&t.0, &[], &mut never, &mut |_, _, _| Ok(Execution { success: false, dependency_missing: false, diagnostic: "ordinary error".into() })).is_err());
    let mut count = 0;
    assert!(run_using(&t.0, &[], &mut |_| Ok(full()), &mut |_, _, _| { count += 1; Ok(Execution { success: false, dependency_missing: count == 1, diagnostic: "failure".into() }) }).is_err());
    assert_eq!(count, 2);
    m.checks.clear(); assert!(checks_using(&t.0, &m, &mut execute).is_err());
    m.checks = vec![strings(&["false"]), strings(&["true"])]; let mut checks = 0;
    assert!(checks_using(&t.0, &m, &mut |_, _, _| { checks += 1; Ok(Execution { success: checks > 1, dependency_missing: false, diagnostic: "failed".into() }) }).is_err());
    assert_eq!(checks, 2);
}
