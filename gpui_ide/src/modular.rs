use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use crc32fast::Hasher as Crc32;
use semver::Version;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::compile::valid_luac_name;
use crate::metadata;
use crate::project::ProjectMeta;

const BUNDLE_MAGIC: u32 = 0x5055_4d4e;
const BUNDLE_HEADER_SIZE: usize = 32;
const SLOT_ENTRY_SIZE: usize = 32;
const MODULE_MAGIC: u32 = 0x444f_4d4c;
const BUNDLED_OLED_SHA256: &str =
    "2269ccf40c16089d5900c29acf4828e16f1b050d50690ffe02b04b2a12a11c10";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CatalogIdentity {
    pub id: String,
    pub version: String,
    pub firmware_id: String,
    pub firmware_version: String,
    pub target: String,
    pub core_abi: u16,
    pub module_format: u16,
    pub nmup_format: u16,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ModuleLayout {
    pub abi_version: u16,
    pub core_end: u32,
    pub core_api: u32,
    pub slot_base: u32,
    pub slot_size: u32,
    pub slot_count: u8,
    pub empty_slots_allowed: bool,
    pub vm_rebuild_required: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct ArtifactRef {
    path: String,
    length: u64,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
struct ManifestArtifacts {
    core: ArtifactRef,
    modules: ArtifactRef,
    index: ArtifactRef,
    api: ArtifactRef,
}

#[derive(Clone, Debug, Deserialize)]
struct CatalogHashAlgorithm {
    name: String,
    ordering: String,
    record: String,
    digest: String,
    file_count: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct CatalogManifest {
    schema: u32,
    firmware_id: String,
    firmware_version: String,
    target: String,
    core_abi: u16,
    module_format: u16,
    nmup_format: u16,
    layout: ModuleLayout,
    artifacts: ManifestArtifacts,
    catalog_sha256: String,
    catalog_hash_algorithm: CatalogHashAlgorithm,
    catalog_files: Vec<ArtifactRef>,
}

#[derive(Clone, Debug, Deserialize)]
struct ModuleDefinitionFile {
    schema: u32,
    catalog: CatalogIdentity,
    layout: ModuleLayout,
    modules: Vec<ModuleDefinition>,
    #[serde(default)]
    sets: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Deserialize)]
struct ModuleDefinition {
    name: String,
    version: String,
    lua_modules: Vec<String>,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    conflicts: Vec<String>,
    #[serde(default)]
    resident: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct ModuleIndex {
    schema: u32,
    catalog: CatalogIdentity,
    modules_definition_sha256: String,
    layout: ModuleLayout,
    modules: BTreeMap<String, IndexedModule>,
}

#[derive(Clone, Debug, Deserialize)]
struct IndexedModule {
    name: String,
    version: String,
    build_id: String,
    lua_modules: Vec<String>,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    conflicts: Vec<String>,
    #[serde(default)]
    resident: bool,
    variants: Vec<ModuleVariant>,
}

#[derive(Clone, Debug, Deserialize)]
struct ModuleVariant {
    slot: u8,
    address: u32,
    size: u32,
    crc16: u16,
    sha256: String,
    image: String,
    module: String,
    module_version: String,
    target: String,
    abi_version: u16,
    module_format: u16,
    build_id: String,
}

#[derive(Clone, Debug)]
pub struct Catalog {
    root: PathBuf,
    pub identity: CatalogIdentity,
    pub layout: ModuleLayout,
    pub catalog_sha256: String,
    definitions: BTreeMap<String, ModuleDefinition>,
    module_order: Vec<String>,
    sets: BTreeMap<String, Vec<String>>,
    index: ModuleIndex,
    lua_to_native: BTreeMap<String, String>,
    compiler_native_requirements: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedSlot {
    pub slot: u8,
    pub name: String,
    pub size: u32,
    pub crc32: u32,
}

#[derive(Clone, Debug)]
pub struct ModuleDeployment {
    pub modules: Vec<String>,
    pub slots: Vec<PlannedSlot>,
    pub bundle: Vec<u8>,
    pub bundle_sha256: String,
}

#[derive(Clone, Debug)]
pub struct ScriptSource {
    pub source_path: Option<PathBuf>,
    pub upload_name: String,
    pub source: String,
    pub entry: bool,
}

#[derive(Clone, Debug)]
pub struct PreparedRun {
    pub catalog: Catalog,
    pub deployment: ModuleDeployment,
    pub scripts: Vec<ScriptSource>,
    pub combined_source: String,
}

impl Catalog {
    pub fn load(manifest_path: &Path) -> Result<Self> {
        let manifest_path = manifest_path
            .canonicalize()
            .with_context(|| format!("找不到 catalog manifest: {}", manifest_path.display()))?;
        let release_dir = manifest_path
            .parent()
            .ok_or_else(|| anyhow!("catalog manifest 没有父目录"))?;
        let root = release_dir
            .parent()
            .ok_or_else(|| anyhow!("catalog manifest 必须位于 release 目录"))?
            .to_path_buf();
        if release_dir.file_name().and_then(|name| name.to_str()) != Some("release") {
            bail!("catalog manifest 必须位于 <catalog-root>/release/catalog_manifest.json");
        }

        let manifest_bytes = fs::read(&manifest_path)
            .with_context(|| format!("读取 {}", manifest_path.display()))?;
        let manifest: CatalogManifest = serde_json::from_slice(&manifest_bytes)
            .with_context(|| format!("解析 {}", manifest_path.display()))?;
        validate_manifest_header(&manifest)?;

        let modules_bytes = read_verified(&root, &manifest.artifacts.modules)?;
        let index_bytes = read_verified(&root, &manifest.artifacts.index)?;
        let api_bytes = read_verified(&root, &manifest.artifacts.api)?;
        let _core_bytes = read_verified(&root, &manifest.artifacts.core)?;

        let definitions_file: ModuleDefinitionFile =
            serde_json::from_slice(&modules_bytes).context("解析 modules.json")?;
        let index: ModuleIndex = serde_json::from_slice(&index_bytes).context("解析 index.json")?;
        let api = metadata::validate_api_bytes(&api_bytes, &manifest.artifacts.api.path)?;

        let identity = CatalogIdentity {
            id: manifest.firmware_id.clone(),
            version: manifest.firmware_version.clone(),
            firmware_id: manifest.firmware_id.clone(),
            firmware_version: manifest.firmware_version.clone(),
            target: manifest.target.clone(),
            core_abi: manifest.core_abi,
            module_format: manifest.module_format,
            nmup_format: manifest.nmup_format,
        };
        ensure_identity("modules.json", &definitions_file.catalog, &identity)?;
        ensure_identity("index.json", &index.catalog, &identity)?;
        if definitions_file.schema != 1 || index.schema != 2 {
            bail!(
                "不支持的 catalog schema: modules={}, index={}",
                definitions_file.schema,
                index.schema
            );
        }
        if definitions_file.layout != manifest.layout || index.layout != manifest.layout {
            bail!("manifest/modules/index 的 slot layout 不一致");
        }
        if index.modules_definition_sha256 != manifest.artifacts.modules.sha256 {
            bail!("index.json 引用的 modules.json SHA-256 不一致");
        }

        validate_catalog_files(&root, &manifest)?;
        let definitions = validate_definitions(&definitions_file, &index)?;
        validate_sets(&definitions_file.sets, &definitions)?;
        validate_index(&root, &identity, &manifest.layout, &definitions, &index)?;
        let (lua_to_native, compiler_native_requirements) =
            validate_api(&api, &identity, &manifest.catalog_sha256, &definitions)?;

        Ok(Self {
            root,
            identity,
            layout: manifest.layout,
            catalog_sha256: manifest.catalog_sha256,
            definitions,
            module_order: definitions_file
                .modules
                .iter()
                .map(|module| module.name.clone())
                .collect(),
            sets: definitions_file.sets,
            index,
            lua_to_native,
            compiler_native_requirements,
        })
    }

    pub fn plan_modules<I, S>(&self, requested: I) -> Result<ModuleDeployment>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut selected = BTreeSet::new();
        for name in requested {
            selected.insert(name.as_ref().to_string());
        }
        for (name, definition) in &self.definitions {
            if definition.resident {
                selected.insert(name.clone());
            }
        }

        let mut pending: Vec<String> = selected.iter().cloned().collect();
        while let Some(name) = pending.pop() {
            let definition = self
                .definitions
                .get(&name)
                .ok_or_else(|| anyhow!("catalog 中不存在原生模块 {name}"))?;
            for dependency in &definition.dependencies {
                if selected.insert(dependency.clone()) {
                    pending.push(dependency.clone());
                }
            }
        }
        for name in &selected {
            let definition = &self.definitions[name];
            for conflict in &definition.conflicts {
                if selected.contains(conflict) {
                    bail!("原生模块冲突: {name} 与 {conflict}");
                }
            }
        }
        if selected.len() > self.layout.slot_count as usize {
            bail!(
                "原生模块需要 {} 个槽，但设备只有 {} 个槽；选择: {}",
                selected.len(),
                self.layout.slot_count,
                selected.iter().cloned().collect::<Vec<_>>().join(", ")
            );
        }

        let modules = self
            .sets
            .values()
            .find(|modules| {
                modules.len() == selected.len()
                    && modules.iter().all(|module| selected.contains(module))
            })
            .cloned()
            .unwrap_or(stable_module_order(
                &selected,
                &self.definitions,
                &self.module_order,
            )?);
        validate_selected_order(&modules, &self.definitions)?;
        let mut images = Vec::new();
        let mut slots = Vec::new();
        for (slot, name) in modules.iter().enumerate() {
            let indexed = &self.index.modules[name];
            let variant = indexed
                .variants
                .iter()
                .find(|variant| variant.slot as usize == slot)
                .ok_or_else(|| anyhow!("模块 {name} 缺少 slot{slot} 变体"))?;
            let image_path = resolve_catalog_path(&self.root, &variant.image)?;
            let image = fs::read(&image_path)
                .with_context(|| format!("读取模块变体 {}", image_path.display()))?;
            validate_module_image(&image, name, slot as u8, variant, &self.layout)?;
            let image_crc32 = crc32(&image);
            slots.push(PlannedSlot {
                slot: slot as u8,
                name: name.clone(),
                size: image.len() as u32,
                crc32: image_crc32,
            });
            images.push(image);
        }
        let bundle = build_nmup(&self.layout, self.identity.nmup_format, &modules, &images)?;
        validate_nmup(
            &bundle,
            &self.layout,
            self.identity.nmup_format,
            self.identity.module_format,
        )?;
        Ok(ModuleDeployment {
            modules,
            slots,
            bundle_sha256: sha256_hex(&bundle),
            bundle,
        })
    }

    fn native_for_lua_module(&self, name: &str) -> Option<&str> {
        if name == "oled" {
            return Some("i2c");
        }
        self.lua_to_native.get(name).map(String::as_str)
    }

    fn compiler_native_modules_for_lua_module(&self, name: &str) -> Option<&[String]> {
        self.compiler_native_requirements.get(name).map(Vec::as_slice)
    }

    fn bundled_lua_source(&self, name: &str) -> Result<String> {
        if name != "oled" {
            bail!("未知的 IDE Lua 功能模块: {name}");
        }
        let path = resolve_catalog_path(&self.root, &format!("release/lua/{name}.lua"))?;
        let bytes =
            fs::read(&path).with_context(|| format!("读取 IDE Lua 功能模块 {}", path.display()))?;
        if sha256_hex(&bytes) != BUNDLED_OLED_SHA256 {
            bail!("IDE Lua 功能模块哈希不匹配: {}", path.display());
        }
        String::from_utf8(bytes)
            .with_context(|| format!("IDE Lua 功能模块不是 UTF-8: {}", path.display()))
    }
}

pub fn prepare_run(
    project_dir: Option<&Path>,
    meta: &ProjectMeta,
    overlays: &[(PathBuf, String)],
    fallback_source: &str,
) -> Result<PreparedRun> {
    let manifest_path = discover_manifest(project_dir, meta)?;
    let catalog = Catalog::load(&manifest_path)?;
    let scripts = if let Some(project_dir) = project_dir {
        prepare_project_sources(project_dir, meta, overlays, &catalog)?
    } else {
        prepare_single_source(fallback_source, meta, &catalog)?
    };
    let mut native = BTreeSet::new();
    let mut dynamic_require = false;
    let mut uses_oled = false;
    for script in &scripts {
        let scan = scan_source(&script.source);
        dynamic_require |= scan.dynamic_require;
        for module in scan.api_modules {
            uses_oled |= module == "oled";
            if let Some(native_module) = catalog.native_for_lua_module(&module) {
                native.insert(native_module.to_string());
            }
            if let Some(required) = catalog.compiler_native_modules_for_lua_module(&module) {
                native.extend(required.iter().cloned());
            }
        }
        for reference in scan.references {
            uses_oled |= reference == "oled";
            if let Some(native_module) = catalog.native_for_lua_module(&reference) {
                native.insert(native_module.to_string());
            }
        }
    }
    for module in &meta.native_modules {
        native.insert(module.clone());
    }
    if dynamic_require && meta.native_modules.is_empty() {
        bail!(
            "检测到无法静态求值的 require/runfile；请在 mspm0_lua.json 的 native_modules 中显式选择原生模块"
        );
    }
    let deployment = catalog.plan_modules(native.iter())?;
    let mut scripts = scripts;
    if uses_oled {
        let entry_index = scripts
            .iter()
            .position(|script| script.entry)
            .unwrap_or(scripts.len());
        scripts.insert(
            entry_index,
            ScriptSource {
                source_path: None,
                upload_name: "oled.luac".into(),
                source: catalog.bundled_lua_source("oled")?,
                entry: false,
            },
        );
    }
    let combined_source = scripts
        .iter()
        .map(|script| script.source.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(PreparedRun {
        catalog,
        deployment,
        scripts,
        combined_source,
    })
}

fn discover_manifest(project_dir: Option<&Path>, meta: &ProjectMeta) -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(configured) = meta.catalog_manifest.as_deref() {
        let configured = PathBuf::from(configured);
        let path = if configured.is_absolute() {
            configured
        } else {
            project_dir
                .ok_or_else(|| anyhow!("相对 catalog_manifest 需要工程目录"))?
                .join(configured)
        };
        if !path.is_file() {
            bail!("工程指定的 catalog manifest 不存在: {}", path.display());
        }
        return Ok(path);
    }
    if let Some(project_dir) = project_dir {
        candidates.push(project_dir.join("firmware/release/catalog_manifest.json"));
        candidates.push(project_dir.join("firmware/catalog_manifest.json"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("firmware/release/catalog_manifest.json"));
            candidates.push(dir.join("firmware/catalog_manifest.json"));
        }
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    candidates.push(manifest_dir.join("firmware/release/catalog_manifest.json"));
    if let Some(parent) = manifest_dir.parent() {
        candidates.push(parent.join("mspm0_lua/release/catalog_manifest.json"));
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            anyhow!("找不到模块化固件 catalog；请在工程 mspm0_lua.json 设置 catalog_manifest")
        })
}

fn prepare_single_source(
    source: &str,
    meta: &ProjectMeta,
    _catalog: &Catalog,
) -> Result<Vec<ScriptSource>> {
    if source.trim().is_empty() {
        bail!("源码为空");
    }
    if meta.target_luac != "main.luac" {
        bail!("模块化固件入口必须为 main.luac");
    }
    Ok(vec![ScriptSource {
        source_path: None,
        upload_name: "main.luac".into(),
        source: source.to_string(),
        entry: true,
    }])
}

fn prepare_project_sources(
    project_dir: &Path,
    meta: &ProjectMeta,
    overlays: &[(PathBuf, String)],
    catalog: &Catalog,
) -> Result<Vec<ScriptSource>> {
    if meta.target_luac != "main.luac" {
        bail!("模块化固件运行入口固定为 main.luac");
    }
    let main_rel = validate_source_relative_path(Path::new(&meta.main_source))?;
    let overlay_map: HashMap<PathBuf, &String> = overlays
        .iter()
        .filter_map(|(path, source)| {
            path.strip_prefix(project_dir)
                .ok()
                .map(|relative| (relative.to_path_buf(), source))
        })
        .collect();
    let mut paths = Vec::new();
    collect_lua_paths(project_dir, project_dir, &mut paths)?;
    if !paths.iter().any(|path| path == &main_rel) {
        bail!("工程入口不存在: {}", project_dir.join(&main_rel).display());
    }
    paths.sort();

    let mut by_name = BTreeMap::new();
    for relative in paths {
        let validated = validate_source_relative_path(&relative)?;
        let module_name = module_name_from_path(&validated)?;
        let upload_name = if validated == main_rel {
            "main.luac".to_string()
        } else {
            format!("{module_name}.luac")
        };
        if !valid_luac_name(&upload_name) {
            bail!("Lua 模块文件名不符合设备规则: {upload_name}");
        }
        if by_name.contains_key(&module_name) {
            bail!("重复 Lua 模块名: {module_name}");
        }
        let source = if let Some(source) = overlay_map.get(&validated) {
            (*source).clone()
        } else {
            fs::read_to_string(project_dir.join(&validated))
                .with_context(|| format!("读取 Lua 源码 {}", validated.display()))?
        };
        by_name.insert(
            module_name,
            ScriptSource {
                source_path: Some(project_dir.join(&validated)),
                upload_name,
                source,
                entry: validated == main_rel,
            },
        );
    }
    let main_name = module_name_from_path(&main_rel)?;

    let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut dynamic = false;
    for (name, script) in &by_name {
        let scan = scan_source(&script.source);
        dynamic |= scan.dynamic_require;
        let mut dependencies = BTreeSet::new();
        for reference in scan.references {
            if catalog.native_for_lua_module(&reference).is_some() {
                continue;
            }
            let normalized = normalize_lua_reference(&reference)?;
            if !by_name.contains_key(&normalized) {
                bail!("{name}.lua 引用了缺失的 Lua 模块 {reference}");
            }
            if normalized != *name {
                dependencies.insert(normalized);
            }
        }
        graph.insert(name.clone(), dependencies);
    }

    let selected = if dynamic {
        by_name.keys().cloned().collect::<BTreeSet<_>>()
    } else {
        reachable_modules(&main_name, &graph)?
    };
    let order = stable_script_order(&selected, &graph, &main_name)?;
    Ok(order
        .into_iter()
        .map(|name| {
            by_name
                .remove(&name)
                .expect("script order references known name")
        })
        .collect())
}

fn collect_lua_paths(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("读取工程目录 {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_lua_paths(root, &path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("lua") {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| anyhow!("源码路径越出工程目录: {}", path.display()))?;
            out.push(relative.to_path_buf());
        }
    }
    Ok(())
}

fn validate_source_relative_path(path: &Path) -> Result<PathBuf> {
    let components: Vec<_> = path.components().collect();
    if components.is_empty()
        || !components
            .iter()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        bail!("Lua 源码路径必须位于工程目录内: {}", path.display());
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("lua") {
        bail!("Lua 源码必须以 .lua 结尾: {}", path.display());
    }
    Ok(path.to_path_buf())
}

fn module_name_from_path(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    let without_extension = path.with_extension("");
    for component in without_extension.components() {
        let Component::Normal(value) = component else {
            bail!("Lua 模块路径无效: {}", path.display());
        };
        let value = value
            .to_str()
            .ok_or_else(|| anyhow!("Lua 模块路径不是 UTF-8: {}", path.display()))?;
        parts.push(value);
    }
    let name = parts.join(".");
    if !valid_luac_name(&format!("{name}.luac")) {
        bail!("Lua 模块路径映射后的设备文件名无效: {name}.luac");
    }
    Ok(name)
}

fn normalize_lua_reference(reference: &str) -> Result<String> {
    let normalized_separators = reference.trim().replace(['/', '\\'], ".");
    let mut name = normalized_separators.as_str();
    if let Some(value) = name.strip_suffix(".luac") {
        name = value;
    } else if let Some(value) = name.strip_suffix(".lua") {
        name = value;
    }
    if name.is_empty()
        || name.len() > 23
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        bail!("Lua 模块引用不符合设备文件名规则: {reference}");
    }
    Ok(name.to_string())
}

fn reachable_modules(
    main: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
) -> Result<BTreeSet<String>> {
    if !graph.contains_key(main) {
        bail!("工程图中缺少入口模块 {main}");
    }
    let mut reachable = BTreeSet::new();
    let mut pending = vec![main.to_string()];
    while let Some(name) = pending.pop() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        let dependencies = graph
            .get(&name)
            .ok_or_else(|| anyhow!("Lua 依赖图缺少 {name}"))?;
        pending.extend(dependencies.iter().cloned());
    }
    Ok(reachable)
}

fn stable_script_order(
    selected: &BTreeSet<String>,
    graph: &BTreeMap<String, BTreeSet<String>>,
    main: &str,
) -> Result<Vec<String>> {
    let mut remaining = selected.clone();
    let mut emitted = BTreeSet::new();
    let mut order = Vec::new();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .find(|name| {
                graph
                    .get(*name)
                    .map(|deps| {
                        deps.iter()
                            .filter(|dep| selected.contains(*dep))
                            .all(|dep| emitted.contains(dep))
                    })
                    .unwrap_or(false)
            })
            .cloned()
            .or_else(|| remaining.iter().next().cloned())
            .ok_or_else(|| anyhow!("无法排序 Lua 依赖"))?;
        remaining.remove(&ready);
        emitted.insert(ready.clone());
        order.push(ready);
    }
    if let Some(position) = order.iter().position(|name| name == main) {
        let entry = order.remove(position);
        order.push(entry);
    }
    Ok(order)
}

