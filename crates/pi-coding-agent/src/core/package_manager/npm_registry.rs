//! Built-in npm registry client, used when no `npm` executable is available.
//!
//! TS pi always shells out to `npm` (or the configured `npmCommand`). The Rust binary does not
//! require Node.js, so when `npm` is missing and no `npmCommand` is configured, the package
//! manager installs npm packages itself with this module. It writes the same on-disk layout npm
//! does (`<root>/package.json` dependencies and a hoisted `<root>/node_modules` tree), so TS pi
//! and real npm keep working on the same directories:
//!
//! - registry and auth configuration from `.npmrc` files (install root, cwd, user) and the
//!   `npm_config_registry` environment variable (`registry`, `@scope:registry`,
//!   `//host/path/:_authToken`, `//host/path/:_auth`, `${ENV}` expansion);
//! - packuments are fetched in the abbreviated ("corgi") format; versions are picked like
//!   `npm-pick-manifest` (dist-tag, exact version, or the `latest` tag when it satisfies the range,
//!   else the highest satisfying non-deprecated version);
//! - tarballs are verified against `dist.integrity` (sha512/sha384/sha256/sha1 SRI) or
//!   `dist.shasum` and extracted with the first path component stripped (`package/`), skipping
//!   links and unsafe paths;
//! - `dependencies` and `optionalDependencies` are installed recursively with npm's hoisting
//!   (place at the shallowest level without a conflict, nest otherwise); peer dependencies are
//!   not installed (`--legacy-peer-deps`), dev dependencies are omitted; optional dependencies
//!   that fail or do not match `os`/`cpu` are skipped; `npm:` aliases and tarball URLs work;
//! - `node_modules/.bin` links are created on Unix;
//! - lifecycle scripts (`preinstall`/`install`/`postinstall`) are NOT run: there is no Node.js
//!   to run them. Packages that need them must be installed with npm.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use base64::Engine;
use serde_json::{Map, Value};

use super::node_semver;

const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org/";
const CORGI_ACCEPT: &str =
    "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*";
const FETCH_RETRIES: usize = 2;
const DOWNLOAD_CONCURRENCY: usize = 8;

#[derive(Debug, Clone)]
enum AuthEntry {
    Token(String),
    Basic(String),
}

/// Registry configuration read from `.npmrc` files and the environment.
#[derive(Debug, Clone)]
pub struct NpmrcConfig {
    registry: String,
    scoped: HashMap<String, String>,
    auth: Vec<(String, AuthEntry)>,
}

fn expand_env(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let name = &after[..end];
                out.push_str(&std::env::var(name).unwrap_or_default());
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn with_trailing_slash(url: &str) -> String {
    if url.ends_with('/') {
        url.to_string()
    } else {
        format!("{url}/")
    }
}

/// npm's "nerf dart" of a URL: `//host[:port]/path/` (without protocol, query or filename).
fn nerf_dart(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
    let path = parsed.path();
    let dir = match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "/",
    };
    Some(format!("//{host}{port}{dir}"))
}

impl NpmrcConfig {
    /// Load `.npmrc` files (lowest precedence first) and environment overrides.
    pub fn load(dirs: &[PathBuf]) -> Self {
        let mut config = NpmrcConfig {
            registry: DEFAULT_REGISTRY.to_string(),
            scoped: HashMap::new(),
            auth: Vec::new(),
        };
        let mut files = Vec::new();
        if let Ok(userconfig) =
            std::env::var("npm_config_userconfig").or_else(|_| std::env::var("NPM_CONFIG_USERCONFIG"))
        {
            files.push(PathBuf::from(userconfig));
        } else if let Some(home) = std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
        {
            files.push(home.join(".npmrc"));
        }
        for dir in dirs {
            files.push(dir.join(".npmrc"));
        }
        let mut seen = HashSet::new();
        for file in files {
            if !seen.insert(file.clone()) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&file) {
                config.apply_ini(&content);
            }
        }
        for key in ["npm_config_registry", "NPM_CONFIG_REGISTRY"] {
            if let Ok(value) = std::env::var(key)
                && !value.trim().is_empty()
            {
                config.registry = with_trailing_slash(value.trim());
            }
        }
        config
    }

    fn apply_ini(&mut self, content: &str) {
        for raw in content.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some(eq) = line.find('=') else {
                continue;
            };
            let key = line[..eq].trim();
            let mut value = line[eq + 1..].trim().to_string();
            if value.len() >= 2
                && ((value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\'')))
            {
                value = value[1..value.len() - 1].to_string();
            }
            let value = expand_env(&value);
            let key = expand_env(key);
            if key == "registry" {
                self.registry = with_trailing_slash(&value);
            } else if let Some(scope) = key.strip_suffix(":registry")
                && scope.starts_with('@')
            {
                self.scoped.insert(scope.to_string(), with_trailing_slash(&value));
            } else if let Some(prefix) = key.strip_suffix(":_authToken") {
                self.set_auth(prefix, AuthEntry::Token(value));
            } else if let Some(prefix) = key.strip_suffix(":_auth") {
                self.set_auth(prefix, AuthEntry::Basic(value));
            }
        }
    }

    fn set_auth(&mut self, prefix: &str, entry: AuthEntry) {
        let prefix = with_trailing_slash(prefix);
        self.auth.retain(|(p, _)| *p != prefix);
        self.auth.push((prefix, entry));
    }

    /// The registry base URL (with trailing slash) for a package name.
    pub fn registry_for(&self, name: &str) -> String {
        if name.starts_with('@')
            && let Some(slash) = name.find('/')
            && let Some(registry) = self.scoped.get(&name[..slash])
        {
            return registry.clone();
        }
        self.registry.clone()
    }

    fn auth_header(&self, url: &str) -> Option<String> {
        let dart = nerf_dart(url)?;
        let (_, entry) = self
            .auth
            .iter()
            .filter(|(prefix, _)| dart.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())?;
        Some(match entry {
            AuthEntry::Token(token) => format!("Bearer {token}"),
            AuthEntry::Basic(basic) => format!("Basic {basic}"),
        })
    }
}

/// How a dependency is fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DepSpec {
    /// A registry package (`name` may differ from the install folder for `npm:` aliases).
    Registry { name: String, spec: String },
    /// A tarball URL.
    Tarball { url: String },
}

