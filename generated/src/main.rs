use percent_encoding::percent_decode_str;
use pulldown_cmark::{Event, Parser, Tag};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Sources = BTreeMap<String, Source>;
const MANIFEST: &str = ".nat-manifest.json";
const IR: &str = "ir.json";
const INAT: &str = "assumptions.inat";
const HELP: &str = "nat — natural-language source compiler\n\nnat init [DIR]\nnat build [--project DIR] [--proposal JSON] [--full]\nnat watch [--project DIR] [--no-test] [--proposal JSON]\nnat run [--project DIR] [ARGS...]\nnat test [--project DIR]\nnat status [--project DIR]\nnat check [--project DIR]\nnat promote ID [--project DIR]\n\ninit creates a project and a starter greeting spec if needed.\nbuild validates and writes code; unchanged builds skip the compiler unless a proposal is explicitly selected.\n--full forces a whole-project compilation.\n--proposal JSON always loads and validates the saved response, even on a current build.\nProposal paths are relative to your current directory.\nwatch polls inputs, rebuilds after edits settle, and runs checks unless --no-test is set.\nrun and test build when needed; run forwards arguments without a shell.\nFor run, -- ends nat option parsing; remaining application arguments are forwarded.\nstatus reports freshness, drift, summary, and file counts.\ncheck succeeds only when all tracked inputs and artifacts match.\npromote appends a current assumption to spec/promoted.nat.\n--project DIR selects a project; otherwise search current directory and ancestors for spec/.\nhelp, --help, -h show this help; version, --version, -V show the package version.\n\nCodex compiles with read-only workspace access. Generated programs and checks execute\nwith your local account privileges. Review code and saved proposals before executing them.\nA build writes validated files but does not execute generated commands.\n";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Source { content: String, references: BTreeMap<String, String> }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Requirement { id: String, source: String, statement: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Assumption { id: String, source: String, statement: String, reason: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputFile { path: String, content: String, sources: Vec<String> }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Compilation {
    summary: String, requirements: Vec<Requirement>, assumptions: Vec<Assumption>,
    files: Vec<OutputFile>, run: Vec<String>, checks: Vec<Vec<String>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    full_rebuild_required: bool, reason: String, summary: String,
    requirements: Vec<Requirement>, assumptions: Vec<Assumption>,
    files: Vec<OutputFile>, remove_files: Vec<String>, run: Vec<String>, checks: Vec<Vec<String>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Semantic {
    #[serde(default)] version: u32,
    summary: String, requirements: Vec<Requirement>, assumptions: Vec<Assumption>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Manifest {
    version: u32, input_hash: String,
    #[serde(default)] source_hashes: BTreeMap<String, String>,
    generated: BTreeMap<String, String>,
    #[serde(default)] file_sources: BTreeMap<String, Vec<String>>,
    inferred: BTreeMap<String, String>, run: Vec<String>, checks: Vec<Vec<String>>, summary: String,
}
#[derive(Clone, Debug)]
struct Scope { changed: BTreeSet<String>, affected: BTreeSet<String> }

fn ensure(ok: bool, message: impl Into<String>) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into().into()) }
}
fn hash(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn pretty<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn relative(path: &str) -> Result<PathBuf> {
    ensure(!path.is_empty() && !path.contains(['\\', ':', '\0'])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|p| !p.is_empty() && p != "." && p != ".."),
        format!("unsafe relative path: {path:?}"))?;
    let p = PathBuf::from(path);
    ensure(p.components().all(|c| matches!(c, Component::Normal(_))), format!("unsafe path: {path}"))?;
    Ok(p)
}
fn no_symlinks(root: &Path, target: &Path) -> Result<()> {
    let mut path = root.to_path_buf();
    for component in target.strip_prefix(root)?.components() {
        ensure(matches!(component, Component::Normal(_)), "non-normal filesystem path")?;
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(m) => ensure(!m.file_type().is_symlink(), format!("symlink is not allowed: {}", path.display()))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {},
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn check_parents(root: &Path, target: &Path) -> Result<()> {
    no_symlinks(root, target)?;
    let mut p = target.parent();
    while let Some(parent) = p {
        if parent == root { break; }
        if parent.exists() { ensure(parent.is_dir(), format!("parent is a file: {}", parent.display()))?; }
        p = parent.parent();
    }
    Ok(())
}
fn collect_specs(root: &Path, dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    no_symlinks(root, dir)?;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        let kind = entry.file_type()?;
        ensure(!kind.is_symlink(), format!("symlink within spec/: {}", p.display()))?;
        if kind.is_dir() { collect_specs(root, &p, paths)?; }
        else if kind.is_file() && p.extension().is_some_and(|x| x == "nat") { paths.push(p); }
    }
    Ok(())
}
fn local_destination(destination: &str) -> bool {
    if destination.is_empty() || destination.starts_with('#') || destination.starts_with("//") { return false; }
    if let Some((scheme, _)) = destination.split_once(':') {
        if scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) { return false; }
    }
    true
}
fn lexical(path: &Path) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => { ensure(out.pop(), "reference escapes filesystem root")?; },
            Component::CurDir => {},
            _ => out.push(c.as_os_str()),
        }
    }
    Ok(out)
}
fn forbidden_reference(path: &Path) -> bool {
    matches!(path.components().next(), Some(Component::Normal(p)) if p == "generated" || p == "inferred")
}
fn read_source(root: &Path, name: &str, content: String) -> Result<Source> {
    let root = fs::canonicalize(root)?;
    let containing = root.join("spec").join(relative(name)?);
    let mut references = BTreeMap::new();
    for event in Parser::new(&content) {
        let destination = match event {
            Event::Start(Tag::Link { dest_url, .. }) | Event::Start(Tag::Image { dest_url, .. }) => dest_url,
            _ => continue,
        };
        if !local_destination(&destination) { continue; }
        let resolve = || -> Result<(String, String)> {
            let raw = destination.split(['#', '?']).next().unwrap_or("");
            let decoded = percent_decode_str(raw).decode_utf8()?;
            ensure(!decoded.contains('\0') && !decoded.contains('\\'), "invalid reference path")?;
            let target = containing.parent().unwrap().join(decoded.as_ref());
            let canonical = fs::canonicalize(&target)?;
            let canonical_local = canonical.strip_prefix(&root).map_err(|_| "reference escapes project")?;
            ensure(!forbidden_reference(canonical_local), "reference resolves into generated/ or inferred/")?;
            ensure(canonical.is_file(), "reference must name a file")?;
            let normalized = lexical(&target)?;
            let local = normalized.strip_prefix(&root).unwrap_or(canonical_local);
            ensure(!forbidden_reference(local), "reference points into generated/ or inferred/")?;
            let key = local.to_str().ok_or("reference path is not UTF-8")?.to_owned();
            relative(&key)?;
            Ok((key, hash(&fs::read(&canonical)?)))
        };
        let (key, digest) = resolve().map_err(|e| format!("spec/{name}: reference {destination:?}: {e}"))?;
        references.insert(key, digest);
    }
    Ok(Source { content, references })
}
fn read_sources(root: &Path) -> Result<Sources> {
    let root = fs::canonicalize(root)?;
    let mut paths = Vec::new();
    collect_specs(&root, &root.join("spec"), &mut paths)?;
    paths.sort();
    ensure(!paths.is_empty(), "spec/ must contain at least one .nat file")?;
    let mut sources = Sources::new();
    for path in paths {
        let name = path.strip_prefix(root.join("spec"))?.to_str().ok_or("spec path is not UTF-8")?.to_owned();
        let content = fs::read_to_string(&path).map_err(|e| format!("spec/{name}: {e}"))?;
        sources.insert(name.clone(), read_source(&root, &name, content)?);
    }
    Ok(sources)
}
fn source_hashes(sources: &Sources) -> BTreeMap<String, String> {
    sources.iter().map(|(name, source)| {
        let digest = if source.references.is_empty() { hash(source.content.as_bytes()) }
            else { hash(&serde_json::to_vec(source).expect("source serialization")) };
        (name.clone(), digest)
    }).collect()
}
fn input_hash(sources: &Sources) -> String {
    if sources.values().all(|s| s.references.is_empty()) {
        let text: BTreeMap<_, _> = sources.iter().map(|(k, v)| (k, &v.content)).collect();
        hash(&serde_json::to_vec(&text).expect("source serialization"))
    } else { hash(&serde_json::to_vec(&source_hashes(sources)).expect("hash serialization")) }
}
fn argv_valid(argv: &[String], allow_empty: bool) -> Result<()> {
    ensure(allow_empty || !argv.is_empty(), "empty check command")?;
    ensure(argv.iter().all(|s| !s.is_empty() && !s.contains('\0')), "command contains an empty argument or NUL")
}
fn load_manifest(root: &Path) -> Result<Option<Manifest>> {
    let path = root.join("generated").join(MANIFEST);
    no_symlinks(root, &path)?;
    if !path.exists() { return Ok(None); }
    let m: Manifest = serde_json::from_slice(&fs::read(path)?)?;
    ensure(m.version == 1 || m.version == 2, format!("unsupported manifest version {}", m.version))?;
    argv_valid(&m.run, true)?;
    for c in &m.checks { argv_valid(c, false)?; }
    for p in m.generated.keys() { relative(p)?; ensure(p != MANIFEST, "manifest cannot track itself")?; }
    for p in m.inferred.keys() { relative(p)?; }
    Ok(Some(m))
}
fn verify_artifacts(root: &Path, m: &Manifest) -> Result<()> {
    for (directory, files) in [("generated", &m.generated), ("inferred", &m.inferred)] {
        for (name, digest) in files {
            let path = root.join(directory).join(relative(name)?);
            no_symlinks(root, &path)?;
            let bytes = fs::read(&path).map_err(|e| format!("missing or unreadable tracked artifact {}: {e}", path.display()))?;
            ensure(hash(&bytes) == *digest, format!("edited tracked artifact: {}. Move the change into spec/ or restore the recorded file.", path.display()))?;
        }
    }
    Ok(())
}
fn current(root: &Path) -> Result<Manifest> {
    let m = load_manifest(root)?.ok_or("no build; run nat build")?;
    verify_artifacts(root, &m)?;
    ensure(input_hash(&read_sources(root)?) == m.input_hash, "stale inputs; run nat build")?;
    Ok(m)
}
fn semantic(root: &Path) -> Result<Semantic> {
    Ok(serde_json::from_slice(&fs::read(root.join("generated").join(IR))?)?)
}
fn validate(c: &Compilation, sources: &Sources) -> Result<()> {
    ensure(!c.requirements.is_empty(), "no mapped requirements")?;
    ensure(!c.files.is_empty(), "no generated files")?;
    let mut ids = BTreeSet::new();
    for (kind, id, source, statement) in c.requirements.iter().map(|r| ("requirement", &r.id, &r.source, &r.statement))
        .chain(c.assumptions.iter().map(|a| ("assumption", &a.id, &a.source, &a.statement))) {
        let valid_id = !id.is_empty() && id.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'));
        ensure(valid_id && sources.contains_key(source), format!("invalid {kind} {id} or source: {source:?}"))?;
        ensure(ids.insert(id), format!("duplicate {kind} ID: {id}"))?;
        ensure(!statement.trim().is_empty(), format!("empty {kind} statement: {id}"))?;
    }
    for a in &c.assumptions { ensure(!a.reason.trim().is_empty(), format!("empty assumption reason: {}", a.id))?; }
    let mut paths = BTreeSet::from([IR.to_string(), MANIFEST.to_string()]);
    for file in &c.files {
        relative(&file.path)?;
        ensure(paths.insert(file.path.clone()), format!("duplicate or reserved output: {}", file.path))?;
        ensure(!file.sources.is_empty(), format!("missing dependencies: {}", file.path))?;
        let mut deps = BTreeSet::new();
        for source in &file.sources {
            ensure(sources.contains_key(source), format!("unknown output source: {source}"))?;
            ensure(deps.insert(source), format!("duplicate dependency: {source}"))?;
        }
    }
    for path in &paths {
        let mut parent = Path::new(path).parent();
        while let Some(p) = parent {
            if p.as_os_str().is_empty() { break; }
            ensure(!paths.contains(p.to_str().ok_or("non-UTF-8 output path")?), format!("output parent collision: {path}"))?;
            parent = p.parent();
        }
    }
    argv_valid(&c.run, true)?;
    for check in &c.checks { argv_valid(check, false)?; }
    Ok(())
}
fn decode_full(mut response: Value, sources: &Sources) -> Result<Compilation> {
    let dependencies = json!(sources.keys().collect::<Vec<_>>());
    if let Some(files) = response.get_mut("files").and_then(Value::as_array_mut) {
        for file in files {
            if let Some(object) = file.as_object_mut() {
                if !object.contains_key("sources") { object.insert("sources".into(), dependencies.clone()); }
            }
        }
    }
    Ok(serde_json::from_value(response)?)
}
fn scope_for(m: &Manifest, sources: &Sources) -> Option<Scope> {
    let hashes = source_hashes(sources);
    if m.version != 2 || m.source_hashes.keys().ne(hashes.keys()) { return None; }
    let outputs: BTreeSet<_> = m.generated.keys().filter(|p| p.as_str() != IR).cloned().collect();
    if outputs != m.file_sources.keys().cloned().collect() { return None; }
    if m.file_sources.values().any(|deps| deps.is_empty() || deps.iter().any(|s| !sources.contains_key(s))) { return None; }
    let changed: BTreeSet<_> = hashes.iter().filter(|(p, h)| m.source_hashes.get(*p) != Some(*h)).map(|(p, _)| p.clone()).collect();
    let affected: BTreeSet<_> = m.file_sources.iter().filter(|(_, deps)| deps.iter().any(|s| changed.contains(s))).map(|(p, _)| p.clone()).collect();
    if changed.is_empty() || affected.is_empty() || affected.len() == outputs.len() { return None; }
    Some(Scope { changed, affected })
}
fn restore_compilation(root: &Path, m: &Manifest) -> Result<Compilation> {
    let ir = semantic(root)?;
    let all_sources: Vec<_> = read_sources(root)?.keys().cloned().collect();
    let mut files = Vec::new();
    for path in m.generated.keys().filter(|p| p.as_str() != IR) {
        files.push(OutputFile { path: path.clone(), content: fs::read_to_string(root.join("generated").join(path))?,
            sources: m.file_sources.get(path).filter(|v| !v.is_empty()).cloned().unwrap_or_else(|| all_sources.clone()) });
    }
    Ok(Compilation { summary: ir.summary, requirements: ir.requirements, assumptions: ir.assumptions,
        files, run: m.run.clone(), checks: m.checks.clone() })
}
fn merge_patch(mut base: Compilation, patch: Patch, scope: &Scope) -> Result<Compilation> {
    ensure(!patch.full_rebuild_required, format!("compiler requested full rebuild: {}", patch.reason))?;
    for source in patch.requirements.iter().map(|r| &r.source).chain(patch.assumptions.iter().map(|a| &a.source)) {
        ensure(scope.changed.contains(source), format!("scoped semantic change to unchanged source: {source}"))?;
    }
    let old_paths: BTreeSet<_> = base.files.iter().map(|f| f.path.clone()).collect();
    let mut replaced = BTreeSet::new();
    for f in &patch.files {
        ensure(!old_paths.contains(&f.path) || scope.affected.contains(&f.path), format!("scoped change to unaffected output: {}", f.path))?;
        ensure(replaced.insert(f.path.clone()), format!("duplicate patch output: {}", f.path))?;
    }
    for path in &patch.remove_files {
        ensure(scope.affected.contains(path), format!("cannot remove unaffected output: {path}"))?;
        ensure(replaced.insert(path.clone()), format!("duplicate replacement/removal: {path}"))?;
    }
    ensure(scope.affected.iter().all(|p| replaced.contains(p)), "scoped result did not replace or remove every affected output")?;
    base.files.retain(|f| !scope.affected.contains(&f.path));
    base.files.extend(patch.files);
    base.requirements.retain(|r| !scope.changed.contains(&r.source));
    base.requirements.extend(patch.requirements);
    base.assumptions.retain(|a| !scope.changed.contains(&a.source));
    base.assumptions.extend(patch.assumptions);
    base.summary = patch.summary;
    base.run = patch.run;
    base.checks = patch.checks;
    Ok(base)
}
fn request(root: &Path, sources: &Sources, old: Option<&Manifest>, scope: Option<&Scope>, feedback: Option<&str>) -> Result<Value> {
    let mut human = sources.clone();
    let mut previous = if old.is_some() { Some(semantic(root)?) } else { None };
    let mut affected = Vec::new();
    let mut reserved = Vec::new();
    if let Some(scope) = scope {
        human.retain(|p, _| scope.changed.contains(p));
        if let Some(ir) = previous.as_mut() {
            reserved.extend(ir.requirements.iter().filter(|r| !scope.changed.contains(&r.source)).map(|r| r.id.clone()));
            reserved.extend(ir.assumptions.iter().filter(|a| !scope.changed.contains(&a.source)).map(|a| a.id.clone()));
            ir.requirements.retain(|r| scope.changed.contains(&r.source));
            ir.assumptions.retain(|a| scope.changed.contains(&a.source));
        }
        for path in &scope.affected {
            affected.push(json!({"path": path, "content": fs::read_to_string(root.join("generated").join(path))?}));
        }
    }
    let prior_inference = if scope.is_none() && old.is_some() {
        fs::read_to_string(root.join("inferred").join(INAT))?
    } else { String::new() };
    Ok(json!({
        "scope": if scope.is_some() { "scoped" } else { "full" },
        "human_sources": human, "source_paths": sources.keys().collect::<Vec<_>>(),
        "previous_semantic_record": previous, "previous_inferences": prior_inference,
        "affected_outputs": affected, "previous_dependencies": old.map(|m| &m.file_sources),
        "previous_run": old.map(|m| &m.run), "previous_checks": old.map(|m| &m.checks),
        "reserved_ids": reserved, "runtime_failure_feedback": feedback,
    }))
}
fn preflight(root: &Path, c: &Compilation, old: Option<&Manifest>) -> Result<()> {
    if let Some(m) = old { verify_artifacts(root, m)?; }
    for (dir, names) in [
        ("generated", c.files.iter().map(|f| f.path.as_str()).chain([IR, MANIFEST]).collect::<Vec<_>>()),
        ("inferred", vec![INAT]),
    ] {
        for name in names {
            let path = root.join(dir).join(relative(name)?);
            check_parents(root, &path)?;
            if path.exists() {
                let tracked = old.is_some_and(|m| if dir == "generated" { name == MANIFEST || m.generated.contains_key(name) } else { m.inferred.contains_key(name) });
                ensure(tracked, format!("refusing to overwrite untracked file: {}", path.display()))?;
                ensure(path.is_file(), format!("output is not a file: {}", path.display()))?;
            }
        }
    }
    Ok(())
}
static SERIAL: AtomicU64 = AtomicU64::new(0);
fn unique() -> String {
    format!("{}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos(), SERIAL.fetch_add(1, Ordering::Relaxed))
}
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Result<Self> {
        let path = env::temp_dir().join(format!("nat-{}", unique()));
        fs::create_dir(&path)?;
        Ok(Self(fs::canonicalize(path)?))
    }
}
impl Drop for Scratch { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
fn write_changed(path: &Path, bytes: &[u8]) -> Result<()> {
    if fs::read(path).ok().as_deref() == Some(bytes) { return Ok(()); }
    let parent = path.parent().ok_or("file has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".nat-write-{}", unique()));
    let operation = || -> Result<()> {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    };
    let result = operation();
    if result.is_err() { let _ = fs::remove_file(&temporary); }
    result
}
fn render_inference(assumptions: &[Assumption]) -> String {
    let mut text = String::from("# Inferred assumptions\n\nReviewable compiler inference; spec/ remains authoritative.\n");
    if assumptions.is_empty() { text.push_str("\nNo inferred product assumptions.\n"); }
    for a in assumptions {
        text.push_str(&format!("\n## {}\n\nSource: spec/{}\n\n{}\n\nReason: {}\n", a.id, a.source, a.statement, a.reason));
    }
    text
}
fn persist(root: &Path, sources: &Sources, c: &Compilation, old: Option<&Manifest>) -> Result<Manifest> {
    let ir = Semantic { version: 1, summary: c.summary.clone(), requirements: c.requirements.clone(), assumptions: c.assumptions.clone() };
    let mut bytes: BTreeMap<String, Vec<u8>> = c.files.iter().map(|f| (f.path.clone(), f.content.as_bytes().to_vec())).collect();
    bytes.insert(IR.into(), pretty(&ir)?);
    let inference = render_inference(&c.assumptions);
    let m = Manifest {
        version: 2, input_hash: input_hash(sources), source_hashes: source_hashes(sources),
        generated: bytes.iter().map(|(p, b)| (p.clone(), hash(b))).collect(),
        file_sources: c.files.iter().map(|f| (f.path.clone(), f.sources.clone())).collect(),
        inferred: BTreeMap::from([(INAT.into(), hash(inference.as_bytes()))]),
        run: c.run.clone(), checks: c.checks.clone(), summary: c.summary.clone(),
    };
    let manifest_bytes = pretty(&m)?;
    for (path, content) in &bytes { write_changed(&root.join("generated").join(path), content)?; }
    write_changed(&root.join("inferred").join(INAT), inference.as_bytes())?;
    if let Some(old) = old {
        for path in old.generated.keys().filter(|p| !m.generated.contains_key(*p)) { fs::remove_file(root.join("generated").join(path))?; }
        for path in old.inferred.keys().filter(|p| !m.inferred.contains_key(*p)) { fs::remove_file(root.join("inferred").join(path))?; }
    }
    write_changed(&root.join("generated").join(MANIFEST), &manifest_bytes)?;
    Ok(m)
}
fn build_using<F>(root: &Path, force: bool, feedback: Option<&str>, compiler: &mut F) -> Result<Manifest>
where F: FnMut(&Value) -> Result<Value> {
    build_with_policy(root, force, feedback, false, compiler)
}
fn build_with_policy<F>(root: &Path, force: bool, feedback: Option<&str>, explicit_proposal: bool, compiler: &mut F) -> Result<Manifest>
where F: FnMut(&Value) -> Result<Value> {
    let sources = read_sources(root)?;
    let old = load_manifest(root)?;
    if let Some(m) = &old {
        verify_artifacts(root, m)?;
        if !force && !explicit_proposal && feedback.is_none() && m.input_hash == input_hash(&sources) {
            println!("Current: {}", m.summary);
            return Ok(m.clone());
        }
    }
    let manifest_before = fs::read(root.join("generated").join(MANIFEST)).ok();
    let scope = if force || feedback.is_some() { None } else { old.as_ref().and_then(|m| scope_for(m, &sources)) };
    let mut candidate = None;
    if let (Some(scope), Some(m)) = (scope.as_ref(), old.as_ref()) {
        let attempt = (|| -> Result<Compilation> {
            let response = compiler(&request(root, &sources, old.as_ref(), Some(scope), feedback)?)?;
            let patch: Patch = serde_json::from_value(response)?;
            let c = merge_patch(restore_compilation(root, m)?, patch, scope)?;
            validate(&c, &sources)?;
            preflight(root, &c, old.as_ref())?;
            Ok(c)
        })();
        match attempt {
            Ok(c) => candidate = Some(c),
            Err(e) => eprintln!("nat: scoped response requires full compilation: {e}"),
        }
    }
    let c = if let Some(c) = candidate { c } else {
        let response = compiler(&request(root, &sources, old.as_ref(), None, feedback)?)?;
        decode_full(response, &sources)?
    };
    validate(&c, &sources)?;
    preflight(root, &c, old.as_ref())?;
    ensure(input_hash(&read_sources(root)?) == input_hash(&sources), "inputs changed during compilation; retry build")?;
    ensure(fs::read(root.join("generated").join(MANIFEST)).ok() == manifest_before, "manifest changed during compilation; retry build")?;
    let m = persist(root, &sources, &c, old.as_ref())?;
    println!("Built: {} ({} files)", m.summary, c.files.len());
    Ok(m)
}

mod compiler;

fn saved_response(path: &Path) -> Result<Value> { Ok(serde_json::from_slice(&fs::read(path)?)?) }
fn build(root: &Path, proposal: Option<&Path>, force: bool) -> Result<Manifest> {
    let mut compile = |request: &Value| -> Result<Value> {
        if let Some(path) = proposal {
            eprintln!("nat: loading explicitly selected proposal {} ({})", path.display(), request["scope"]);
            saved_response(path)
        } else { compiler::invoke(root, request) }
    };
    build_with_policy(root, force, None, proposal.is_some(), &mut compile)
}
fn tee<R: Read + Send + 'static>(mut reader: R, stderr: bool) -> thread::JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut captured = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let n = reader.read(&mut buffer)?;
            if n == 0 { break; }
            if stderr { let mut out = io::stderr().lock(); let _ = out.write_all(&buffer[..n]); let _ = out.flush(); }
            else { let mut out = io::stdout().lock(); let _ = out.write_all(&buffer[..n]); let _ = out.flush(); }
            captured.extend_from_slice(&buffer[..n]);
            if captured.len() > 262144 { captured.drain(..captured.len() - 262144); }
        }
        Ok(captured)
    })
}
fn joined(handle: thread::JoinHandle<io::Result<Vec<u8>>>) -> Result<Vec<u8>> {
    Ok(handle.join().map_err(|_| "output reader panicked")??)
}
#[derive(Debug)]
struct Execution { success: bool, dependency_missing: bool, diagnostic: String }
fn dependency_diagnostic(message: &str) -> bool {
    let lower = message.to_lowercase();
    ["modulenotfounderror:", "importerror: no module named", "no module named ",
        "cannot find module '", "err_module_not_found", "error while loading shared libraries:",
        "library not loaded:", "cannot open shared object file", "no display name and no $display",
        "couldn't connect to display", "could not connect to display", "cannot open display:",
        "no x11 display variable", "cannot load library", "qt platform plugin could not be initialized",
        "could not load the qt platform plugin", "bad interpreter: no such file"].iter().any(|p| lower.contains(p))
}
fn execute(root: &Path, command: &[String], extra: &[String]) -> Result<Execution> {
    argv_valid(command, false)?;
    let child = Command::new(&command[0]).args(&command[1..]).args(extra)
        .current_dir(root.join("generated")).stdin(Stdio::inherit()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Ok(Execution { success: false, dependency_missing: e.kind() == io::ErrorKind::NotFound,
            diagnostic: format!("cannot execute {command:?}: {e}") }),
    };
    let stdout = tee(child.stdout.take().unwrap(), false);
    let stderr = tee(child.stderr.take().unwrap(), true);
    let status = child.wait()?;
    let mut output = joined(stdout)?;
    output.extend(joined(stderr)?);
    let message = String::from_utf8_lossy(&output);
    Ok(Execution { success: status.success(), dependency_missing: !status.success() && dependency_diagnostic(&message),
        diagnostic: format!("command {command:?}, arguments {extra:?}, status {status}\n{message}") })
}
fn checks_using<E>(root: &Path, m: &Manifest, executor: &mut E) -> Result<()>
where E: FnMut(&Path, &[String], &[String]) -> Result<Execution> {
    ensure(!m.checks.is_empty(), "no generated checks")?;
    let mut failures = Vec::new();
    for command in &m.checks {
        match executor(root, command, &[]) {
            Ok(out) if out.success => {},
            Ok(out) => failures.push(out.diagnostic),
            Err(e) => failures.push(e.to_string()),
        }
    }
    ensure(failures.is_empty(), format!("generated checks failed:\n{}", failures.join("\n")))
}
fn run_using<F, E>(root: &Path, extra: &[String], compiler: &mut F, executor: &mut E) -> Result<()>
where F: FnMut(&Value) -> Result<Value>, E: FnMut(&Path, &[String], &[String]) -> Result<Execution> {
    let m = build_using(root, false, None, compiler)?;
    ensure(!m.run.is_empty(), "project is not runnable (empty run command)")?;
    let first = executor(root, &m.run, extra)?;
    if first.success { return Ok(()); }
    if !first.dependency_missing { return Err(first.diagnostic.into()); }
    eprintln!("nat: runtime dependency unavailable; recompiling for this machine (one full rebuild)\n{}", first.diagnostic);
    let repaired = build_using(root, true, Some(&first.diagnostic), compiler)?;
    checks_using(root, &repaired, executor)?;
    ensure(!repaired.run.is_empty(), "recompiled project is not runnable")?;
    let second = executor(root, &repaired.run, extra)?;
    ensure(second.success, format!("run failed after one repair: {}", second.diagnostic))
}
fn promote(root: &Path, id: &str) -> Result<()> {
    let old = current(root)?;
    let mut sources = read_sources(root)?;
    let mut c = restore_compilation(root, &old)?;
    let index = c.assumptions.iter().position(|a| a.id == id).ok_or_else(|| format!("unknown assumption ID: {id}"))?;
    let a = c.assumptions.remove(index);
    let path = root.join("spec/promoted.nat");
    check_parents(root, &path)?;
    let mut content = sources.get("promoted.nat").map(|s| s.content.clone()).unwrap_or_default();
    content.push_str(&format!("\n\n# Promoted from {}\n\n{}\n", a.id, a.statement));
    let source = read_source(root, "promoted.nat", content.clone())?;
    sources.insert("promoted.nat".into(), source);
    for f in &mut c.files {
        if f.sources.contains(&a.source) && !f.sources.iter().any(|s| s == "promoted.nat") {
            f.sources.push("promoted.nat".into());
            f.sources.sort();
        }
    }
    c.requirements.push(Requirement { id: a.id, source: "promoted.nat".into(), statement: a.statement });
    validate(&c, &sources)?;
    preflight(root, &c, Some(&old))?;
    current(root)?;
    write_changed(&path, content.as_bytes())?;
    persist(root, &sources, &c, Some(&old))?;
    println!("Promoted {id} to spec/promoted.nat");
    Ok(())
}
struct Debounce { key: String, changed: Option<Instant> }
impl Debounce {
    fn new(key: String) -> Self { Self { key, changed: None } }
    fn observe(&mut self, key: String, now: Instant) -> bool {
        if key != self.key { self.key = key; self.changed = Some(now); return false; }
        if self.changed.is_some_and(|t| now.duration_since(t) >= Duration::from_millis(400)) {
            self.changed = None;
            return true;
        }
        false
    }
}
fn fingerprint(root: &Path) -> String {
    match read_sources(root) { Ok(s) => input_hash(&s), Err(e) => format!("error:{e}") }
}
fn watch(root: &Path, proposal: Option<&Path>, test: bool) -> Result<()> {
    let mut debounce = Debounce::new(fingerprint(root));
    let cycle = || -> Result<()> {
        let m = build(root, proposal, false)?;
        if test { checks_using(root, &m, &mut execute)?; }
        Ok(())
    };
    if let Err(e) = cycle() { eprintln!("nat watch: {e}"); }
    eprintln!("nat: watching {} (150 ms polling, 400 ms settle)", root.display());
    loop {
        thread::sleep(Duration::from_millis(150));
        if debounce.observe(fingerprint(root), Instant::now()) {
            if let Err(e) = cycle() { eprintln!("nat watch: {e}"); }
        }
    }
}
fn init(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    let root = fs::canonicalize(path)?;
    for dir in ["spec", "inferred", "generated"] {
        no_symlinks(&root, &root.join(dir))?;
        fs::create_dir_all(root.join(dir))?;
    }
    let mut specs = Vec::new();
    collect_specs(&root, &root.join("spec"), &mut specs)?;
    if specs.is_empty() {
        write_changed(&root.join("spec/app.nat"), b"# Greeting\n\nBuild a command-line program that greets a name supplied as an argument, defaulting to the world. Print the greeting to stdout.\n")?;
    }
    println!("Project: {}\nNext: nat run --project {}", root.display(), root.display());
    Ok(())
}
fn resolve_project(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        let root = fs::canonicalize(p).map_err(|e| format!("project {}: {e}", p.display()))?;
        ensure(root.join("spec").is_dir(), format!("project {} has no spec/ directory", root.display()))?;
        no_symlinks(&root, &root.join("spec"))?;
        return Ok(root);
    }
    let start = env::current_dir()?;
    for p in start.ancestors() {
        if p.join("spec").is_dir() {
            let root = fs::canonicalize(p)?;
            no_symlinks(&root, &root.join("spec"))?;
            return Ok(root);
        }
    }
    Err("no project found; run nat init or use --project DIR".into())
}
#[derive(Debug)]
struct Cli { command: String, project: Option<PathBuf>, proposal: Option<PathBuf>, full: bool, no_test: bool, positional: Vec<String> }
fn parse(args: Vec<String>) -> Result<Cli> {
    let command = args.first().cloned().unwrap_or_else(|| "help".into());
    let command = match command.as_str() { "--help" | "-h" => "help".into(), "--version" | "-V" => "version".into(), _ => command };
    ensure(["help", "version", "init", "build", "run", "test", "status", "check", "watch", "promote"].contains(&command.as_str()), format!("unknown command {command}; use nat help"))?;
    let mut cli = Cli { command, project: None, proposal: None, full: false, no_test: false, positional: Vec::new() };
    let mut i = 1;
    let mut flags = BTreeSet::new();
    while i < args.len() {
        let arg = &args[i];
        if cli.command == "run" && arg == "--" { cli.positional.extend_from_slice(&args[i + 1..]); break; }
        if cli.command == "run" && arg != "--project" { cli.positional.extend_from_slice(&args[i..]); break; }
        if arg == "--project" && !["init", "help", "version"].contains(&cli.command.as_str()) {
            ensure(flags.insert(arg.clone()), "duplicate --project")?;
            i += 1;
            let value = args.get(i).filter(|s| !s.is_empty() && !s.starts_with('-')).ok_or("missing value for --project")?;
            cli.project = Some(PathBuf::from(value));
        } else if arg == "--proposal" && ["build", "watch"].contains(&cli.command.as_str()) {
            ensure(flags.insert(arg.clone()), "duplicate --proposal")?;
            i += 1;
            let value = args.get(i).filter(|s| !s.is_empty() && !s.starts_with('-')).ok_or("missing value for --proposal")?;
            cli.proposal = Some(PathBuf::from(value));
        } else if arg == "--full" && cli.command == "build" {
            ensure(!cli.full, "duplicate --full")?; cli.full = true;
        } else if arg == "--no-test" && cli.command == "watch" {
            ensure(!cli.no_test, "duplicate --no-test")?; cli.no_test = true;
        } else if ["init", "promote"].contains(&cli.command.as_str()) && !arg.starts_with('-') && !arg.is_empty() {
            ensure(cli.positional.is_empty(), format!("excess argument: {arg}"))?;
            cli.positional.push(arg.clone());
        } else { return Err(format!("invalid {} option or argument: {arg}; use nat help", cli.command).into()); }
        i += 1;
    }
    ensure(cli.command != "promote" || cli.positional.len() == 1, "promote requires an assumption ID")?;
    Ok(cli)
}
fn dispatch(args: Vec<String>) -> Result<()> {
    let cli = parse(args)?;
    match cli.command.as_str() {
        "help" => { print!("{HELP}"); return Ok(()); },
        "version" => { println!("nat {}", env!("CARGO_PKG_VERSION")); return Ok(()); },
        "init" => return init(Path::new(cli.positional.first().map(String::as_str).unwrap_or("."))),
        _ => {},
    }
    let root = resolve_project(cli.project.as_deref())?;
    match cli.command.as_str() {
        "build" => { build(&root, cli.proposal.as_deref(), cli.full)?; },
        "check" => { current(&root)?; println!("Current: all tracked inputs and artifacts match"); },
        "status" => {
            if let Some(m) = load_manifest(&root)? {
                println!("Previous: {}\nFiles: {} source, {} generated, {} inferred", m.summary, m.source_hashes.len(), m.generated.len(), m.inferred.len());
                if let Err(e) = verify_artifacts(&root, &m) { println!("Artifact drift: {e}"); }
                match read_sources(&root) {
                    Ok(s) if input_hash(&s) == m.input_hash => {
                        if verify_artifacts(&root, &m).is_ok() { println!("Current build"); }
                    },
                    Ok(_) => println!("Stale inputs"),
                    Err(e) => println!("Stale or inaccessible inputs: {e}"),
                }
            } else { println!("No build"); }
        },
        "test" => { let m = build(&root, None, false)?; checks_using(&root, &m, &mut execute)?; },
        "run" => { run_using(&root, &cli.positional, &mut |r| compiler::invoke(&root, r), &mut execute)?; },
        "watch" => return watch(&root, cli.proposal.as_deref(), !cli.no_test),
        "promote" => promote(&root, &cli.positional[0])?,
        _ => unreachable!(),
    }
    Ok(())
}
fn main() {
    if let Err(e) = dispatch(env::args().skip(1).collect()) { eprintln!("nat: {e}"); std::process::exit(1); }
}
#[cfg(test)]
mod tests;