#[derive(Default)]
struct SourceScan {
    references: BTreeSet<String>,
    api_modules: BTreeSet<String>,
    dynamic_require: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LuaToken {
    Ident(String),
    String(String),
    Symbol(char),
}

fn scan_source(source: &str) -> SourceScan {
    let tokens = lex_lua(source);
    let mut scan = SourceScan::default();
    for index in 0..tokens.len() {
        let LuaToken::Ident(name) = &tokens[index] else {
            continue;
        };
        if index + 2 < tokens.len()
            && tokens[index + 1] == LuaToken::Symbol('.')
            && matches!(tokens[index + 2], LuaToken::Ident(_))
        {
            scan.api_modules.insert(name.clone());
        }
        if name != "require" && name != "runfile" {
            continue;
        }
        let mut argument = index + 1;
        let parenthesized = tokens.get(argument) == Some(&LuaToken::Symbol('('));
        if parenthesized {
            argument += 1;
        }
        match tokens.get(argument) {
            Some(LuaToken::String(value)) => {
                scan.references.insert(value.clone());
            }
            _ => scan.dynamic_require = true,
        }
    }
    scan
}

fn lex_lua(source: &str) -> Vec<LuaToken> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if byte == b'-' && bytes.get(index + 1) == Some(&b'-') {
            index += 2;
            if bytes.get(index) == Some(&b'[') && bytes.get(index + 1) == Some(&b'[') {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b']' && bytes[index + 1] == b']')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            } else {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            continue;
        }
        if byte == b'\'' || byte == b'"' {
            let quote = byte;
            index += 1;
            let mut value = String::new();
            while index < bytes.len() {
                let current = bytes[index];
                if current == quote {
                    index += 1;
                    break;
                }
                if current == b'\\' && index + 1 < bytes.len() {
                    index += 1;
                    value.push(bytes[index] as char);
                    index += 1;
                    continue;
                }
                value.push(current as char);
                index += 1;
            }
            tokens.push(LuaToken::String(value));
            continue;
        }
        if byte == b'[' && bytes.get(index + 1) == Some(&b'[') {
            index += 2;
            let start = index;
            while index + 1 < bytes.len() && !(bytes[index] == b']' && bytes[index + 1] == b']') {
                index += 1;
            }
            tokens.push(LuaToken::String(
                String::from_utf8_lossy(&bytes[start..index]).into_owned(),
            ));
            index = (index + 2).min(bytes.len());
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            tokens.push(LuaToken::Ident(
                String::from_utf8_lossy(&bytes[start..index]).into_owned(),
            ));
            continue;
        }
        tokens.push(LuaToken::Symbol(byte as char));
        index += 1;
    }
    tokens
}