fn classify_spec(alias: &str, raw: &str) -> anyhow::Result<DepSpec> {
    let spec = raw.trim();
    if let Some(rest) = spec.strip_prefix("npm:") {
        let (name, version) = split_name_spec(rest);
        return Ok(DepSpec::Registry {
            name,
            spec: version.unwrap_or_default(),
        });
    }
    if spec.starts_with("http://") || spec.starts_with("https://") {
        return Ok(DepSpec::Tarball {
            url: spec.to_string(),
        });
    }
    let unsupported = spec.starts_with("file:")
        || spec.starts_with("link:")
        || spec.starts_with("workspace:")
        || spec.starts_with("git")
        || spec.starts_with("github:")
        || spec.starts_with("gitlab:")
        || spec.starts_with("bitbucket:")
        || spec.starts_with("gist:")
        || spec.starts_with('.')
        || spec.starts_with('/')
        // `~/dir` is a path; `~1.2.3` is a semver tilde range.
        || spec.starts_with("~/")
        || spec.contains('/');
    if unsupported {
        bail!(
            "Unsupported dependency specifier for {alias}: \"{spec}\" (install npm to use git, file, or workspace dependencies)"
        );
    }
    Ok(DepSpec::Registry {
        name: alias.to_string(),
        spec: spec.to_string(),
    })
}

/// Split `name@spec` like the package manager's `parseNpmSpec`.
pub fn split_name_spec(value: &str) -> (String, Option<String>) {
    // /^(@?[^@]+(?:\/[^@]+)?)(?:@(.+))?$/
    let (scope_at, rest) = match value.strip_prefix('@') {
        Some(rest) => ("@", rest),
        None => ("", value),
    };
    match rest.find('@') {
        Some(index) if index > 0 && index + 1 < rest.len() => (
            format!("{scope_at}{}", &rest[..index]),
            Some(rest[index + 1..].to_string()),
        ),
        Some(index) if index > 0 => (format!("{scope_at}{}", &rest[..index]), None),
        _ => (value.to_string(), None),
    }
}

fn escape_package_name(name: &str) -> String {
    name.replace('/', "%2f")
}

fn current_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

fn current_cpu() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "x86" => "ia32",
        "aarch64" => "arm64",
        "powerpc64" => "ppc64",
        other => other,
    }
}

/// npm's `checkPlatform` for one of the `os`/`cpu` arrays.
fn platform_list_allows(list: Option<&Value>, current: &str) -> bool {
    let Some(items) = list.and_then(Value::as_array) else {
        return true;
    };
    let entries: Vec<&str> = items.iter().filter_map(Value::as_str).collect();
    if entries.is_empty() {
        return true;
    }
    let mut allowed = false;
    let mut has_positive = false;
    for entry in &entries {
        if let Some(negated) = entry.strip_prefix('!') {
            if negated == current {
                return false;
            }
        } else {
            has_positive = true;
            if *entry == current {
                allowed = true;
            }
        }
    }
    allowed || !has_positive
}

fn platform_supported(manifest: &Value) -> bool {
    platform_list_allows(manifest.get("os"), current_os())
        && platform_list_allows(manifest.get("cpu"), current_cpu())
}

/// The package directory that owns `dir/node_modules`, i.e. the parent package of a package dir.
fn parent_package_dir(package_dir: &Path, root: &Path) -> Option<PathBuf> {
    if package_dir == root {
        return None;
    }
    let mut current = package_dir.parent()?;
    loop {
        if current.file_name().is_some_and(|n| n == "node_modules") {
            return current.parent().map(Path::to_path_buf);
        }
        current = current.parent()?;
    }
}

fn package_location(dir: &Path, name: &str) -> PathBuf {
    let mut path = dir.join("node_modules");
    for part in name.split('/') {
        path.push(part);
    }
    path
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(crate::utils::text::strip_bom(&text).as_str()).ok()
}

/// `(name, spec, optional)`.
type DepEntry = (String, String, bool);