fn validate_manifest_header(manifest: &CatalogManifest) -> Result<()> {
    if manifest.schema != 1 {
        bail!("不支持 catalog manifest schema {}", manifest.schema);
    }
    Version::parse(&manifest.firmware_version).context("firmware_version 不是语义版本")?;
    if manifest.layout.abi_version != manifest.core_abi {
        bail!("manifest layout ABI 与 Core ABI 不一致");
    }
    if manifest.layout.slot_count == 0 || manifest.layout.slot_count > 8 {
        bail!("无效 slot_count {}", manifest.layout.slot_count);
    }
    if manifest.layout.slot_size == 0 {
        bail!("slot_size 不能为 0");
    }
    if manifest.catalog_sha256.len() != 64 {
        bail!("catalog SHA-256 格式无效");
    }
    let algorithm = &manifest.catalog_hash_algorithm;
    if algorithm.name != "sha256-path-length-content-v1"
        || algorithm.ordering != "ascending UTF-8 relative POSIX path bytes"
        || algorithm.record
            != "utf8(path) || NUL || ascii(decimal_length) || NUL || lowercase_sha256 || LF"
        || algorithm.digest != "SHA-256 of the concatenated records"
    {
        bail!("不支持的 catalog 哈希规范");
    }
    Ok(())
}

fn ensure_identity(
    label: &str,
    actual: &CatalogIdentity,
    expected: &CatalogIdentity,
) -> Result<()> {
    if actual != expected {
        bail!("{label} 的 catalog 身份与 manifest 不一致");
    }
    Ok(())
}

fn validate_catalog_files(root: &Path, manifest: &CatalogManifest) -> Result<()> {
    if manifest.catalog_files.len() != manifest.catalog_hash_algorithm.file_count {
        bail!("catalog 文件数与 manifest 算法声明不一致");
    }
    let mut previous: Option<&[u8]> = None;
    let mut canonical = Sha256::new();
    let mut paths = HashSet::new();
    for artifact in &manifest.catalog_files {
        if artifact.path.contains('\\') {
            bail!("catalog 路径必须使用 POSIX 分隔符: {}", artifact.path);
        }
        if let Some(prev) = previous {
            if prev >= artifact.path.as_bytes() {
                bail!("catalog_files 未严格按 UTF-8 路径排序或存在重复");
            }
        }
        previous = Some(artifact.path.as_bytes());
        if !paths.insert(artifact.path.clone()) {
            bail!("catalog_files 重复路径: {}", artifact.path);
        }
        let bytes = read_verified(root, artifact)?;
        canonical.update(artifact.path.as_bytes());
        canonical.update([0]);
        canonical.update(bytes.len().to_string().as_bytes());
        canonical.update([0]);
        canonical.update(artifact.sha256.to_ascii_lowercase().as_bytes());
        canonical.update(b"\n");
    }
    let digest = format!("{:x}", canonical.finalize());
    if digest != manifest.catalog_sha256 {
        bail!(
            "catalog SHA-256 不一致: manifest={}, actual={digest}",
            manifest.catalog_sha256
        );
    }
    Ok(())
}