fn dependency_entries(manifest: &Value) -> Vec<DepEntry> {
    let mut deps: Vec<DepEntry> = Vec::new();
    let bundled: HashSet<String> = manifest
        .get("bundleDependencies")
        .or_else(|| manifest.get("bundledDependencies"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if let Some(map) = manifest.get("dependencies").and_then(Value::as_object) {
        for (name, spec) in map {
            if bundled.contains(name) {
                continue;
            }
            deps.push((name.clone(), spec.as_str().unwrap_or("").to_string(), false));
        }
    }
    if let Some(map) = manifest.get("optionalDependencies").and_then(Value::as_object) {
        for (name, spec) in map {
            if bundled.contains(name) {
                continue;
            }
            let spec = spec.as_str().unwrap_or("").to_string();
            if let Some(existing) = deps.iter_mut().find(|(n, _, _)| n == name) {
                existing.1 = spec;
                existing.2 = true;
            } else {
                deps.push((name.clone(), spec, true));
            }
        }
    }
    deps
}

#[derive(Debug, Clone)]
struct Resolved {
    /// The real package name (differs from the folder name for aliases).
    name: String,
    version: String,
    tarball: String,
    integrity: Option<String>,
    shasum: Option<String>,
    manifest: Value,
}

#[derive(Debug, Clone)]
enum NodeSource {
    /// Already on disk and kept.
    Existing,
    /// To be downloaded and extracted.
    Fetch(Resolved),
}

#[derive(Debug, Clone)]
struct TreeNode {
    version: String,
    real_name: String,
    manifest: Value,
    source: NodeSource,
}

/// The pure-Rust npm client.
pub struct NpmRegistryClient {
    config: NpmrcConfig,
    http: reqwest::Client,
    packuments: tokio::sync::Mutex<HashMap<String, Arc<Value>>>,
}

impl NpmRegistryClient {
    pub fn new(config_dirs: &[PathBuf]) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(300))
            .user_agent(crate::utils::pi_user_agent::get_pi_user_agent(
                crate::config::VERSION,
            ))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            config: NpmrcConfig::load(config_dirs),
            http,
            packuments: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    async fn get_with_retry(&self, url: &str, accept: Option<&str>) -> anyhow::Result<reqwest::Response> {
        let mut last_error = None;
        for attempt in 0..=FETCH_RETRIES {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }
            let mut request = self.http.get(url);
            if let Some(accept) = accept {
                request = request.header(reqwest::header::ACCEPT, accept);
            }
            if let Some(auth) = self.config.auth_header(url) {
                request = request.header(reqwest::header::AUTHORIZATION, auth);
            }
            match request.send().await {
                Ok(response) if response.status().is_server_error() => {
                    last_error = Some(anyhow!(
                        "{} {} - GET {url}",
                        response.status().as_u16(),
                        response.status().canonical_reason().unwrap_or("")
                    ));
                }
                Ok(response) => return Ok(response),
                Err(error) => last_error = Some(anyhow!(error).context(format!("GET {url}"))),
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("GET {url} failed")))
    }

    /// Fetch (and cache) the abbreviated packument of a package.
    async fn packument(&self, name: &str) -> anyhow::Result<Arc<Value>> {
        if let Some(cached) = self.packuments.lock().await.get(name) {
            return Ok(cached.clone());
        }
        let url = format!("{}{}", self.config.registry_for(name), escape_package_name(name));
        let response = self.get_with_retry(&url, Some(CORGI_ACCEPT)).await?;
        let status = response.status();
        if status.as_u16() == 404 {
            bail!("404 Not Found - GET {url} - '{name}' is not in this registry.");
        }
        if !status.is_success() {
            bail!(
                "{} {} - GET {url}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("")
            );
        }
        let value: Value = response
            .json()
            .await
            .with_context(|| format!("Invalid registry response for {name}"))?;
        let value = Arc::new(value);
        self.packuments
            .lock()
            .await
            .insert(name.to_string(), value.clone());
        Ok(value)
    }

    /// Pick a version like `npm-pick-manifest`.
    fn pick_version(packument: &Value, name: &str, spec: &str) -> anyhow::Result<String> {
        let versions = packument
            .get("versions")
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow!("No versions available for {name}"))?;
        let tags = packument.get("dist-tags").and_then(Value::as_object);
        let spec = spec.trim();
        let tag_name = if spec.is_empty() { "latest" } else { spec };
        if let Some(version) = tags.and_then(|t| t.get(tag_name)).and_then(Value::as_str)
            && versions.contains_key(version)
        {
            return Ok(version.to_string());
        }
        if let Some(exact) = node_semver::parse_version(spec) {
            if let Some(key) = versions.keys().find(|key| {
                node_semver::parse_version(key)
                    .is_some_and(|v| node_semver::compare(&v, &exact) == std::cmp::Ordering::Equal)
            }) {
                return Ok(key.clone());
            }
            bail!("No matching version found for {name}@{spec}.");
        }
        let range = if spec.is_empty() { "*" } else { spec };
        if !node_semver::is_valid_range(range) {
            bail!("No matching version found for {name}@{spec}.");
        }
        if let Some(latest) = tags.and_then(|t| t.get("latest")).and_then(Value::as_str)
            && versions.contains_key(latest)
            && node_semver::satisfies(latest, range)
        {
            return Ok(latest.to_string());
        }
        let not_deprecated = versions
            .iter()
            .filter(|(_, manifest)| manifest.get("deprecated").is_none())
            .map(|(key, _)| key.as_str());
        if let Some(best) = node_semver::max_satisfying(not_deprecated, range) {
            return Ok(best);
        }
        if let Some(best) = node_semver::max_satisfying(versions.keys().map(String::as_str), range) {
            return Ok(best);
        }
        bail!("No matching version found for {name}@{spec}.")
    }

    async fn resolve_registry(&self, name: &str, spec: &str) -> anyhow::Result<Resolved> {
        let packument = self.packument(name).await?;
        let version = Self::pick_version(&packument, name, spec)?;
        let manifest = packument
            .get("versions")
            .and_then(|v| v.get(&version))
            .cloned()
            .unwrap_or(Value::Null);
        let dist = manifest.get("dist").cloned().unwrap_or(Value::Null);
        let tarball = dist
            .get("tarball")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("No tarball for {name}@{version}"))?
            .to_string();
        Ok(Resolved {
            name: name.to_string(),
            version,
            tarball,
            integrity: dist.get("integrity").and_then(Value::as_str).map(str::to_string),
            shasum: dist.get("shasum").and_then(Value::as_str).map(str::to_string),
            manifest,
        })
    }

    async fn download(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        let response = self.get_with_retry(url, None).await?;
        let status = response.status();
        if !status.is_success() {
            bail!(
                "{} {} - GET {url}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("")
            );
        }
        Ok(response.bytes().await?.to_vec())
    }

    async fn resolve_tarball(&self, alias: &str, url: &str) -> anyhow::Result<(Resolved, Vec<u8>)> {
        let bytes = self.download(url).await?;
        let manifest = read_manifest_from_tarball(&bytes)?
            .ok_or_else(|| anyhow!("No package.json in tarball {url}"))?;
        let name = manifest
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(alias)
            .to_string();
        let version = manifest
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("0.0.0")
            .to_string();
        Ok((
            Resolved {
                name,
                version,
                tarball: url.to_string(),
                integrity: None,
                shasum: None,
                manifest,
            },
            bytes,
        ))
    }

    /// `npm view <spec> version --json` resolved to one version: the dist-tag (default `latest`)
    /// for tag specs, the highest version in the range for ranges.
    pub async fn latest_version(&self, package_spec: &str, range: Option<&str>) -> anyhow::Result<String> {
        let (name, version) = split_name_spec(package_spec);
        let packument = self.packument(&name).await?;
        if let Some(range) = range {
            let versions = packument
                .get("versions")
                .and_then(Value::as_object)
                .map(|v| v.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            return node_semver::max_satisfying(versions.iter().map(String::as_str), range)
                .ok_or_else(|| anyhow!("No matching version found for {package_spec}."));
        }
        Self::pick_version(&packument, &name, version.as_deref().unwrap_or(""))
    }

    /// `npm install <specs> --prefix <root>`: add the packages to `<root>/package.json` and
    /// install the whole dependency tree.
    pub async fn install_packages(&self, root: &Path, specs: &[String]) -> anyhow::Result<()> {
        let package_json_path = root.join("package.json");
        let mut root_manifest = read_json(&package_json_path).unwrap_or_else(|| Value::Object(Map::new()));
        let mut forced: HashMap<String, String> = HashMap::new();
        let mut requested: Vec<(String, String, Option<String>)> = Vec::new();
        for spec in specs {
            let (name, version) = split_name_spec(spec);
            forced.insert(name.clone(), version.clone().unwrap_or_default());
            requested.push((name, version.clone().unwrap_or_default(), version));
        }

        let resolved = self.install_tree(root, &root_manifest, &forced).await?;

        // Save the requested dependencies like npm's default save behavior.
        let object = root_manifest
            .as_object_mut()
            .ok_or_else(|| anyhow!("Invalid package.json at {}", package_json_path.display()))?;
        let deps = object
            .entry("dependencies")
            .or_insert_with(|| Value::Object(Map::new()));
        if !deps.is_object() {
            *deps = Value::Object(Map::new());
        }
        let deps = deps.as_object_mut().expect("dependencies object");
        for (name, _, explicit) in requested {
            let installed = resolved.get(&name).cloned().unwrap_or_default();
            let saved = match explicit {
                Some(spec) if node_semver::is_valid_version(&spec) => spec,
                Some(spec) if node_semver::is_valid_range(&spec) && spec != "*" && !spec.is_empty() => spec,
                _ => format!("^{installed}"),
            };
            deps.insert(name, Value::String(saved));
        }
        let text = serde_json::to_string_pretty(&root_manifest)?;
        std::fs::write(&package_json_path, format!("{text}\n"))?;
        Ok(())
    }

    /// `npm install --omit=dev` in a package directory: install its dependencies only.
    pub async fn install_dependencies(&self, root: &Path) -> anyhow::Result<()> {
        let manifest = read_json(&root.join("package.json"))
            .ok_or_else(|| anyhow!("Cannot read {}", root.join("package.json").display()))?;
        self.install_tree(root, &manifest, &HashMap::new()).await?;
        Ok(())
    }

    /// Plan and reify the dependency tree of `root_manifest` under `root/node_modules`.
    /// Returns the resolved versions of the root's direct dependencies.
    async fn install_tree(
        &self,
        root: &Path,
        root_manifest: &Value,
        forced: &HashMap<String, String>,
    ) -> anyhow::Result<HashMap<String, String>> {
        let mut root_deps = dependency_entries(root_manifest);
        for (name, spec) in forced {
            match root_deps.iter_mut().find(|(n, _, _)| n == name) {
                Some(entry) => {
                    entry.1 = spec.clone();
                    entry.2 = false;
                }
                None => root_deps.push((name.clone(), spec.clone(), false)),
            }
        }

        let mut tree: HashMap<PathBuf, TreeNode> = HashMap::new();
        let mut replaced: Vec<PathBuf> = Vec::new();
        let mut visited: HashSet<PathBuf> = HashSet::new();
        let mut queue: VecDeque<(PathBuf, Vec<DepEntry>)> = VecDeque::new();
        let mut direct: HashMap<String, String> = HashMap::new();
        let mut prefetched_tarballs: HashMap<String, Vec<u8>> = HashMap::new();
        queue.push_back((root.to_path_buf(), root_deps));
        visited.insert(root.to_path_buf());

        while let Some((requester, deps)) = queue.pop_front() {
            // Prefetch packuments for this package's registry dependencies concurrently.
            let names: Vec<String> = deps
                .iter()
                .filter_map(|(alias, spec, _)| match classify_spec(alias, spec) {
                    Ok(DepSpec::Registry { name, .. }) => Some(name),
                    _ => None,
                })
                .collect();
            let fetches = names.iter().map(|name| self.packument(name));
            let _ = futures::future::join_all(fetches).await;

            for (alias, raw_spec, optional) in deps {
                let is_root = requester == root;
                let is_forced = is_root && forced.contains_key(&alias);
                let result = self
                    .place_dependency(
                        root,
                        &requester,
                        &alias,
                        &raw_spec,
                        is_forced,
                        &mut tree,
                        &mut replaced,
                        &mut prefetched_tarballs,
                    )
                    .await;
                match result {
                    Ok((location, version)) => {
                        if is_root {
                            direct.insert(alias.clone(), version);
                        }
                        if visited.insert(location.clone())
                            && let Some(node) = tree.get(&location)
                        {
                            let fetched = matches!(node.source, NodeSource::Fetch(_));
                            if fetched && !platform_supported(&node.manifest) {
                                if optional {
                                    tree.remove(&location);
                                    continue;
                                }
                                bail!(
                                    "Unsupported platform for {}@{}: wanted {{\"os\":{},\"cpu\":{}}} (current: {{\"os\":\"{}\",\"cpu\":\"{}\"}})",
                                    node.real_name,
                                    node.version,
                                    node.manifest.get("os").cloned().unwrap_or(Value::Null),
                                    node.manifest.get("cpu").cloned().unwrap_or(Value::Null),
                                    current_os(),
                                    current_cpu()
                                );
                            }
                            queue.push_back((location.clone(), dependency_entries(&node.manifest)));
                        }
                    }
                    Err(error) => {
                        if optional {
                            tracing::warn!("Skipping optional dependency {alias}: {error:#}");
                            continue;
                        }
                        return Err(error);
                    }
                }
            }
        }

        self.reify(root, &tree, prefetched_tarballs).await?;
        Ok(direct)
    }

    /// Find or plan the location of `alias` for `requester`. Returns `(location, version)`.
    #[allow(clippy::too_many_arguments)]
    async fn place_dependency(
        &self,
        root: &Path,
        requester: &Path,
        alias: &str,
        raw_spec: &str,
        forced: bool,
        tree: &mut HashMap<PathBuf, TreeNode>,
        replaced: &mut Vec<PathBuf>,
        tarballs: &mut HashMap<String, Vec<u8>>,
    ) -> anyhow::Result<(PathBuf, String)> {
        let dep = classify_spec(alias, raw_spec)?;

        // Resolve the target first (tags and forced specs need the concrete version).
        let (resolved, tarball_bytes) = match &dep {
            DepSpec::Registry { name, spec } => (self.resolve_registry(name, spec).await?, None),
            DepSpec::Tarball { url } => {
                let (resolved, bytes) = self.resolve_tarball(alias, url).await?;
                (resolved, Some(bytes))
            }
        };

        let accepts = |version: &str, real_name: &str| -> bool {
            if real_name != resolved.name {
                return false;
            }
            match &dep {
                DepSpec::Registry { spec, .. } => {
                    let spec = spec.trim();
                    if forced {
                        return version == resolved.version;
                    }
                    if spec.is_empty() || spec == "*" {
                        return node_semver::is_valid_version(version);
                    }
                    if node_semver::is_valid_range(spec) {
                        node_semver::satisfies(version, spec)
                    } else {
                        version == resolved.version
                    }
                }
                DepSpec::Tarball { .. } => version == resolved.version,
            }
        };

        let is_replaced = |path: &Path, replaced: &[PathBuf]| -> bool {
            replaced.iter().any(|r| path.starts_with(r) && path != r.as_path())
        };

        let mut target: Option<PathBuf> = None;
        let mut dir = requester.to_path_buf();
        loop {
            let location = package_location(&dir, alias);
            let existing = if let Some(node) = tree.get(&location) {
                Some((node.version.clone(), node.real_name.clone()))
            } else if !is_replaced(&location, replaced) {
                read_json(&location.join("package.json")).map(|manifest| {
                    (
                        manifest.get("version").and_then(Value::as_str).unwrap_or("").to_string(),
                        manifest.get("name").and_then(Value::as_str).unwrap_or(alias).to_string(),
                    )
                })
            } else {
                None
            };
            if let Some((version, real_name)) = existing {
                if accepts(&version, &real_name) {
                    if !tree.contains_key(&location) {
                        let manifest = read_json(&location.join("package.json")).unwrap_or(Value::Null);
                        tree.insert(
                            location.clone(),
                            TreeNode {
                                version: version.clone(),
                                real_name,
                                manifest,
                                source: NodeSource::Existing,
                            },
                        );
                    }
                    return Ok((location, version));
                }
                // Conflict: replace at the requester's own level, otherwise nest under it.
                target = Some(package_location(requester, alias));
                break;
            }
            if dir.as_path() == root {
                break;
            }
            match parent_package_dir(&dir, root) {
                Some(parent) => dir = parent,
                None => break,
            }
        }
        let location = target.unwrap_or_else(|| package_location(root, alias));
        if location.exists() || tree.contains_key(&location) {
            replaced.push(location.clone());
            // Drop planned nodes below the replaced folder.
            tree.retain(|path, _| !(path.starts_with(&location) && path != &location));
        }
        if let Some(bytes) = tarball_bytes {
            tarballs.insert(resolved.tarball.clone(), bytes);
        }
        let version = resolved.version.clone();
        tree.insert(
            location.clone(),
            TreeNode {
                version: version.clone(),
                real_name: resolved.name.clone(),
                manifest: resolved.manifest.clone(),
                source: NodeSource::Fetch(resolved),
            },
        );
        Ok((location, version))
    }

    async fn reify(
        &self,
        root: &Path,
        tree: &HashMap<PathBuf, TreeNode>,
        mut tarballs: HashMap<String, Vec<u8>>,
    ) -> anyhow::Result<()> {
        let mut to_fetch: Vec<(PathBuf, Resolved)> = tree
            .iter()
            .filter_map(|(path, node)| match &node.source {
                NodeSource::Fetch(resolved) => Some((path.clone(), resolved.clone())),
                NodeSource::Existing => None,
            })
            .collect();
        to_fetch.sort_by_key(|(path, _)| path.components().count());

        // Download everything first (concurrently), then extract parents before children.
        use futures::StreamExt;
        let missing: Vec<Resolved> = to_fetch
            .iter()
            .filter(|(_, r)| !tarballs.contains_key(&r.tarball))
            .map(|(_, r)| r.clone())
            .collect();
        let downloads = futures::stream::iter(missing.into_iter().map(|resolved| async move {
            let bytes = self.download(&resolved.tarball).await?;
            verify_integrity(&resolved, &bytes)?;
            Ok::<_, anyhow::Error>((resolved.tarball.clone(), bytes))
        }))
        .buffer_unordered(DOWNLOAD_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
        for download in downloads {
            let (url, bytes) = download?;
            tarballs.insert(url, bytes);
        }

        let mut bin_targets: Vec<(PathBuf, Value)> = Vec::new();
        for (location, resolved) in &to_fetch {
            let bytes = tarballs
                .get(&resolved.tarball)
                .ok_or_else(|| anyhow!("Missing tarball for {}@{}", resolved.name, resolved.version))?;
            let parent = location
                .parent()
                .ok_or_else(|| anyhow!("Invalid install location {}", location.display()))?;
            std::fs::create_dir_all(parent)?;
            let staging = parent.join(format!(
                ".{}-{}",
                location.file_name().and_then(|n| n.to_str()).unwrap_or("pkg"),
                uuid::Uuid::new_v4().simple()
            ));
            let extract_result = extract_tarball(bytes, &staging);
            if let Err(error) = extract_result {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(error.context(format!(
                    "Failed to extract {}@{}",
                    resolved.name, resolved.version
                )));
            }
            if location.exists() || std::fs::symlink_metadata(location).is_ok() {
                remove_path(location)?;
            }
            std::fs::rename(&staging, location).with_context(|| {
                format!("Failed to move {} into place", location.display())
            })?;
            if has_install_scripts(&resolved.manifest) {
                tracing::warn!(
                    "{}@{} has install scripts that were not run (npm is not available)",
                    resolved.name,
                    resolved.version
                );
            }
            bin_targets.push((location.clone(), resolved.manifest.clone()));
        }

        for (location, manifest) in bin_targets {
            link_bins(&location, &manifest);
        }
        // npm ignores a stale hidden lockfile, but drop it so it never shadows this tree.
        let _ = std::fs::remove_file(root.join("node_modules").join(".package-lock.json"));
        Ok(())
    }

    /// `npm uninstall <name> --prefix <root>`.
    pub async fn uninstall(&self, root: &Path, name: &str) -> anyhow::Result<()> {
        let package_json_path = root.join("package.json");
        if let Some(mut manifest) = read_json(&package_json_path) {
            let mut changed = false;
            if let Some(object) = manifest.as_object_mut() {
                for field in [
                    "dependencies",
                    "devDependencies",
                    "optionalDependencies",
                    "peerDependencies",
                ] {
                    if let Some(deps) = object.get_mut(field).and_then(Value::as_object_mut)
                        && deps.shift_remove(name).is_some()
                    {
                        changed = true;
                    }
                }
            }
            if changed {
                let text = serde_json::to_string_pretty(&manifest)?;
                std::fs::write(&package_json_path, format!("{text}\n"))?;
            }
        }
        let location = package_location(root, name);
        if std::fs::symlink_metadata(&location).is_ok() {
            remove_path(&location)?;
        }
        prune_extraneous(root);
        let _ = std::fs::remove_file(root.join("node_modules").join(".package-lock.json"));
        Ok(())
    }
}

fn has_install_scripts(manifest: &Value) -> bool {
    if manifest.get("hasInstallScript").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    manifest
        .get("scripts")
        .and_then(Value::as_object)
        .is_some_and(|s| s.contains_key("preinstall") || s.contains_key("install") || s.contains_key("postinstall"))
}

fn remove_path(path: &Path) -> anyhow::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Create `node_modules/.bin` links for a package's `bin` entries (Unix only).
fn link_bins(location: &Path, manifest: &Value) {
    #[cfg(unix)]
    {
        let Some(bin) = manifest.get("bin") else {
            return;
        };
        let name = manifest.get("name").and_then(Value::as_str).unwrap_or("");
        let entries: Vec<(String, String)> = match bin {
            Value::String(path) => {
                let bin_name = name.rsplit('/').next().unwrap_or(name).to_string();
                vec![(bin_name, path.clone())]
            }
            Value::Object(map) => map
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|p| (k.clone(), p.to_string())))
                .collect(),
            _ => return,
        };
        // The node_modules folder that contains this package (scoped packages are one deeper).
        let mut node_modules = location.parent();
        if node_modules.is_some_and(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('@'))) {
            node_modules = node_modules.and_then(Path::parent);
        }
        let Some(node_modules) = node_modules else {
            return;
        };
        let bin_dir = node_modules.join(".bin");
        for (bin_name, target) in entries {
            let bin_name = bin_name.rsplit('/').next().unwrap_or(&bin_name).to_string();
            if bin_name.is_empty() || bin_name.contains("..") {
                continue;
            }
            let target_path = location.join(target.trim_start_matches("./"));
            if !target_path.starts_with(location) || !target_path.exists() {
                continue;
            }
            if std::fs::create_dir_all(&bin_dir).is_err() {
                return;
            }
            let link = bin_dir.join(&bin_name);
            let _ = std::fs::remove_file(&link);
            let relative = crate::utils::paths::node_relative(
                &bin_dir.to_string_lossy(),
                &target_path.to_string_lossy(),
            );
            let _ = std::os::unix::fs::symlink(&relative, &link);
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&target_path) {
                let mut perms = meta.permissions();
                perms.set_mode(perms.mode() | 0o111);
                let _ = std::fs::set_permissions(&target_path, perms);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (location, manifest);
    }
}

/// Remove packages under `root/node_modules` that no dependency chain reaches.
fn prune_extraneous(root: &Path) {
    let Some(root_manifest) = read_json(&root.join("package.json")) else {
        return;
    };
    let mut reachable: HashSet<PathBuf> = HashSet::new();
    let mut queue: VecDeque<(PathBuf, Value)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), root_manifest));
    while let Some((dir, manifest)) = queue.pop_front() {
        let mut deps = dependency_entries(&manifest);
        if dir == root
            && let Some(dev) = manifest.get("devDependencies").and_then(Value::as_object)
        {
            for (name, spec) in dev {
                deps.push((name.clone(), spec.as_str().unwrap_or("").to_string(), true));
            }
        }
        for (name, _, _) in deps {
            let mut current = dir.clone();
            loop {
                let location = package_location(&current, &name);
                if let Some(child) = read_json(&location.join("package.json")) {
                    if reachable.insert(location.clone()) {
                        queue.push_back((location, child));
                    }
                    break;
                }
                if current == root {
                    break;
                }
                match parent_package_dir(&current, root) {
                    Some(parent) => current = parent,
                    None => break,
                }
            }
        }
    }
    prune_dir(&root.join("node_modules"), &reachable);
}

fn prune_dir(node_modules: &Path, reachable: &HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(node_modules) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with('.') {
            if name == ".bin" {
                prune_bins(&path);
            }
            continue;
        }
        if name.starts_with('@') {
            prune_dir(&path, reachable);
            if std::fs::read_dir(&path).map(|mut d| d.next().is_none()).unwrap_or(false) {
                let _ = std::fs::remove_dir(&path);
            }
            continue;
        }
        if reachable.contains(&path) {
            prune_dir(&path.join("node_modules"), reachable);
        } else {
            let _ = remove_path(&path);
        }
    }
}