fn validate_definitions(
    file: &ModuleDefinitionFile,
    index: &ModuleIndex,
) -> Result<BTreeMap<String, ModuleDefinition>> {
    let mut definitions = BTreeMap::new();
    for definition in &file.modules {
        validate_module_name(&definition.name)?;
        Version::parse(&definition.version)
            .with_context(|| format!("模块 {} 版本无效", definition.name))?;
        if definitions
            .insert(definition.name.clone(), definition.clone())
            .is_some()
        {
            bail!("modules.json 重复模块 {}", definition.name);
        }
        if definition.lua_modules.is_empty() {
            bail!("模块 {} 未声明 lua_modules", definition.name);
        }
    }
    if definitions.len() != index.modules.len() {
        bail!("modules.json 与 index.json 模块数量不一致");
    }
    for definition in definitions.values() {
        for dependency in &definition.dependencies {
            if !definitions.contains_key(dependency) {
                bail!("模块 {} 依赖未知模块 {dependency}", definition.name);
            }
        }
        for conflict in &definition.conflicts {
            if !definitions.contains_key(conflict) || conflict == &definition.name {
                bail!("模块 {} 的冲突项无效: {conflict}", definition.name);
            }
        }
    }
    Ok(definitions)
}

fn validate_sets(
    sets: &BTreeMap<String, Vec<String>>,
    definitions: &BTreeMap<String, ModuleDefinition>,
) -> Result<()> {
    for (set, modules) in sets {
        let mut seen = HashSet::new();
        for module in modules {
            if !definitions.contains_key(module) {
                bail!("模块集合 {set} 引用了未知模块 {module}");
            }
            if !seen.insert(module) {
                bail!("模块集合 {set} 重复模块 {module}");
            }
        }
    }
    Ok(())
}

fn validate_index(
    root: &Path,
    identity: &CatalogIdentity,
    layout: &ModuleLayout,
    definitions: &BTreeMap<String, ModuleDefinition>,
    index: &ModuleIndex,
) -> Result<()> {
    for (name, definition) in definitions {
        let indexed = index
            .modules
            .get(name)
            .ok_or_else(|| anyhow!("index.json 缺少模块 {name}"))?;
        if indexed.name != *name
            || indexed.version != definition.version
            || indexed.lua_modules != definition.lua_modules
            || indexed.dependencies != definition.dependencies
            || indexed.conflicts != definition.conflicts
            || indexed.resident != definition.resident
        {
            bail!("index.json 模块 {name} 与 modules.json 不一致");
        }
        if indexed.variants.len() != layout.slot_count as usize {
            bail!("模块 {name} 的 slot 变体数量不完整");
        }
        let mut slots = HashSet::new();
        for variant in &indexed.variants {
            if !slots.insert(variant.slot) || variant.slot >= layout.slot_count {
                bail!("模块 {name} 的 slot 变体重复或越界");
            }
            if variant.module != *name
                || variant.module_version != definition.version
                || variant.target != identity.target
                || variant.abi_version != identity.core_abi
                || variant.module_format != identity.module_format
                || variant.build_id != indexed.build_id
                || variant.address != layout.slot_base + u32::from(variant.slot) * layout.slot_size
                || variant.size > layout.slot_size
            {
                bail!("模块 {name} slot{} 元数据不一致", variant.slot);
            }
            let artifact = ArtifactRef {
                path: variant.image.clone(),
                length: u64::from(variant.size),
                sha256: variant.sha256.clone(),
            };
            let image = read_verified(root, &artifact)?;
            validate_module_image(&image, name, variant.slot, variant, layout)?;
        }
    }
    Ok(())
}