fn prune_bins(bin_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(bin_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) && !path.exists() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Verify a tarball against the strongest supported `dist.integrity` hash, else `dist.shasum`.
fn verify_integrity(resolved: &Resolved, bytes: &[u8]) -> anyhow::Result<()> {
    use sha2::Digest;
    if let Some(integrity) = &resolved.integrity {
        let mut best: Option<(u8, &str, &str)> = None;
        for entry in integrity.split_whitespace() {
            let Some((algorithm, digest)) = entry.split_once('-') else {
                continue;
            };
            let digest = digest.split('?').next().unwrap_or(digest);
            let rank = match algorithm {
                "sha512" => 4,
                "sha384" => 3,
                "sha256" => 2,
                "sha1" => 1,
                _ => continue,
            };
            if best.is_none_or(|(r, _, _)| rank > r) {
                best = Some((rank, algorithm, digest));
            }
        }
        if let Some((_, algorithm, _)) = best {
            let actual = match algorithm {
                "sha512" => sha2::Sha512::digest(bytes).to_vec(),
                "sha384" => sha2::Sha384::digest(bytes).to_vec(),
                "sha256" => sha2::Sha256::digest(bytes).to_vec(),
                _ => sha1::Sha1::digest(bytes).to_vec(),
            };
            let actual = base64::engine::general_purpose::STANDARD.encode(actual);
            let matches = integrity.split_whitespace().any(|entry| {
                entry
                    .split_once('-')
                    .is_some_and(|(a, d)| a == algorithm && d.split('?').next() == Some(actual.as_str()))
            });
            if !matches {
                bail!(
                    "Integrity check failed for {}@{} ({}): wanted {} but got {algorithm}-{actual}",
                    resolved.name,
                    resolved.version,
                    resolved.tarball,
                    integrity
                );
            }
            return Ok(());
        }
    }
    if let Some(shasum) = &resolved.shasum {
        let actual: String = sha1::Sha1::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if !actual.eq_ignore_ascii_case(shasum) {
            bail!(
                "Integrity check failed for {}@{} ({}): wanted sha1 {} but got {}",
                resolved.name,
                resolved.version,
                resolved.tarball,
                shasum,
                actual
            );
        }
    }
    Ok(())
}

fn decompress(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    use std::io::Read;
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_end(&mut out)
            .context("Invalid gzip data")?;
        return Ok(out);
    }
    Ok(bytes.to_vec())
}

fn cstr(field: &[u8]) -> String {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

fn parse_octal(field: &[u8]) -> u64 {
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        // Base-256 encoding for large values.
        let mut value: u64 = (field[0] & 0x7f) as u64;
        for &b in &field[1..] {
            value = (value << 8) | b as u64;
        }
        return value;
    }
    let text = cstr(field);
    u64::from_str_radix(text.trim().trim_matches(' '), 8).unwrap_or(0)
}

fn parse_pax(content: &[u8]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut rest = content;
    while !rest.is_empty() {
        let Some(space) = rest.iter().position(|&b| b == b' ') else {
            break;
        };
        let Ok(len) = std::str::from_utf8(&rest[..space]).unwrap_or("").parse::<usize>() else {
            break;
        };
        if len == 0 || len > rest.len() {
            break;
        }
        let record = &rest[space + 1..len];
        let record = record.strip_suffix(b"\n").unwrap_or(record);
        if let Some(eq) = record.iter().position(|&b| b == b'=') {
            out.insert(
                String::from_utf8_lossy(&record[..eq]).into_owned(),
                String::from_utf8_lossy(&record[eq + 1..]).into_owned(),
            );
        }
        rest = &rest[len..];
    }
    out
}

struct TarEntry {
    path: String,
    kind: u8,
    mode: u64,
    start: usize,
    size: usize,
}

fn tar_entries(data: &[u8]) -> anyhow::Result<Vec<TarEntry>> {
    let mut entries = Vec::new();
    let mut offset = 0usize;
    let mut long_name: Option<String> = None;
    let mut pax_path: Option<String> = None;
    while offset + 512 <= data.len() {
        let header = &data[offset..offset + 512];
        if header.iter().all(|&b| b == 0) {
            break;
        }
        let name = cstr(&header[0..100]);
        let mode = parse_octal(&header[100..108]);
        let size = parse_octal(&header[124..136]) as usize;
        let kind = header[156];
        let prefix = if &header[257..262] == b"ustar" {
            cstr(&header[345..500])
        } else {
            String::new()
        };
        offset += 512;
        if offset + size > data.len() {
            bail!("Truncated tar archive");
        }
        let start = offset;
        offset += size.div_ceil(512) * 512;
        match kind {
            b'x' => {
                if let Some(path) = parse_pax(&data[start..start + size]).remove("path") {
                    pax_path = Some(path);
                }
                continue;
            }
            b'g' => continue,
            b'L' => {
                long_name = Some(cstr(&data[start..start + size]));
                continue;
            }
            _ => {}
        }
        let path = pax_path
            .take()
            .or_else(|| long_name.take())
            .unwrap_or_else(|| {
                if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                }
            });
        entries.push(TarEntry {
            path,
            kind,
            mode,
            start,
            size,
        });
    }
    Ok(entries)
}