fn validate_api(
    api: &Value,
    identity: &CatalogIdentity,
    catalog_sha256: &str,
    definitions: &BTreeMap<String, ModuleDefinition>,
) -> Result<(BTreeMap<String, String>, BTreeMap<String, Vec<String>>)> {
    if api.get("id").and_then(Value::as_str) != Some(identity.id.as_str())
        || api.get("version").and_then(Value::as_str) != Some(identity.version.as_str())
        || api.pointer("/firmware/version").and_then(Value::as_str)
            != Some(identity.firmware_version.as_str())
        || api.pointer("/firmware/build_id").and_then(Value::as_str) != Some(catalog_sha256)
    {
        bail!("API 元数据身份与 catalog 不一致");
    }
    let modules = api
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("API 元数据缺少 modules"))?;
    let mut mapping = BTreeMap::new();
    let mut compiler_requirements = BTreeMap::new();
    for module in modules {
        let lua_name = module
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("API module 缺少 name"))?;
        if module
            .pointer("/extensions/mspm0.core_resident")
            .and_then(Value::as_bool)
            == Some(true)
        {
            continue;
        }
        if let Some(required) = module
            .pointer("/extensions/mspm0.compiler_injected/required_native_modules")
            .and_then(Value::as_array)
        {
            let mut requirements = Vec::new();
            for native in required {
                let native = native
                    .as_str()
                    .ok_or_else(|| anyhow!("API compiler-injected module {lua_name} has non-string dependency"))?;
                if !definitions.contains_key(native) {
                    bail!("API compiler-injected module {lua_name} references unknown native module {native}");
                }
                requirements.push(native.to_string());
            }
            if requirements.is_empty()
                || compiler_requirements
                    .insert(lua_name.to_string(), requirements)
                    .is_some()
            {
                bail!("API compiler-injected module {lua_name} has invalid dependencies");
            }
            continue;
        }
        if module
            .pointer("/extensions/mspm0.lua_library/uploaded_with_project")
            .and_then(Value::as_bool)
            == Some(true)
        {
            let files = module
                .pointer("/extensions/mspm0.lua_library/files")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("API Lua library {lua_name} 缺少 files"))?;
            if files.is_empty() || files.iter().any(|file| file.as_str().is_none()) {
                bail!("API Lua library {lua_name} 包含无效 files");
            }
            continue;
        }
        let native = module
            .pointer("/extensions/mspm0.native_module/id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("API module {lua_name} 缺少 mspm0.native_module.id"))?;
        let definition = definitions
            .get(native)
            .ok_or_else(|| anyhow!("API module {lua_name} 引用未知原生模块 {native}"))?;
        if !definition.lua_modules.iter().any(|name| name == lua_name) {
            bail!("API module {lua_name} 与 modules.json 的 {native} 映射不一致");
        }
        if mapping
            .insert(lua_name.to_string(), native.to_string())
            .is_some()
        {
            bail!("API 元数据重复 Lua module {lua_name}");
        }
    }
    for definition in definitions.values() {
        for lua_module in &definition.lua_modules {
            if mapping.get(lua_module) != Some(&definition.name) {
                bail!("API 元数据缺少 Lua module {lua_module}");
            }
        }
    }
    Ok((mapping, compiler_requirements))
}

fn stable_module_order(
    selected: &BTreeSet<String>,
    definitions: &BTreeMap<String, ModuleDefinition>,
    catalog_order: &[String],
) -> Result<Vec<String>> {
    let mut remaining = selected.clone();
    let mut emitted = BTreeSet::new();
    let mut order = Vec::new();
    while !remaining.is_empty() {
        let Some(ready) = catalog_order
            .iter()
            .filter(|name| remaining.contains(*name))
            .find(|name| {
                definitions[*name]
                    .dependencies
                    .iter()
                    .all(|dependency| emitted.contains(dependency))
            })
            .cloned()
        else {
            bail!(
                "原生模块依赖存在循环: {}",
                remaining.into_iter().collect::<Vec<_>>().join(", ")
            );
        };
        remaining.remove(&ready);
        emitted.insert(ready.clone());
        order.push(ready);
    }
    Ok(order)
}

fn validate_selected_order(
    modules: &[String],
    definitions: &BTreeMap<String, ModuleDefinition>,
) -> Result<()> {
    let mut emitted = HashSet::new();
    for module in modules {
        let definition = &definitions[module];
        if let Some(missing) = definition
            .dependencies
            .iter()
            .find(|dependency| !emitted.contains(*dependency))
        {
            bail!("模块顺序无效：{module} 必须位于依赖 {missing} 之后");
        }
        emitted.insert(module.clone());
    }
    Ok(())
}

fn build_nmup(
    layout: &ModuleLayout,
    format: u16,
    names: &[String],
    images: &[Vec<u8>],
) -> Result<Vec<u8>> {
    if names.len() != images.len() || names.len() > layout.slot_count as usize {
        bail!("NMUP 模块/镜像数量不一致");
    }
    let header_size = BUNDLE_HEADER_SIZE + layout.slot_count as usize * SLOT_ENTRY_SIZE;
    let total_size = header_size + images.iter().map(Vec::len).sum::<usize>();
    let max_size = header_size + layout.slot_count as usize * layout.slot_size as usize;
    if total_size > max_size || total_size > u32::MAX as usize {
        bail!("NMUP 总长度越界");
    }
    let mut bundle = Vec::with_capacity(total_size);
    push_u32(&mut bundle, BUNDLE_MAGIC);
    push_u16(&mut bundle, format);
    push_u16(&mut bundle, layout.abi_version);
    push_u16(&mut bundle, header_size as u16);
    bundle.push(layout.slot_count);
    bundle.push(names.len() as u8);
    push_u32(&mut bundle, total_size as u32);
    push_u32(&mut bundle, 0);
    bundle.extend_from_slice(&[0; 12]);

    let mut offset = header_size as u32;
    for slot in 0..layout.slot_count as usize {
        if slot < names.len() {
            let name = &names[slot];
            validate_module_name(name)?;
            let image = &images[slot];
            if image.len() <= 32 || image.len() > layout.slot_size as usize {
                bail!("slot{slot} 镜像长度无效");
            }
            bundle.push(1);
            bundle.push(slot as u8);
            push_u16(&mut bundle, 0);
            push_u32(&mut bundle, image.len() as u32);
            push_u32(&mut bundle, offset);
            push_u32(&mut bundle, crc32(image));
            push_u16(&mut bundle, read_u16(image, 12)?);
            push_u16(&mut bundle, 0);
            let mut raw_name = [0u8; 8];
            raw_name[..name.len()].copy_from_slice(name.as_bytes());
            bundle.extend_from_slice(&raw_name);
            push_u32(&mut bundle, 0);
            offset += image.len() as u32;
        } else {
            bundle.push(0);
            bundle.push(slot as u8);
            bundle.extend_from_slice(&[0; 30]);
        }
    }
    for image in images {
        bundle.extend_from_slice(image);
    }
    let checksum = crc32(&bundle);
    bundle[16..20].copy_from_slice(&checksum.to_le_bytes());
    Ok(bundle)
}

fn validate_nmup(
    data: &[u8],
    layout: &ModuleLayout,
    format: u16,
    module_format: u16,
) -> Result<()> {
    let expected_header = BUNDLE_HEADER_SIZE + layout.slot_count as usize * SLOT_ENTRY_SIZE;
    if data.len() < expected_header
        || read_u32(data, 0)? != BUNDLE_MAGIC
        || read_u16(data, 4)? != format
        || read_u16(data, 6)? != layout.abi_version
        || read_u16(data, 8)? as usize != expected_header
        || data[10] != layout.slot_count
        || read_u32(data, 12)? as usize != data.len()
    {
        bail!("NMUP header 校验失败");
    }
    if data[20..32].iter().any(|byte| *byte != 0) {
        bail!("NMUP header reserved 字段非零");
    }
    let expected_crc = read_u32(data, 16)?;
    let mut crc_data = data.to_vec();
    crc_data[16..20].fill(0);
    if crc32(&crc_data) != expected_crc {
        bail!("NMUP bundle CRC32 校验失败");
    }
    let mut present = 0u8;
    let mut expected_offset = expected_header;
    let mut names = HashSet::new();
    for slot in 0..layout.slot_count as usize {
        let base = BUNDLE_HEADER_SIZE + slot * SLOT_ENTRY_SIZE;
        let is_present = data[base];
        if data[base + 1] as usize != slot
            || !matches!(is_present, 0 | 1)
            || read_u16(data, base + 2)? != 0
            || read_u16(data, base + 18)? != 0
            || read_u32(data, base + 28)? != 0
        {
            bail!("NMUP slot{slot} entry 校验失败");
        }
        if is_present == 0 {
            if data[base + 4..base + SLOT_ENTRY_SIZE]
                .iter()
                .any(|byte| *byte != 0)
            {
                bail!("NMUP slot{slot} 空 entry 含非零字段");
            }
            continue;
        }
        present += 1;
        let size = read_u32(data, base + 4)? as usize;
        let offset = read_u32(data, base + 8)? as usize;
        if size <= 32
            || size > layout.slot_size as usize
            || offset != expected_offset
            || offset
                .checked_add(size)
                .filter(|end| *end <= data.len())
                .is_none()
        {
            bail!("NMUP slot{slot} payload 越界");
        }
        let raw_name = &data[base + 20..base + 28];
        let end = raw_name.iter().position(|byte| *byte == 0).unwrap_or(8);
        let name = std::str::from_utf8(&raw_name[..end]).context("NMUP 模块名不是 ASCII")?;
        validate_module_name(name)?;
        if raw_name[end..].iter().any(|byte| *byte != 0) || !names.insert(name.to_string()) {
            bail!("NMUP slot{slot} 模块名重复或 padding 无效");
        }
        let image = &data[offset..offset + size];
        if crc32(image) != read_u32(data, base + 12)?
            || read_u16(image, 12)? != read_u16(data, base + 16)?
            || read_u32(image, 0)? != MODULE_MAGIC
            || read_u16(image, 4)? != module_format
            || read_u16(image, 6)? != layout.abi_version
            || read_u32(image, 8)? as usize != size
        {
            bail!("NMUP slot{slot} 模块镜像校验失败");
        }
        expected_offset += size;
    }
    if present != data[11] || expected_offset != data.len() {
        bail!("NMUP selected_count/总长度不一致");
    }
    Ok(())
}

fn validate_module_image(
    image: &[u8],
    name: &str,
    slot: u8,
    variant: &ModuleVariant,
    layout: &ModuleLayout,
) -> Result<()> {
    if image.len() != variant.size as usize
        || sha256_hex(image) != variant.sha256
        || image.len() <= 32
        || read_u32(image, 0)? != MODULE_MAGIC
        || read_u16(image, 4)? != variant.module_format
        || read_u16(image, 6)? != variant.abi_version
        || read_u32(image, 8)? as usize != image.len()
        || read_u16(image, 12)? != variant.crc16
        || read_u16(image, 14)? != 32
    {
        bail!("模块 {name} slot{slot} 镜像 header/哈希无效");
    }
    let raw_name = &image[24..32];
    let end = raw_name.iter().position(|byte| *byte == 0).unwrap_or(8);
    if std::str::from_utf8(&raw_name[..end]).ok() != Some(name)
        || raw_name[end..].iter().any(|byte| *byte != 0)
    {
        bail!("模块 {name} slot{slot} 镜像名称无效");
    }
    if crc16_modbus(&image[32..]) != variant.crc16 {
        bail!("模块 {name} slot{slot} payload CRC16 无效");
    }
    let address = layout.slot_base + u32::from(slot) * layout.slot_size;
    let init = read_u32(image, 16)?;
    let deinit = read_u32(image, 20)?;
    if !thumb_pointer_in_image(init, address, image.len() as u32)
        || (deinit != 0 && !thumb_pointer_in_image(deinit, address, image.len() as u32))
    {
        bail!("模块 {name} slot{slot} init/deinit 地址越界");
    }
    Ok(())
}

fn thumb_pointer_in_image(pointer: u32, address: u32, size: u32) -> bool {
    pointer & 1 == 1
        && (pointer & !1) >= address.saturating_add(32)
        && (pointer & !1) < address.saturating_add(size)
}

fn validate_module_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 7
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        bail!("原生模块名不符合 NMUP 规则: {name}");
    }
    Ok(())
}

fn read_verified(root: &Path, artifact: &ArtifactRef) -> Result<Vec<u8>> {
    let path = resolve_catalog_path(root, &artifact.path)?;
    let bytes = fs::read(&path).with_context(|| format!("读取 catalog 文件 {}", path.display()))?;
    if bytes.len() as u64 != artifact.length {
        bail!(
            "catalog 文件长度不一致: {}，期望 {}，实际 {}",
            artifact.path,
            artifact.length,
            bytes.len()
        );
    }
    let actual = sha256_hex(&bytes);
    if artifact.sha256 != artifact.sha256.to_ascii_lowercase()
        || artifact.sha256.len() != 64
        || actual != artifact.sha256
    {
        bail!(
            "catalog 文件 SHA-256 不一致: {}，期望 {}，实际 {actual}",
            artifact.path,
            artifact.sha256
        );
    }
    Ok(bytes)
}