/// Strip the first path component (`package/`) and reject unsafe paths.
fn sanitize_entry_path(path: &str) -> Option<PathBuf> {
    let normalized = path.replace('\\', "/");
    let mut parts = normalized.split('/').filter(|p| !p.is_empty() && *p != ".");
    parts.next()?;
    let mut out = PathBuf::new();
    for part in parts {
        if part == ".." || part.contains(':') {
            return None;
        }
        out.push(part);
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Read `package/package.json` from a tarball.
fn read_manifest_from_tarball(bytes: &[u8]) -> anyhow::Result<Option<Value>> {
    let data = decompress(bytes)?;
    for entry in tar_entries(&data)? {
        if matches!(entry.kind, b'0' | 0 | b'7')
            && sanitize_entry_path(&entry.path).is_some_and(|p| p == Path::new("package.json"))
        {
            let content = &data[entry.start..entry.start + entry.size];
            return Ok(serde_json::from_slice(content).ok());
        }
    }
    Ok(None)
}

/// Extract an npm tarball into `dest` (created), like pacote: first component stripped, links
/// skipped, files `0o644`/`0o755`.
pub fn extract_tarball(bytes: &[u8], dest: &Path) -> anyhow::Result<()> {
    let data = decompress(bytes)?;
    std::fs::create_dir_all(dest)?;
    for entry in tar_entries(&data)? {
        let Some(rel) = sanitize_entry_path(&entry.path) else {
            continue;
        };
        let target = dest.join(&rel);
        match entry.kind {
            b'0' | 0 | b'7' => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&target, &data[entry.start..entry.start + entry.size])?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = if entry.mode & 0o111 != 0 { 0o755 } else { 0o644 };
                    let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode));
                }
                #[cfg(not(unix))]
                let _ = entry.mode;
            }
            b'5' => {
                std::fs::create_dir_all(&target)?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A tiny local npm registry for tests (no network).

    use super::*;
    use std::io::Write;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Build a gzipped npm tarball (`package/<path>`) from `(path, content)` pairs.
    pub fn make_tarball(files: &[(&str, &str)]) -> Vec<u8> {
        let mut tar = Vec::new();
        for (path, content) in files {
            let mut header = [0u8; 512];
            let name = format!("package/{path}");
            header[..name.len()].copy_from_slice(name.as_bytes());
            header[100..107].copy_from_slice(b"0000644");
            header[108..115].copy_from_slice(b"0000000");
            header[116..123].copy_from_slice(b"0000000");
            let size = format!("{:011o}", content.len());
            header[124..135].copy_from_slice(size.as_bytes());
            header[136..147].copy_from_slice(b"00000000000");
            header[156] = b'0';
            header[257..263].copy_from_slice(b"ustar\0");
            header[263..265].copy_from_slice(b"00");
            header[148..156].copy_from_slice(b"        ");
            let sum: u32 = header.iter().map(|&b| b as u32).sum();
            let checksum = format!("{sum:06o}\0 ");
            header[148..156].copy_from_slice(checksum.as_bytes());
            tar.extend_from_slice(&header);
            tar.extend_from_slice(content.as_bytes());
            let pad = (512 - content.len() % 512) % 512;
            tar.extend(std::iter::repeat_n(0u8, pad));
        }
        tar.extend(std::iter::repeat_n(0u8, 1024));
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&tar).unwrap();
        encoder.finish().unwrap()
    }

    pub fn sri_sha512(bytes: &[u8]) -> String {
        use sha2::Digest;
        format!(
            "sha512-{}",
            base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(bytes))
        )
    }

    /// A package version to serve.
    pub struct FakeVersion {
        pub name: String,
        pub version: String,
        pub deps: Vec<(String, String)>,
        pub files: Vec<(String, String)>,
        pub corrupt: bool,
    }

    /// Serve packuments and tarballs over HTTP on 127.0.0.1. Returns the registry URL.
    pub async fn serve_registry(versions: Vec<FakeVersion>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}/");
        let mut routes: HashMap<String, (String, Vec<u8>)> = HashMap::new();
        let mut packuments: HashMap<String, Map<String, Value>> = HashMap::new();
        let mut latest: HashMap<String, String> = HashMap::new();
        for v in versions {
            let mut package_json = serde_json::json!({ "name": v.name, "version": v.version });
            if !v.deps.is_empty() {
                let deps: Map<String, Value> = v
                    .deps
                    .iter()
                    .map(|(n, s)| (n.clone(), Value::String(s.clone())))
                    .collect();
                package_json["dependencies"] = Value::Object(deps);
            }
            let package_json_text = serde_json::to_string(&package_json).unwrap();
            let mut files: Vec<(&str, &str)> = vec![("package.json", package_json_text.as_str())];
            for (p, c) in &v.files {
                files.push((p.as_str(), c.as_str()));
            }
            let tarball = make_tarball(&files);
            let integrity = if v.corrupt {
                sri_sha512(b"something else")
            } else {
                sri_sha512(&tarball)
            };
            let file_name = v.name.rsplit('/').next().unwrap();
            let tarball_path = format!("{}/-/{}-{}.tgz", v.name, file_name, v.version);
            let mut manifest = package_json.clone();
            manifest["dist"] = serde_json::json!({
                "tarball": format!("{base}{tarball_path}"),
                "integrity": integrity,
            });
            routes.insert(format!("/{tarball_path}"), ("application/octet-stream".into(), tarball));
            packuments
                .entry(v.name.clone())
                .or_default()
                .insert(v.version.clone(), manifest);
            let entry = latest.entry(v.name.clone()).or_insert_with(|| v.version.clone());
            if node_semver::gt(&v.version, entry).unwrap_or(false) {
                *entry = v.version.clone();
            }
        }
        for (name, versions) in packuments {
            let doc = serde_json::json!({
                "name": name,
                "dist-tags": { "latest": latest[&name] },
                "versions": versions,
            });
            routes.insert(
                format!("/{}", escape_package_name(&name)),
                ("application/json".into(), serde_json::to_vec(&doc).unwrap()),
            );
        }
        let routes = Arc::new(routes);
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    loop {
                        let Ok(n) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let request = String::from_utf8_lossy(&buf);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/")
                        .to_string();
                    let path = path.replace("%2F", "%2f");
                    let (status, content_type, body) = match routes.get(&path) {
                        Some((ct, body)) => ("200 OK", ct.clone(), body.clone()),
                        None => ("404 Not Found", "text/plain".to_string(), b"not found".to_vec()),
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        base
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    fn fake(name: &str, version: &str, deps: &[(&str, &str)]) -> FakeVersion {
        FakeVersion {
            name: name.to_string(),
            version: version.to_string(),
            deps: deps.iter().map(|(n, s)| (n.to_string(), s.to_string())).collect(),
            files: vec![("index.js".to_string(), format!("// {name}@{version}"))],
            corrupt: false,
        }
    }

    fn client(registry: &str, dir: &Path) -> NpmRegistryClient {
        std::fs::write(dir.join(".npmrc"), format!("registry={registry}\n")).unwrap();
        NpmRegistryClient::new(&[dir.to_path_buf()])
    }

    fn installed_version(path: &Path) -> Option<String> {
        read_json(&path.join("package.json"))?
            .get("version")?
            .as_str()
            .map(str::to_string)
    }

    #[test]
    fn classifies_tilde_ranges_and_home_paths() {
        assert_eq!(
            classify_spec("safer-buffer", "~2.1.0").unwrap(),
            DepSpec::Registry { name: "safer-buffer".into(), spec: "~2.1.0".into() }
        );
        assert!(classify_spec("local", "~/src/local").is_err());
        assert!(classify_spec("repo", "user/repo").is_err());
    }

    #[test]
    fn splits_name_specs() {
        assert_eq!(split_name_spec("pkg"), ("pkg".to_string(), None));
        assert_eq!(split_name_spec("pkg@1.0.0"), ("pkg".to_string(), Some("1.0.0".to_string())));
        assert_eq!(split_name_spec("@s/p"), ("@s/p".to_string(), None));
        assert_eq!(split_name_spec("@s/p@^2"), ("@s/p".to_string(), Some("^2".to_string())));
    }

    #[test]
    fn reads_npmrc_registry_scopes_and_auth() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".npmrc"),
            "registry=https://reg.example.com/npm\n@acme:registry=https://acme.example.com/\n//acme.example.com/:_authToken=secret\n",
        )
        .unwrap();
        let mut config = NpmrcConfig {
            registry: DEFAULT_REGISTRY.to_string(),
            scoped: HashMap::new(),
            auth: Vec::new(),
        };
        config.apply_ini(&std::fs::read_to_string(dir.path().join(".npmrc")).unwrap());
        assert_eq!(config.registry_for("left-pad"), "https://reg.example.com/npm/");
        assert_eq!(config.registry_for("@acme/tools"), "https://acme.example.com/");
        assert_eq!(
            config.auth_header("https://acme.example.com/@acme%2ftools").as_deref(),
            Some("Bearer secret")
        );
        assert_eq!(config.auth_header("https://reg.example.com/npm/x"), None);
    }

    #[test]
    fn extracts_tarballs_stripping_the_package_folder() {
        let tarball = make_tarball(&[("package.json", "{\"name\":\"x\"}"), ("lib/a.js", "a"), ("../evil", "x")]);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out");
        extract_tarball(&tarball, &dest).unwrap();
        assert_eq!(std::fs::read_to_string(dest.join("lib/a.js")).unwrap(), "a");
        assert!(dest.join("package.json").exists());
        assert!(!dir.path().join("evil").exists());
    }

    #[tokio::test]
    async fn installs_packages_with_hoisting_and_nesting() {
        let registry = serve_registry(vec![
            fake("app-ext", "1.0.0", &[("dep-a", "^1.0.0"), ("dep-b", "^1.0.0")]),
            fake("app-ext", "1.1.0", &[("dep-a", "^1.0.0"), ("dep-b", "^1.0.0")]),
            fake("dep-a", "1.0.0", &[]),
            fake("dep-a", "1.4.0", &[]),
            fake("dep-a", "2.0.0", &[]),
            fake("dep-b", "1.0.0", &[("dep-a", "^2.0.0")]),
            fake("@scope/tool", "0.3.0", &[]),
        ])
        .await;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("npm");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("package.json"), "{\n  \"name\": \"pi-extensions\",\n  \"private\": true\n}").unwrap();
        let client = client(&registry, &root);

        client
            .install_packages(&root, &["app-ext".to_string(), "@scope/tool@0.3.0".to_string()])
            .await
            .unwrap();

        let nm = root.join("node_modules");
        assert_eq!(installed_version(&nm.join("app-ext")).as_deref(), Some("1.1.0"));
        assert_eq!(installed_version(&nm.join("dep-a")).as_deref(), Some("1.4.0"));
        assert_eq!(installed_version(&nm.join("dep-b")).as_deref(), Some("1.0.0"));
        assert_eq!(
            installed_version(&nm.join("dep-b/node_modules/dep-a")).as_deref(),
            Some("2.0.0")
        );
        assert_eq!(installed_version(&nm.join("@scope/tool")).as_deref(), Some("0.3.0"));
        let manifest = read_json(&root.join("package.json")).unwrap();
        assert_eq!(manifest["dependencies"]["app-ext"], "^1.1.0");
        assert_eq!(manifest["dependencies"]["@scope/tool"], "0.3.0");
        assert_eq!(manifest["name"], "pi-extensions");

        // Pinning an older version replaces the installed one.
        client
            .install_packages(&root, &["app-ext@1.0.0".to_string()])
            .await
            .unwrap();
        assert_eq!(installed_version(&nm.join("app-ext")).as_deref(), Some("1.0.0"));

        client.uninstall(&root, "app-ext").await.unwrap();
        assert!(!nm.join("app-ext").exists());
        assert!(!nm.join("dep-a").exists());
        assert!(!nm.join("dep-b").exists());
        assert!(nm.join("@scope/tool").exists());
        let manifest = read_json(&root.join("package.json")).unwrap();
        assert!(manifest["dependencies"].get("app-ext").is_none());

        assert_eq!(client.latest_version("dep-a", None).await.unwrap(), "2.0.0");
        assert_eq!(client.latest_version("dep-a@^1.0.0", Some("^1.0.0")).await.unwrap(), "1.4.0");
    }

    #[tokio::test]
    async fn installs_package_dependencies_and_rejects_bad_integrity() {
        let mut corrupt = fake("broken", "1.0.0", &[]);
        corrupt.corrupt = true;
        let registry = serve_registry(vec![fake("dep-a", "1.0.0", &[]), corrupt]).await;
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("repo");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            r#"{"name":"repo","dependencies":{"dep-a":"1.0.0"},"devDependencies":{"broken":"1.0.0"},"peerDependencies":{"@earendil-works/pi-coding-agent":"*"}}"#,
        )
        .unwrap();
        let client = client(&registry, &pkg);
        client.install_dependencies(&pkg).await.unwrap();
        assert!(pkg.join("node_modules/dep-a/index.js").exists());
        assert!(!pkg.join("node_modules/broken").exists());
        assert!(!pkg.join("node_modules/@earendil-works").exists());

        let error = client
            .install_packages(&pkg, &["broken".to_string()])
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("Integrity check failed"), "{error:#}");
        let missing = client
            .install_packages(&pkg, &["does-not-exist".to_string()])
            .await
            .unwrap_err();
        assert!(format!("{missing:#}").contains("404"), "{missing:#}");
    }
}