fn resolve_catalog_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("catalog 路径不是安全相对路径: {relative}");
    }
    Ok(root.join(path))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut hasher = Crc32::new();
    hasher.update(bytes);
    hasher.finalize()
}

fn crc16_modbus(bytes: &[u8]) -> u16 {
    let mut crc = 0xffffu16;
    for byte in bytes {
        crc ^= u16::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xa001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16> {
    let bytes: [u8; 2] = data
        .get(offset..offset + 2)
        .ok_or_else(|| anyhow!("二进制字段越界"))?
        .try_into()
        .expect("slice length checked");
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
    let bytes: [u8; 4] = data
        .get(offset..offset + 4)
        .ok_or_else(|| anyhow!("二进制字段越界"))?
        .try_into()
        .expect("slice length checked");
    Ok(u32::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn firmware_root() -> Option<PathBuf> {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|parent| parent.join("mspm0_lua"))
            .filter(|path| path.is_dir())
    }

    #[test]
    fn source_scan_ignores_comments_and_strings() {
        let scan = scan_source(
            r#"
-- require("fake")
local text = "gpio.toggle('PA0')"
local x = require("helper")
i2c.open(0, 100000, "PA10", "PA11")
runfile(name)
"#,
        );
        assert_eq!(scan.references, BTreeSet::from(["helper".to_string()]));
        assert_eq!(scan.api_modules, BTreeSet::from(["i2c".to_string()]));
        assert!(scan.dynamic_require);
    }

    #[test]
    fn event_dispatcher_selects_tmr_native_module() {
        let Some(root) = firmware_root() else {
            return;
        };
        let catalog = Catalog::load(&root.join("release/catalog_manifest.json")).unwrap();
        let expected = vec!["tmr".to_string()];
        assert_eq!(
            catalog.compiler_native_modules_for_lua_module("event"),
            Some(expected.as_slice())
        );
    }

    #[test]
    fn real_catalog_builds_exact_i2c_vector() {
        let Some(root) = firmware_root() else {
            return;
        };
        let catalog = Catalog::load(&root.join("release/catalog_manifest.json")).unwrap();
        assert_eq!(catalog.catalog_sha256.len(), 64);
        assert!(catalog.catalog_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let plan = catalog.plan_modules(["i2c"]).unwrap();
        let expected = fs::read(root.join("release/test-vectors/i2c-only-valid.nmup")).unwrap();
        assert_eq!(plan.bundle, expected);
        assert_eq!(plan.modules, vec!["i2c"]);
        assert_eq!(plan.slots[0].name, "i2c");
    }

    #[test]
    fn real_catalog_builds_exact_full_vector() {
        let Some(root) = firmware_root() else {
            return;
        };
        let catalog = Catalog::load(&root.join("release/catalog_manifest.json")).unwrap();
        let requested = ["gpio", "tmr", "pwm", "adc", "i2c", "spi", "uart", "can"];
        let plan = catalog.plan_modules(requested).unwrap();
        let expected = fs::read(root.join("release/test-vectors/full-valid.nmup")).unwrap();
        assert_eq!(plan.bundle, expected);
        assert_eq!(plan.modules, requested);
    }

    #[test]
    fn rejects_corrupted_bundle_vector() {
        let Some(root) = firmware_root() else {
            return;
        };
        let catalog = Catalog::load(&root.join("release/catalog_manifest.json")).unwrap();
        let corrupt = fs::read(root.join("release/test-vectors/i2c-only-bundle-crc.nmup")).unwrap();
        assert!(validate_nmup(
            &corrupt,
            &catalog.layout,
            catalog.identity.nmup_format,
            catalog.identity.module_format,
        )
        .is_err());
    }

    #[test]
    fn real_example_orders_dependencies_before_entry() {
        let Some(root) = firmware_root() else {
            return;
        };
        let project = root.join("examples/ide_oled123");
        let meta = ProjectMeta {
            name: "ide_oled123".into(),
            main_source: "main.lua".into(),
            target_luac: "main.luac".into(),
            target: None,
            catalog_manifest: Some("../../release/catalog_manifest.json".into()),
            native_modules: Vec::new(),
        };
        let prepared = prepare_run(Some(&project), &meta, &[], "").unwrap();
        let names: Vec<_> = prepared
            .scripts
            .iter()
            .map(|script| script.upload_name.as_str())
            .collect();
        assert_eq!(names, ["oled.luac", "main.luac"]);
        assert_eq!(prepared.deployment.modules, ["i2c"]);
    }

    #[test]
    fn project_dependency_walk_is_transitive_and_ignores_unreachable_lua() {
        let Some(root) = firmware_root() else {
            return;
        };
        let project = std::env::temp_dir().join(format!("mspm0-lua-deps-{}", std::process::id()));
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("main.lua"), "local a=require('A'); a.run()\n").unwrap();
        fs::write(
            project.join("A.lua"),
            "local b=require('B'); return {run=b.run}\n",
        )
        .unwrap();
        fs::write(
            project.join("B.lua"),
            "return {run=function() gpio.set('PA0', true) end}\n",
        )
        .unwrap();
        fs::write(
            project.join("unused.lua"),
            "spi.valid(0, 'PA0', 'PA1', 'PA2')\n",
        )
        .unwrap();
        let meta = ProjectMeta {
            name: "deps".into(),
            main_source: "main.lua".into(),
            target_luac: "main.luac".into(),
            target: None,
            catalog_manifest: Some(
                root.join("release/catalog_manifest.json")
                    .to_string_lossy()
                    .into_owned(),
            ),
            native_modules: Vec::new(),
        };
        let prepared = prepare_run(Some(&project), &meta, &[], "").unwrap();
        let names: Vec<_> = prepared
            .scripts
            .iter()
            .map(|script| script.upload_name.as_str())
            .collect();
        assert_eq!(names, ["B.luac", "A.luac", "main.luac"]);
        assert_eq!(prepared.deployment.modules, ["gpio"]);
        fs::remove_dir_all(project).unwrap();
    }
}
