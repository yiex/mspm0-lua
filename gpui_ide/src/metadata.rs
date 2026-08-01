use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use semver::{Version, VersionReq};
use serde::Deserialize;
use serde_json::Value;

use crate::project::ProjectTarget;

const CHIP_SCHEMA: &str = include_str!("../docs/metadata-standard/schemas/chip.schema.json");
const BOARD_SCHEMA: &str = include_str!("../docs/metadata-standard/schemas/board.schema.json");
const API_SCHEMA: &str = include_str!("../docs/metadata-standard/schemas/api.schema.json");

#[derive(Clone, Debug)]
pub struct ProfileCompletion {
    pub label: String,
    pub insert: String,
    pub detail: String,
}

#[derive(Clone, Debug)]
struct Capability {
    function: String,
    class: String,
    peripheral: Option<String>,
    signal: Option<String>,
    route: Option<String>,
}

#[derive(Clone, Debug)]
struct ChipPin {
    capabilities: Vec<Capability>,
}

#[derive(Clone, Debug)]
struct Peripheral {
    id: String,
    class: String,
    instance: i64,
    signals: HashSet<String>,
}

#[derive(Clone, Debug)]
struct BoardPin {
    id: String,
    name: String,
    aliases: Vec<String>,
    chip_pin: String,
    available: bool,
    allow_classes: Option<HashSet<String>>,
    deny_functions: HashSet<String>,
    preference: i64,
    reserved: Option<(String, String)>,
    occupied: Option<(String, bool)>,
}

#[derive(Clone, Debug, Default)]
struct CapabilityRequirement {
    class: Option<String>,
    function: Option<String>,
    peripheral: Option<String>,
    signal: Option<String>,
    route: Option<String>,
}

#[derive(Clone, Debug)]
struct ResourceRequirement {
    kind: String,
    scope: String,
    capability: CapabilityRequirement,
    bindings: HashMap<String, String>,
    allow_reserved: bool,
    allow_releasable: bool,
}

#[derive(Clone, Debug)]
struct ApiParam {
    name: String,
    ty: String,
    optional: bool,
    variadic: bool,
    resource: Option<ResourceRequirement>,
}

#[derive(Clone, Debug)]
struct ApiOverload {
    params: Vec<ApiParam>,
}

#[derive(Clone, Debug)]
struct ApiFunction {
    name: String,
    description: String,
    overloads: Vec<ApiOverload>,
}

#[derive(Clone, Debug)]
struct ApiModule {
    description: String,
    functions: HashMap<String, ApiFunction>,
}

#[derive(Clone, Debug)]
pub struct CallIssue {
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct TargetProfile {
    pub board_id: String,
    pub chip_id: String,
    pub api_id: String,
    globals: HashMap<String, ApiFunction>,
    modules: HashMap<String, ApiModule>,
    chip_pins: HashMap<String, ChipPin>,
    peripherals: HashMap<String, Peripheral>,
    board_pins: Vec<BoardPin>,
}

#[derive(Clone, Debug)]
pub struct BoardChoice {
    /// File stem in `boards/`; this is the stable value stored in config.json.
    pub id: String,
    /// Human-readable name from the JSON file. UTF-8, including Chinese, is valid.
    pub name: String,
    pub chip: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SimpleChip {
    name: String,
    pins: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SimpleBoardPins {
    Direct(Vec<String>),
    Mapped(BTreeMap<String, String>),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SimpleFlash {
    name: String,
    storage: String,
    luac: bool,
    pins: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SimpleBoard {
    name: String,
    chip: String,
    pins: SimpleBoardPins,
    #[serde(default)]
    flash: Option<SimpleFlash>,
}

#[derive(Debug, Deserialize)]
struct SimpleApi {
    #[serde(default)]
    chip: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    types: Vec<SimpleType>,
    #[serde(default)]
    enums: Vec<SimpleType>,
    #[serde(default)]
    globals: Vec<SimpleFunction>,
    #[serde(default)]
    modules: Vec<SimpleModule>,
}

#[derive(Debug, Deserialize)]
struct SimpleModule {
    name: String,
    #[serde(default)]
    description: String,
    functions: Vec<SimpleFunction>,
}

#[derive(Debug, Deserialize)]
struct SimpleFunction {
    name: String,
    #[serde(default)]
    description: String,
    overloads: Vec<SimpleOverload>,
}

#[derive(Debug, Deserialize)]
struct SimpleOverload {
    params: Vec<SimpleParam>,
    #[serde(default)]
    returns: Vec<SimpleReturn>,
}

#[derive(Debug, Deserialize)]
struct SimpleParam {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    variadic: bool,
    #[serde(default)]
    resource: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct SimpleReturn {
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "type")]
    ty: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SimpleType {
    Name(String),
    Definition { id: String },
}

impl TargetProfile {
    pub fn global_completions(&self, prefix: &str) -> Vec<ProfileCompletion> {
        let prefix = prefix.to_ascii_lowercase();
        let mut out = Vec::new();
        for function in self.globals.values() {
            if function.name.to_ascii_lowercase().starts_with(&prefix) {
                out.push(ProfileCompletion {
                    label: function.name.clone(),
                    insert: format!("{}()", function.name),
                    detail: function.description.clone(),
                });
            }
        }
        for (name, module) in &self.modules {
            if name.to_ascii_lowercase().starts_with(&prefix) {
                out.push(ProfileCompletion {
                    label: format!("{name}."),
                    insert: format!("{name}."),
                    detail: module.description.clone(),
                });
            }
        }
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out
    }

    pub fn member_completions(&self, module: &str, prefix: &str) -> Vec<ProfileCompletion> {
        let Some(module) = self.modules.get(module) else {
            return Vec::new();
        };
        let prefix = prefix.to_ascii_lowercase();
        let mut out: Vec<_> = module
            .functions
            .values()
            .filter(|function| function.name.to_ascii_lowercase().starts_with(&prefix))
            .map(|function| ProfileCompletion {
                label: function.name.clone(),
                insert: format!("{}()", function.name),
                detail: function.description.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out
    }

    pub fn has_module(&self, module: &str) -> bool {
        self.modules.contains_key(module)
    }

    pub fn has_global(&self, function: &str) -> bool {
        self.globals.contains_key(function)
    }

    pub fn resource_completions(
        &self,
        module: Option<&str>,
        function: &str,
        argument_index: usize,
        prior_arguments: &[String],
    ) -> Vec<ProfileCompletion> {
        let Some(function) = self.function(module, function) else {
            return Vec::new();
        };
        let Some(overload) = function
            .overloads
            .iter()
            .find(|overload| argument_index < overload.params.len())
        else {
            return Vec::new();
        };
        let Some(resource) = overload.params[argument_index].resource.as_ref() else {
            return Vec::new();
        };
        if resource.kind != "pin" {
            return Vec::new();
        }

        let bindings = self.bindings_from_arguments(overload, prior_arguments);
        let mut candidates = Vec::new();
        for board_pin in &self.board_pins {
            for capability in self.pin_capabilities(board_pin, resource) {
                if !bindings_compatible(&bindings, &resource.bindings, |property| {
                    capability_property(capability, property)
                }) {
                    continue;
                }
                let mut detail = format!(
                    "{} · {}",
                    capability.function,
                    capability.peripheral.as_deref().unwrap_or(&capability.class)
                );
                if let Some((severity, reason)) = &board_pin.reserved {
                    detail.push_str(&format!(" · {severity}: {reason}"));
                }
                if let Some((device, _)) = &board_pin.occupied {
                    detail.push_str(&format!(" · board device: {device}"));
                }
                candidates.push((
                    board_pin.preference,
                    ProfileCompletion {
                        label: board_pin.name.clone(),
                        insert: board_pin.name.clone(),
                        detail,
                    },
                ));
                break;
            }
        }
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.label.cmp(&b.1.label)));
        candidates.into_iter().map(|(_, item)| item).collect()
    }

    pub fn validate_call(
        &self,
        module: Option<&str>,
        function_name: &str,
        arguments: &[String],
    ) -> Vec<CallIssue> {
        let Some(function) = self.function(module, function_name) else {
            return vec![CallIssue {
                code: "API001",
                message: match module {
                    Some(module) => format!("当前固件 API 不包含 {module}.{function_name}"),
                    None => format!("当前固件 API 不包含 {function_name}"),
                },
            }];
        };
        let Some(overload) = function.overloads.iter().find(|overload| {
            let required = overload
                .params
                .iter()
                .filter(|param| !param.optional && !param.variadic)
                .count();
            let accepts_extra = overload.params.last().is_some_and(|param| param.variadic);
            arguments.len() >= required && (accepts_extra || arguments.len() <= overload.params.len())
        }) else {
            return vec![CallIssue {
                code: "API002",
                message: format!("{} 的参数数量与任何重载均不匹配", function.name),
            }];
        };

        let mut issues = Vec::new();
        let mut bindings = HashMap::new();
        for (index, argument) in arguments.iter().enumerate() {
            let Some(param) = overload
                .params
                .get(index)
                .or_else(|| overload.params.last().filter(|param| param.variadic))
            else {
                break;
            };
            let Some(resource) = &param.resource else {
                if is_lua_literal(argument) && !literal_matches_type(argument, &param.ty) {
                    issues.push(CallIssue {
                        code: "API002",
                        message: format!("参数 {} 应为 {}", param.name, param.ty),
                    });
                }
                continue;
            };
            match self.resolve_argument(resource, argument, &bindings) {
                Ok(candidate) => merge_bindings(&mut bindings, &resource.bindings, |property| {
                    candidate.get(property).cloned()
                }),
                Err(issue) => issues.push(issue),
            }
        }
        issues
    }

    fn function(&self, module: Option<&str>, name: &str) -> Option<&ApiFunction> {
        match module {
            Some(module) => self.modules.get(module)?.functions.get(name),
            None => self.globals.get(name),
        }
    }

    fn pin_capabilities<'a>(
        &'a self,
        pin: &'a BoardPin,
        resource: &ResourceRequirement,
    ) -> Vec<&'a Capability> {
        if !pin.available || resource.scope != "board.exposed" {
            return Vec::new();
        }
        if let Some((severity, _)) = &pin.reserved {
            if severity == "error" && !resource.allow_reserved {
                return Vec::new();
            }
        }
        if let Some((_, releasable)) = &pin.occupied {
            if !*releasable || !resource.allow_releasable {
                return Vec::new();
            }
        }
        self.chip_pins
            .get(&pin.chip_pin)
            .into_iter()
            .flat_map(|pin| &pin.capabilities)
            .filter(|capability| {
                pin.allow_classes
                    .as_ref()
                    .is_none_or(|classes| classes.contains(&capability.class))
                    && !pin.deny_functions.contains(&capability.function)
                    && capability_matches(capability, &resource.capability)
            })
            .collect()
    }

    fn bindings_from_arguments(
        &self,
        overload: &ApiOverload,
        arguments: &[String],
    ) -> HashMap<String, String> {
        let mut bindings = HashMap::new();
        for (param, argument) in overload.params.iter().zip(arguments) {
            let Some(resource) = &param.resource else {
                continue;
            };
            if let Ok(candidate) = self.resolve_argument(resource, argument, &bindings) {
                merge_bindings(&mut bindings, &resource.bindings, |property| {
                    candidate.get(property).cloned()
                });
            }
        }
        bindings
    }

    fn resolve_argument(
        &self,
        resource: &ResourceRequirement,
        argument: &str,
        bindings: &HashMap<String, String>,
    ) -> std::result::Result<HashMap<String, String>, CallIssue> {
        if resource.kind == "peripheral" {
            if !is_lua_integer_literal(argument) && lua_string_literal(argument).is_none() {
                return Ok(HashMap::new());
            }
            let value = strip_lua_literal(argument);
            let peripheral = self.peripherals.values().find(|peripheral| {
                resource
                    .capability
                    .class
                    .as_ref()
                    .is_none_or(|class| class == &peripheral.class)
                    && (peripheral.id.eq_ignore_ascii_case(value)
                        || peripheral.instance.to_string() == value)
            });
            let Some(peripheral) = peripheral else {
                return Err(CallIssue {
                    code: "API002",
                    message: format!("外设实例 {value} 不存在于当前芯片"),
                });
            };
            let candidate = HashMap::from([
                ("id".into(), peripheral.id.clone()),
                ("instance".into(), peripheral.instance.to_string()),
                ("class".into(), peripheral.class.clone()),
                ("peripheral".into(), peripheral.id.clone()),
            ]);
            if !candidate_bindings_compatible(bindings, &resource.bindings, &candidate) {
                return Err(CallIssue {
                    code: "PIN004",
                    message: "外设实例与已选择的引脚路由不一致".into(),
                });
            }
            return Ok(candidate);
        }
        if resource.kind != "pin" {
            return Ok(HashMap::new());
        }

        // A local/parameter may evaluate to a valid pin at runtime.  Only
        // validate statically known strings; otherwise the checker would turn
        // ordinary `for _, pin in ...` code into a false PIN001 error.
        let Some(value) = lua_string_literal(argument) else {
            return Ok(HashMap::new());
        };
        let Some(board_pin) = self.board_pins.iter().find(|pin| {
            pin.name.eq_ignore_ascii_case(value)
                || pin.chip_pin.eq_ignore_ascii_case(value)
                || pin.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(value))
        }) else {
            if self.chip_pins.keys().any(|pin| pin.eq_ignore_ascii_case(value)) {
                return Err(CallIssue {
                    code: "PIN002",
                    message: format!("引脚 {value} 未由当前开发板引出"),
                });
            }
            return Err(CallIssue {
                code: "PIN001",
                message: format!("当前芯片不存在引脚 {value}"),
            });
        };
        if !board_pin.available {
            return Err(CallIssue {
                code: "PIN002",
                message: format!("引脚 {} 在当前开发板不可用", board_pin.name),
            });
        }
        if let Some((severity, reason)) = &board_pin.reserved {
            if severity == "error" && !resource.allow_reserved {
                return Err(CallIssue {
                    code: "BOARD001",
                    message: format!("引脚 {} 被保留: {reason}", board_pin.name),
                });
            }
        }
        if let Some((device, releasable)) = &board_pin.occupied {
            if !*releasable || !resource.allow_releasable {
                return Err(CallIssue {
                    code: "PIN005",
                    message: format!("引脚 {} 已被板载设备 {device} 占用", board_pin.name),
                });
            }
        }
        for capability in self.pin_capabilities(board_pin, resource) {
            let candidate = capability_map(capability, board_pin);
            if candidate_bindings_compatible(bindings, &resource.bindings, &candidate) {
                return Ok(candidate);
            }
        }
        let has_required_capability = self
            .pin_capabilities(board_pin, resource)
            .into_iter()
            .next()
            .is_some();
        Err(CallIssue {
            code: if has_required_capability { "PIN004" } else { "PIN003" },
            message: if has_required_capability {
                format!("引脚 {} 与同一调用中的外设或 route 不一致", board_pin.name)
            } else {
                format!("引脚 {} 不支持参数要求的功能", board_pin.name)
            },
        })
    }
}

/// Locate portable data beside the executable. During development, fall back to
/// the crate root so `cargo test` and `cargo run` use the same files.
pub fn data_root() -> Result<PathBuf> {
    let exe_root = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let manifest_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for root in exe_root.into_iter().chain([manifest_root]) {
        if root.join("chips").is_dir()
            && root.join("boards").is_dir()
            && root.join("apis").is_dir()
        {
            return Ok(root);
        }
    }
    bail!("找不到 IDE 数据目录；chips、boards、apis 必须与 IDE 放在同一目录")
}

pub fn discover_boards() -> Result<Vec<BoardChoice>> {
    let root = data_root()?;
    let board_dir = root.join("boards");
    let mut choices = Vec::new();
    for entry in fs::read_dir(&board_dir)
        .with_context(|| format!("读取开发板目录 {}", board_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let board: SimpleBoard = read_simple(&path, "开发板")?;
        validate_identifier(&board.chip, "开发板 chip")?;
        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow!("开发板文件名必须为 UTF-8: {}", path.display()))?
            .to_string();
        choices.push(BoardChoice {
            id,
            name: board.name,
            chip: board.chip,
        });
    }
    choices.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    if choices.is_empty() {
        bail!("boards 目录中没有开发板 JSON")
    }
    Ok(choices)
}

/// Load the selected Board, its referenced Chip, and that chip's newest API.
/// All three files are validated before a profile is returned, so completion
/// and diagnostics can never observe a partially switched target.
pub fn load_board(board_id: &str) -> Result<Arc<TargetProfile>> {
    validate_identifier(board_id, "开发板文件名")?;
    let root = data_root()?;
    let board_path = root.join("boards").join(format!("{board_id}.json"));
    let board: SimpleBoard = read_simple(&board_path, "开发板")?;
    validate_identifier(&board.chip, "开发板 chip")?;
    let chip_path = root.join("chips").join(format!("{}.json", board.chip));
    let api_path = root.join("apis").join(format!("{}_lua.json", board.chip));
    let chip: SimpleChip = read_simple(&chip_path, "芯片")?;
    let api: SimpleApi = read_simple(&api_path, "API")?;
    build_simple_profile(board_id, chip, board, api).map(Arc::new)
}

fn read_simple<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> Result<T> {
    let text = fs::read_to_string(path).with_context(|| format!("读取{label}文件 {}", path.display()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    serde_json::from_str(text).with_context(|| format!("校验{label}文件 {}", path.display()))
}

fn validate_identifier(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        bail!("{label} 只能包含字母、数字、下划线和连字符: {value}")
    }
    Ok(())
}

fn build_simple_profile(
    _board_id: &str,
    chip: SimpleChip,
    board: SimpleBoard,
    api: SimpleApi,
) -> Result<TargetProfile> {
    if chip.name != board.chip {
        bail!("三文件不一致：开发板引用 {}，芯片文件声明 {}", board.chip, chip.name);
    }
    let api_chip = if api.chip.is_empty() {
        api.id.split('.').next().unwrap_or_default().to_string()
    } else {
        api.chip.clone()
    };
    if api_chip != chip.name {
        bail!("三文件不一致：芯片为 {}，API 声明 {}", chip.name, api.chip);
    }
    validate_identifier(&chip.name, "芯片 name")?;
    if chip.pins.is_empty() {
        bail!("芯片文件 pins 不能为空");
    }

    let mut chip_pins = HashMap::new();
    let mut peripherals: HashMap<String, Peripheral> = HashMap::new();
    for (pin_name, functions) in chip.pins {
        if functions.is_empty() {
            bail!("芯片引脚 {pin_name} 的功能复用列表不能为空");
        }
        if chip_pins.contains_key(&pin_name) {
            bail!("芯片文件存在重复引脚 {pin_name}");
        }
        let mut seen = HashSet::new();
        let mut capabilities = Vec::new();
        for function in functions {
            if function.trim().is_empty() || !seen.insert(function.clone()) {
                bail!("芯片引脚 {pin_name} 包含空值或重复功能 {function}");
            }
            for capability in infer_capabilities(&function) {
                if let Some(peripheral_id) = capability.peripheral.as_ref() {
                    let instance = trailing_number(peripheral_id).unwrap_or(0) as i64;
                    let peripheral = peripherals.entry(peripheral_id.clone()).or_insert_with(|| {
                        Peripheral {
                            id: peripheral_id.clone(),
                            class: capability.class.clone(),
                            instance,
                            signals: HashSet::new(),
                        }
                    });
                    if let Some(signal) = capability.signal.as_ref() {
                        peripheral.signals.insert(signal.clone());
                    }
                }
                capabilities.push(capability);
            }
        }
        chip_pins.insert(pin_name, ChipPin { capabilities });
    }

    let flash_pins: HashSet<String> = board
        .flash
        .as_ref()
        .map(|flash| flash.pins.values().cloned().collect())
        .unwrap_or_default();
    if let Some(flash) = board.flash.as_ref() {
        if flash.name.trim().is_empty() || flash.storage != "external" || !flash.luac {
            bail!("开发板 flash 必须声明非空 name、storage=external 和 luac=true");
        }
        for pin in &flash_pins {
            if !chip_pins.contains_key(pin) {
                bail!("开发板 flash 引用了芯片中不存在的引脚 {pin}");
            }
        }
    }

    let pin_pairs: Vec<(String, String)> = match board.pins {
        SimpleBoardPins::Direct(pins) => pins.into_iter().map(|pin| (pin.clone(), pin)).collect(),
        SimpleBoardPins::Mapped(pins) => pins.into_iter().collect(),
    };
    if pin_pairs.is_empty() {
        bail!("开发板 pins 不能为空");
    }
    let mut board_names = HashSet::new();
    let mut board_pins = Vec::new();
    for (name, chip_pin) in pin_pairs {
        if !board_names.insert(name.clone()) {
            bail!("开发板存在重复引出脚名称 {name}");
        }
        if !chip_pins.contains_key(&chip_pin) {
            bail!("开发板引出脚 {name} 引用了芯片中不存在的引脚 {chip_pin}");
        }
        board_pins.push(BoardPin {
            id: name.clone(),
            name,
            aliases: Vec::new(),
            chip_pin: chip_pin.clone(),
            available: true,
            allow_classes: None,
            deny_functions: HashSet::new(),
            preference: 0,
            reserved: None,
            occupied: flash_pins
                .contains(&chip_pin)
                .then(|| ("external-flash".into(), false)),
        });
    }

    let declared_types: HashSet<String> = api
        .types
        .iter()
        .chain(&api.enums)
        .map(|item| match item {
            SimpleType::Name(id) | SimpleType::Definition { id } => id.clone(),
        })
        .collect();
    let mut globals = HashMap::new();
    for function in api.globals {
        let function = convert_simple_function(function, &declared_types)?;
        if globals.insert(function.name.clone(), function).is_some() {
            bail!("API 存在重复全局函数");
        }
    }
    let mut modules = HashMap::new();
    for module in api.modules {
        validate_identifier(&module.name, "API 模块名")?;
        let mut functions = HashMap::new();
        for function in module.functions {
            let function = convert_simple_function(function, &declared_types)?;
            if functions.insert(function.name.clone(), function).is_some() {
                bail!("API 模块 {} 存在重复函数", module.name);
            }
        }
        if functions.is_empty() {
            bail!("API 模块 {} 不能为空", module.name);
        }
        let description = if module.description.is_empty() {
            "firmware module".into()
        } else {
            module.description
        };
        if modules
            .insert(module.name.clone(), ApiModule { description, functions })
            .is_some()
        {
            bail!("API 存在重复模块 {}", module.name);
        }
    }
    validate_api_resources(&globals, &modules, &peripherals, &chip_pins, &board_pins)?;

    Ok(TargetProfile {
        board_id: board.name,
        chip_id: chip.name.clone(),
        api_id: format!("{}_lua", chip.name),
        globals,
        modules,
        chip_pins,
        peripherals,
        board_pins,
    })
}

fn convert_simple_function(
    function: SimpleFunction,
    declared_types: &HashSet<String>,
) -> Result<ApiFunction> {
    validate_identifier(&function.name, "API 函数名")?;
    if function.overloads.is_empty() {
        bail!("API 函数 {} 没有 overloads", function.name);
    }
    let mut overloads = Vec::new();
    for overload in function.overloads {
        let mut names = HashSet::new();
        let mut params = Vec::new();
        for param in overload.params {
            validate_identifier(&param.name, "API 参数名")?;
            if !names.insert(param.name.clone()) {
                bail!("API 函数 {} 存在重复参数 {}", function.name, param.name);
            }
            validate_simple_type(&param.ty, declared_types, &function.name)?;
            params.push(ApiParam {
                name: param.name,
                ty: param.ty,
                optional: param.optional,
                variadic: param.variadic,
                resource: param.resource.as_ref().map(parse_resource).transpose()?,
            });
        }
        for returned in overload.returns {
            validate_simple_type(&returned.ty, declared_types, &function.name)?;
            let _ = returned.name;
        }
        overloads.push(ApiOverload { params });
    }
    Ok(ApiFunction {
        name: function.name,
        description: if function.description.is_empty() {
            "firmware API".into()
        } else {
            function.description
        },
        overloads,
    })
}

fn validate_simple_type(ty: &str, declared: &HashSet<String>, function: &str) -> Result<()> {
    const BUILTIN: [&str; 10] = [
        "nil", "boolean", "integer", "number", "string", "table", "function", "userdata",
        "thread", "any",
    ];
    if !BUILTIN.contains(&ty) && !declared.contains(ty) {
        bail!("API 函数 {function} 引用了未知类型 {ty}");
    }
    Ok(())
}

fn infer_capabilities(function: &str) -> Vec<Capability> {
    let upper = function.to_ascii_uppercase();
    let (class, peripheral, signal) = if upper.starts_with("GPIO") {
        ("gpio", None, None)
    } else if let Some(rest) = upper.strip_prefix('A') {
        let mut parts = rest.split('_');
        match (parts.next(), parts.next()) {
            (Some(adc), Some(channel)) if adc.chars().all(|ch| ch.is_ascii_digit()) => (
                "adc",
                Some(format!("ADC{adc}")),
                Some(format!("CH{channel}")),
            ),
            _ => ("analog", None, None),
        }
    } else {
        let prefix = upper.split('_').next().unwrap_or(&upper);
        let signal = upper.split_once('_').map(|(_, value)| {
            if value == "SCLK" { "SCK".to_string() } else { value.to_string() }
        });
        if prefix.starts_with("I2C") {
            ("i2c", Some(prefix.to_string()), signal)
        } else if prefix.starts_with("UART") {
            ("uart", Some(prefix.to_string()), signal)
        } else if prefix.starts_with("SPI") {
            ("spi", Some(prefix.to_string()), signal)
        } else if let Some(instance) = prefix.strip_prefix("CANFD") {
            let signal = signal.map(|value| value.strip_prefix("CAN").unwrap_or(&value).to_string());
            ("can", Some(format!("CAN{instance}")), signal)
        } else if prefix.starts_with("TIMA") || prefix.starts_with("TIMG") {
            ("timer", Some(prefix.to_string()), signal)
        } else if prefix.starts_with("COMP") {
            ("comp", Some(prefix.to_string()), signal)
        } else if prefix.starts_with("DAC") {
            ("dac", Some("DAC0".into()), signal)
        } else {
            ("system", None, signal)
        }
    };
    let route = peripheral.as_ref().map(|value| value.to_ascii_lowercase());
    let base = Capability {
        function: function.to_string(),
        class: class.into(),
        peripheral: peripheral.clone(),
        signal: signal.clone(),
        route: route.clone(),
    };
    if class == "timer" {
        vec![
            base,
            Capability {
                function: function.to_string(),
                class: "pwm".into(),
                peripheral,
                signal,
                route,
            },
        ]
    } else {
        vec![base]
    }
}

fn trailing_number(value: &str) -> Option<u32> {
    let digits: String = value
        .chars()
        .rev()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

#[allow(dead_code)]
pub fn load_for_project(project_dir: &Path, target: &ProjectTarget) -> Result<Arc<TargetProfile>> {
    let roots = registry_roots(project_dir);
    let board_path = find_registry_file(
        &roots,
        "board",
        &target.board,
        &format!("={}", target.board_version),
        ".board.json",
    )?;
    let api_path = find_registry_file(
        &roots,
        "api",
        &target.api,
        &format!("={}", target.api_version),
        ".api.json",
    )?;
    let board = read_and_validate(&board_path, "Board", BOARD_SCHEMA)?;
    require_production_quality(&board, "Board")?;
    let chip_ref = object(&board, "chip_ref")?;
    let chip_id = string(chip_ref, "id")?;
    let chip_range = string(chip_ref, "version")?;
    let chip_path = find_registry_file(&roots, "chip", chip_id, chip_range, ".chip.json")?;
    let chip = read_and_validate(&chip_path, "Chip", CHIP_SCHEMA)?;
    let api = read_and_validate(&api_path, "API", API_SCHEMA)?;
    require_production_quality(&chip, "Chip")?;
    require_production_quality(&api, "API")?;
    build_profile(&chip, &board, &api).map(Arc::new)
}

pub fn load_profile_files(chip: &Path, board: &Path, api: &Path) -> Result<TargetProfile> {
    let chip = read_and_validate(chip, "Chip", CHIP_SCHEMA)?;
    let board = read_and_validate(board, "Board", BOARD_SCHEMA)?;
    let api = read_and_validate(api, "API", API_SCHEMA)?;
    build_profile(&chip, &board, &api)
}

fn read_and_validate(path: &Path, label: &str, schema_text: &str) -> Result<Value> {
    let text = fs::read_to_string(path).with_context(|| format!("读取 {label} {}", path.display()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let value: Value = serde_json::from_str(text)
        .with_context(|| format!("解析 {label} {}", path.display()))?;
    validate_value(value, label, &path.display().to_string(), schema_text)
}

fn require_production_quality(value: &Value, label: &str) -> Result<()> {
    let quality = object(value, "quality")?;
    let status = string(quality, "status")?;
    let coverage = string(quality, "coverage")?;
    if !matches!(status, "reviewed" | "verified") || coverage != "complete" {
        bail!(
            "META001: {label} production metadata requires reviewed/verified status and complete coverage"
        );
    }
    Ok(())
}

pub fn validate_api_bytes(bytes: &[u8], source: &str) -> Result<Value> {
    let text = std::str::from_utf8(bytes).with_context(|| format!("API {source} 不是 UTF-8"))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let value: Value = serde_json::from_str(text).with_context(|| format!("解析 API {source}"))?;
    let value = validate_value(value, "API", source, API_SCHEMA)?;
    validate_api_type_references(&value)?;
    Ok(value)
}

fn validate_value(value: Value, label: &str, source: &str, schema_text: &str) -> Result<Value> {
    let schema: Value = serde_json::from_str(schema_text).context("内置元数据 Schema 无效")?;
    let validator = jsonschema::validator_for(&schema).context("编译内置元数据 Schema")?;
    let errors: Vec<String> = validator
        .iter_errors(&value)
        .take(8)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    if !errors.is_empty() {
        bail!("{label} Schema 校验失败 {source}: {}", errors.join("; "));
    }
    Ok(value)
}

fn build_profile(chip: &Value, board: &Value, api: &Value) -> Result<TargetProfile> {
    let chip_id = string(chip, "id")?.to_string();
    let chip_version = Version::parse(string(chip, "version")?).context("Chip version 不是 SemVer")?;
    let board_id = string(board, "id")?.to_string();
    let api_id = string(api, "id")?.to_string();

    let chip_ref = object(board, "chip_ref")?;
    if string(chip_ref, "id")? != chip_id {
        bail!("META001: Board 引用的 Chip ID 与已加载文件不一致");
    }
    if !version_matches(string(chip_ref, "version")?, &chip_version)? {
        bail!("META001: Chip {} 不满足 Board 版本约束", chip_version);
    }
    let compatible_chips = array(object(api, "compatibility")?, "chip_ids")?;
    if !compatible_chips.iter().any(|id| id.as_str() == Some(&chip_id)) {
        bail!("META001: API compatibility 未包含 Chip {chip_id}");
    }
    if let Some(board_ids) = object(api, "compatibility")?.get("board_ids") {
        let board_ids = board_ids.as_array().context("board_ids 必须为数组")?;
        if !board_ids.iter().any(|id| id.as_str() == Some(&board_id)) {
            bail!("META001: API compatibility 未包含 Board {board_id}");
        }
    }
    if let Some(api_ids) = board.get("compatibility").and_then(|v| v.get("api_ids")) {
        if !api_ids
            .as_array()
            .context("Board compatibility.api_ids 必须为数组")?
            .iter()
            .any(|id| id.as_str() == Some(&api_id))
        {
            bail!("META001: Board compatibility 未包含 API {api_id}");
        }
    }

    let peripherals = parse_peripherals(chip)?;
    let chip_pins = parse_chip_pins(chip, &peripherals)?;
    let board_pins = parse_board_pins(board, &chip_pins)?;
    validate_board_resources(board, &peripherals, &chip_pins, &board_pins)?;
    validate_api_type_references(api)?;
    let (globals, modules) = parse_api(api)?;
    validate_api_resources(&globals, &modules, &peripherals, &chip_pins, &board_pins)?;

    Ok(TargetProfile {
        board_id,
        chip_id,
        api_id,
        globals,
        modules,
        chip_pins,
        peripherals,
        board_pins,
    })
}

fn parse_peripherals(chip: &Value) -> Result<HashMap<String, Peripheral>> {
    let mut out = HashMap::new();
    for value in array(chip, "peripherals")? {
        let id = string(value, "id")?.to_string();
        if out.contains_key(&id) {
            bail!("META001: 重复 Peripheral ID {id}");
        }
        out.insert(
            id.clone(),
            Peripheral {
                id,
                class: string(value, "class")?.to_string(),
                instance: integer(value, "instance")?,
                signals: array(value, "signals")?
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
            },
        );
    }
    Ok(out)
}

fn parse_chip_pins(
    chip: &Value,
    peripherals: &HashMap<String, Peripheral>,
) -> Result<HashMap<String, ChipPin>> {
    let mut out = HashMap::new();
    for value in array(chip, "pins")? {
        let id = string(value, "id")?.to_string();
        if out.contains_key(&id) {
            bail!("META001: 重复 Chip Pin ID {id}");
        }
        let mut capabilities = Vec::new();
        let mut capability_ids = HashSet::new();
        for capability in array(value, "capabilities")? {
            let capability_id = string(capability, "id")?;
            if !capability_ids.insert(capability_id.to_string()) {
                bail!("META001: {id} 存在重复 capability ID {capability_id}");
            }
            let peripheral = optional_string(capability, "peripheral");
            let signal = optional_string(capability, "signal");
            if let Some(peripheral_id) = peripheral.as_deref() {
                let Some(spec) = peripherals.get(peripheral_id) else {
                    bail!("META001: {id} capability 引用了不存在的外设 {peripheral_id}");
                };
                let Some(signal) = signal.as_deref() else {
                    bail!("META001: {id} capability 引用外设时必须包含 signal");
                };
                if !spec.signals.contains(signal) {
                    bail!("META001: {id} 的信号 {signal} 不属于外设 {peripheral_id}");
                }
            }
            capabilities.push(Capability {
                function: string(capability, "function")?.to_string(),
                class: string(capability, "class")?.to_string(),
                peripheral,
                signal,
                route: optional_string(capability, "route"),
            });
        }
        out.insert(id, ChipPin { capabilities });
    }
    Ok(out)
}

fn parse_board_pins(board: &Value, chip_pins: &HashMap<String, ChipPin>) -> Result<Vec<BoardPin>> {
    let mut out = Vec::new();
    let mut ids = HashSet::new();
    for value in array(board, "pins")? {
        let id = string(value, "id")?.to_string();
        if !ids.insert(id.clone()) {
            bail!("META001: 重复 Board Pin ID {id}");
        }
        let chip_pin = string(value, "chip_pin")?.to_string();
        if !chip_pins.contains_key(&chip_pin) {
            bail!("META001: Board pin {id} 引用了不存在的 Chip pin {chip_pin}");
        }
        let allow_classes = value.get("allow_classes").map(|classes| {
            classes
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        });
        out.push(BoardPin {
            id,
            name: string(value, "name")?.to_string(),
            aliases: optional_string_array(value, "aliases"),
            chip_pin,
            available: value.get("available").and_then(Value::as_bool).unwrap_or(false),
            allow_classes,
            deny_functions: optional_string_array(value, "deny_functions")
                .into_iter()
                .collect(),
            preference: value.get("preference").and_then(Value::as_i64).unwrap_or(0),
            reserved: None,
            occupied: None,
        });
    }
    for reserved in optional_array(board, "reserved_pins") {
        let chip_pin = string(reserved, "chip_pin")?;
        let reason = string(reserved, "reason")?.to_string();
        let severity = string(reserved, "severity")?.to_string();
        for pin in out.iter_mut().filter(|pin| pin.chip_pin == chip_pin) {
            pin.reserved = Some((severity.clone(), reason.clone()));
        }
    }
    for device in optional_array(board, "onboard_devices") {
        let device_id = string(device, "id")?.to_string();
        for connection in optional_array(device, "connections") {
            if !connection
                .get("exclusive")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            let chip_pin = string(connection, "chip_pin")?;
            let releasable = connection
                .get("releasable")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            for pin in out.iter_mut().filter(|pin| pin.chip_pin == chip_pin) {
                pin.occupied = Some((device_id.clone(), releasable));
            }
        }
    }
    Ok(out)
}

fn validate_board_resources(
    board: &Value,
    peripherals: &HashMap<String, Peripheral>,
    chip_pins: &HashMap<String, ChipPin>,
    board_pins: &[BoardPin],
) -> Result<()> {
    let board_ids: HashSet<_> = board_pins.iter().map(|pin| pin.id.as_str()).collect();
    for device in optional_array(board, "onboard_devices") {
        for connection in optional_array(device, "connections") {
            let chip_pin = string(connection, "chip_pin")?;
            if !chip_pins.contains_key(chip_pin) {
                bail!("META001: 板载设备连接引用不存在的 Chip pin {chip_pin}");
            }
            if let Some(function) = connection.get("function").and_then(Value::as_str) {
                if !chip_pins[chip_pin]
                    .capabilities
                    .iter()
                    .any(|capability| capability.function == function)
                {
                    bail!("META001: 板载连接 {chip_pin} 不支持 function {function}");
                }
            }
            if let Some(board_pin) = connection.get("board_pin").and_then(Value::as_str) {
                if !board_ids.contains(board_pin) {
                    bail!("META001: 板载设备连接引用不存在的 Board pin {board_pin}");
                }
            }
        }
    }
    for reserved in optional_array(board, "reserved_pins") {
        let chip_pin = string(reserved, "chip_pin")?;
        if !chip_pins.contains_key(chip_pin) {
            bail!("META001: reserved_pins 引用不存在的 Chip pin {chip_pin}");
        }
    }
    let device_ids = unique_ids(board, "onboard_devices")?;
    let memory_ids = unique_ids(board, "memory_devices")?;
    let _artifact_ids = unique_ids(board, "artifact_targets")?;
    for memory in optional_array(board, "memory_devices") {
        if let Some(device) = memory
            .get("interface")
            .and_then(|value| value.get("device"))
            .and_then(Value::as_str)
        {
            if !device_ids.contains(device) {
                bail!("META001: memory interface 引用不存在的板载设备 {device}");
            }
        }
        if let Some(peripheral) = memory
            .get("interface")
            .and_then(|value| value.get("peripheral"))
            .and_then(Value::as_str)
        {
            if !peripherals.contains_key(peripheral) {
                bail!("META001: memory interface 引用不存在的芯片外设 {peripheral}");
            }
        }
    }
    let mut has_lua_bytecode = false;
    for target in optional_array(board, "artifact_targets") {
        let storage = string(target, "storage")?;
        if !memory_ids.contains(storage) {
            bail!("META001: artifact target 引用不存在的存储 {storage}");
        }
        let writable = optional_array(board, "memory_devices")
            .find(|memory| memory.get("id").and_then(Value::as_str) == Some(storage))
            .and_then(|memory| memory.get("writable"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !writable {
            bail!("STORE001: artifact target 指向只读存储 {storage}");
        }
        has_lua_bytecode |= string(target, "kind")? == "lua_bytecode";
    }
    if !has_lua_bytecode {
        bail!("STORE001: Board 未定义 lua_bytecode artifact target");
    }
    Ok(())
}

fn validate_api_type_references(api: &Value) -> Result<()> {
    let mut types: HashSet<String> = [
        "nil", "boolean", "integer", "number", "string", "table", "function", "userdata",
        "thread", "any",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    for collection in ["types", "enums"] {
        for definition in array(api, collection)? {
            let id = string(definition, "id")?.to_string();
            if !types.insert(id.clone()) {
                bail!("META001: API 存在重复类型 ID {id}");
            }
        }
    }
    for function in array(api, "globals")?.iter().chain(
        array(api, "modules")?
            .iter()
            .flat_map(|module| module.get("functions").and_then(Value::as_array).into_iter().flatten()),
    ) {
        for overload in array(function, "overloads")? {
            for param in array(overload, "params")? {
                let ty = string(param, "type")?;
                if !types.contains(ty) {
                    bail!("META001: API 参数引用未知类型 {ty}");
                }
            }
            for returned in array(overload, "returns")? {
                let ty = string(returned, "type")?;
                if !types.contains(ty) {
                    bail!("META001: API 返回值引用未知类型 {ty}");
                }
            }
        }
    }
    Ok(())
}

fn parse_api(api: &Value) -> Result<(HashMap<String, ApiFunction>, HashMap<String, ApiModule>)> {
    let mut globals = HashMap::new();
    for function in array(api, "globals")? {
        let function = parse_function(function)?;
        if globals.insert(function.name.clone(), function).is_some() {
            bail!("META001: API 存在重复全局函数");
        }
    }
    let mut modules = HashMap::new();
    for module in array(api, "modules")? {
        let name = string(module, "name")?.to_string();
        let mut functions = HashMap::new();
        for function in array(module, "functions")? {
            let function = parse_function(function)?;
            if functions.insert(function.name.clone(), function).is_some() {
                bail!("META001: 模块 {name} 存在重复函数");
            }
        }
        if modules
            .insert(
                name,
                ApiModule {
                    description: optional_string(module, "description").unwrap_or_else(|| "module".into()),
                    functions,
                },
            )
            .is_some()
        {
            bail!("META001: API 存在重复模块");
        }
    }
    Ok((globals, modules))
}

fn parse_function(value: &Value) -> Result<ApiFunction> {
    let name = string(value, "name")?.to_string();
    let mut overload_ids = HashSet::new();
    let mut overloads = Vec::new();
    for overload in array(value, "overloads")? {
        let overload_id = string(overload, "id")?;
        if !overload_ids.insert(overload_id.to_string()) {
            bail!("META001: 函数 {name} 存在重复 overload ID {overload_id}");
        }
        let mut param_names = HashSet::new();
        let mut params = Vec::new();
        for param in array(overload, "params")? {
            let param_name = string(param, "name")?.to_string();
            if !param_names.insert(param_name.clone()) {
                bail!("META001: 函数 {name} 存在重复参数 {param_name}");
            }
            params.push(ApiParam {
                name: param_name,
                ty: string(param, "type")?.to_string(),
                optional: param.get("optional").and_then(Value::as_bool).unwrap_or(false),
                variadic: param.get("variadic").and_then(Value::as_bool).unwrap_or(false),
                resource: param.get("resource").map(parse_resource).transpose()?,
            });
        }
        overloads.push(ApiOverload { params });
    }
    Ok(ApiFunction {
        name,
        description: optional_string(value, "description").unwrap_or_else(|| "firmware API".into()),
        overloads,
    })
}

fn parse_resource(value: &Value) -> Result<ResourceRequirement> {
    let capability = value.get("capability").cloned().unwrap_or(Value::Object(Default::default()));
    let bindings = value
        .get("bindings")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|map| map.iter())
        .filter_map(|(property, slot)| Some((property.clone(), slot.as_str()?.to_string())))
        .collect();
    Ok(ResourceRequirement {
        kind: string(value, "kind")?.to_string(),
        scope: string(value, "scope")?.to_string(),
        capability: CapabilityRequirement {
            class: optional_string(&capability, "class"),
            function: optional_string(&capability, "function"),
            peripheral: optional_string(&capability, "peripheral"),
            signal: optional_string(&capability, "signal"),
            route: optional_string(&capability, "route"),
        },
        bindings,
        allow_reserved: value.get("allow_reserved").and_then(Value::as_bool).unwrap_or(false),
        allow_releasable: value
            .get("allow_releasable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn validate_api_resources(
    globals: &HashMap<String, ApiFunction>,
    modules: &HashMap<String, ApiModule>,
    peripherals: &HashMap<String, Peripheral>,
    chip_pins: &HashMap<String, ChipPin>,
    board_pins: &[BoardPin],
) -> Result<()> {
    for function in globals
        .values()
        .chain(modules.values().flat_map(|module| module.functions.values()))
    {
        for overload in &function.overloads {
            for param in &overload.params {
                let Some(resource) = &param.resource else {
                    continue;
                };
                if resource.kind == "peripheral"
                    && !peripherals.values().any(|peripheral| {
                        resource
                            .capability
                            .class
                            .as_ref()
                            .is_none_or(|class| class == &peripheral.class)
                    })
                {
                    bail!("META001: API {}.{} 没有兼容的芯片外设", function.name, param.name);
                }
                if resource.kind == "pin" {
                    let chip_match = chip_pins.values().any(|pin| {
                        pin.capabilities
                            .iter()
                            .any(|capability| capability_matches(capability, &resource.capability))
                    });
                    if !chip_match {
                        bail!("META001: API {}.{} 的引脚能力无法由 Chip 提供", function.name, param.name);
                    }
                    if resource.scope == "board.exposed" {
                        let board_match = board_pins.iter().any(|pin| {
                            pin.available
                                && chip_pins.get(&pin.chip_pin).is_some_and(|chip_pin| {
                                    chip_pin.capabilities.iter().any(|capability| {
                                        capability_matches(capability, &resource.capability)
                                    })
                                })
                        });
                        if !board_match {
                            bail!("META001: API {}.{} 在 Board 上没有可用引出脚", function.name, param.name);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn capability_matches(capability: &Capability, requirement: &CapabilityRequirement) -> bool {
    requirement.class.as_ref().is_none_or(|v| v == &capability.class)
        && requirement
            .function
            .as_ref()
            .is_none_or(|v| v == &capability.function)
        && requirement
            .peripheral
            .as_ref()
            .is_none_or(|v| capability.peripheral.as_ref() == Some(v))
        && requirement
            .signal
            .as_ref()
            .is_none_or(|v| capability.signal.as_ref() == Some(v))
        && requirement
            .route
            .as_ref()
            .is_none_or(|v| capability.route.as_ref() == Some(v))
}

fn capability_property(capability: &Capability, property: &str) -> Option<String> {
    match property {
        "class" => Some(capability.class.clone()),
        "function" => Some(capability.function.clone()),
        "peripheral" => capability.peripheral.clone(),
        "signal" => capability.signal.clone(),
        "route" => capability.route.clone(),
        _ => None,
    }
}

fn capability_map(capability: &Capability, pin: &BoardPin) -> HashMap<String, String> {
    let mut out = HashMap::from([
        ("id".into(), pin.chip_pin.clone()),
        ("class".into(), capability.class.clone()),
        ("function".into(), capability.function.clone()),
    ]);
    for (key, value) in [
        ("peripheral", capability.peripheral.as_ref()),
        ("signal", capability.signal.as_ref()),
        ("route", capability.route.as_ref()),
    ] {
        if let Some(value) = value {
            out.insert(key.into(), value.clone());
        }
    }
    out
}

fn bindings_compatible(
    existing: &HashMap<String, String>,
    bindings: &HashMap<String, String>,
    mut value: impl FnMut(&str) -> Option<String>,
) -> bool {
    bindings.iter().all(|(property, slot)| {
        existing
            .get(slot)
            .is_none_or(|bound| value(property).as_ref() == Some(bound))
    })
}

fn candidate_bindings_compatible(
    existing: &HashMap<String, String>,
    bindings: &HashMap<String, String>,
    candidate: &HashMap<String, String>,
) -> bool {
    bindings_compatible(existing, bindings, |property| candidate.get(property).cloned())
}

fn merge_bindings(
    output: &mut HashMap<String, String>,
    bindings: &HashMap<String, String>,
    mut value: impl FnMut(&str) -> Option<String>,
) {
    for (property, slot) in bindings {
        if let Some(value) = value(property) {
            output.entry(slot.clone()).or_insert(value);
        }
    }
}

fn literal_matches_type(argument: &str, ty: &str) -> bool {
    let value = argument.trim();
    match ty {
        "integer" | "frequency_hz" => is_lua_integer_literal(value),
        "number" => value.parse::<f64>().is_ok() || is_lua_integer_literal(value),
        "string" | "pin" => {
            (value.starts_with('\'') && value.ends_with('\''))
                || (value.starts_with('"') && value.ends_with('"'))
        }
        "boolean" => matches!(value, "true" | "false"),
        _ => true,
    }
}

fn is_lua_integer_literal(value: &str) -> bool {
    let unsigned = value
        .strip_prefix('-')
        .or_else(|| value.strip_prefix('+'))
        .unwrap_or(value);
    if let Some(hex) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        !hex.is_empty() && i64::from_str_radix(hex, 16).is_ok()
    } else {
        value.parse::<i64>().is_ok()
    }
}

fn is_lua_literal(value: &str) -> bool {
    let value = value.trim();
    lua_string_literal(value).is_some()
        || is_lua_integer_literal(value)
        || value.parse::<f64>().is_ok()
        || matches!(value, "true" | "false" | "nil")
}

fn strip_lua_literal(value: &str) -> &str {
    let value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('\'') && value.ends_with('\''))
            || (value.starts_with('"') && value.ends_with('"')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn lua_string_literal(value: &str) -> Option<&str> {
    let value = value.trim();
    (value.len() >= 2
        && ((value.starts_with('\'') && value.ends_with('\''))
            || (value.starts_with('"') && value.ends_with('"'))))
        .then(|| &value[1..value.len() - 1])
}

fn registry_roots(project_dir: &Path) -> Vec<PathBuf> {
    let mut roots = vec![
        project_dir.join("metadata"),
        project_dir.join("firmware/release"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join("metadata"));
            roots.push(dir.join("firmware/release"));
        }
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    roots.push(manifest_dir.join("metadata"));
    if let Some(parent) = manifest_dir.parent() {
        roots.push(parent.join("mspm0_lua/release"));
    }
    let mut unique = Vec::new();
    for root in roots {
        if !unique.contains(&root) {
            unique.push(root);
        }
    }
    unique
}

fn find_registry_file(
    roots: &[PathBuf],
    kind: &str,
    id: &str,
    version: &str,
    suffix: &str,
) -> Result<PathBuf> {
    let req = parse_version_req(version)?;
    let mut matches = Vec::new();
    for (root_priority, root) in roots.iter().filter(|root| root.is_dir()).enumerate() {
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            if depth > 5 {
                continue;
            }
            for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push((path, depth + 1));
                    continue;
                }
                if !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(suffix))
                {
                    continue;
                }
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(value) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else {
                    continue;
                };
                if value.get("kind").and_then(Value::as_str) != Some(kind)
                    || value.get("id").and_then(Value::as_str) != Some(id)
                {
                    continue;
                }
                let Some(found_version) = value.get("version").and_then(Value::as_str) else {
                    continue;
                };
                let Ok(found_version) = Version::parse(found_version) else {
                    continue;
                };
                if req.matches(&found_version) {
                    matches.push((root_priority, found_version, path));
                }
            }
        }
    }
    matches.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));
    let Some((best_root, best_version, best_path)) = matches.first() else {
        bail!(
            "META001: 在 metadata registry 中找不到 {kind} {id} ({version})；搜索路径: {}",
            roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
        );
    };
    if matches
        .iter()
        .skip(1)
        .any(|(candidate_root, candidate_version, _)| {
            candidate_root == best_root && candidate_version == best_version
        })
    {
        bail!("META001: {kind} {id} {best_version} 存在重复注册");
    }
    Ok(best_path.clone())
}

fn version_matches(requirement: &str, version: &Version) -> Result<bool> {
    Ok(parse_version_req(requirement)?.matches(version))
}

fn parse_version_req(requirement: &str) -> Result<VersionReq> {
    let normalized = if requirement.contains(' ') && !requirement.contains(',') {
        requirement.split_whitespace().collect::<Vec<_>>().join(", ")
    } else {
        requirement.to_string()
    };
    VersionReq::parse(&normalized).with_context(|| format!("无效版本约束 {requirement}"))
}

fn object<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value.get(key).filter(|value| value.is_object()).ok_or_else(|| anyhow!("缺少对象字段 {key}"))
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    value.get(key).and_then(Value::as_array).ok_or_else(|| anyhow!("缺少数组字段 {key}"))
}

fn optional_array<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    value.get(key).and_then(Value::as_array).into_iter().flatten()
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| anyhow!("缺少字符串字段 {key}"))
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn optional_string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn unique_ids<'a>(value: &'a Value, key: &str) -> Result<HashSet<&'a str>> {
    let mut ids = HashSet::new();
    for item in optional_array(value, key) {
        let id = string(item, "id")?;
        if !ids.insert(id) {
            bail!("META001: {key} 存在重复 ID {id}");
        }
    }
    Ok(ids)
}

fn integer(value: &Value, key: &str) -> Result<i64> {
    value.get(key).and_then(Value::as_i64).ok_or_else(|| anyhow!("缺少整数字段 {key}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_hex_literals_match_integer_and_number_types() {
        assert!(literal_matches_type("0x3c", "integer"));
        assert!(literal_matches_type("-0X10", "number"));
        assert!(!literal_matches_type("0x", "integer"));
    }

    fn examples() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/metadata-standard/examples")
    }

    #[test]
    fn portable_three_file_profile_loads_and_filters_board_pins() {
        let boards = discover_boards().unwrap();
        assert_eq!(boards.len(), 1);
        assert_eq!(boards[0].id, "LKDMX");
        assert_eq!(boards[0].name, "地猛星");
        let profile = load_board("LKDMX").unwrap();
        assert_eq!(profile.chip_id, "mspm0g3507");
        assert!(!profile.member_completions("iq", "sin").is_empty());
        let pins = profile.resource_completions(Some("i2c"), "write_on", 1, &["PA0".into()]);
        assert!(!pins.is_empty());
        assert!(pins.iter().all(|pin| pin.label.starts_with('P')));
    }

    #[test]
    fn example_profile_loads_and_filters_i2c_route() {
        let root = examples();
        let profile = load_profile_files(
            &root.join("mspm0g3507-lqfp48.chip.json"),
            &root.join("launchpad-mspm0g3507.board.json"),
            &root.join("mspm0-lua.api.json"),
        )
        .unwrap();
        let scl = profile.resource_completions(Some("i2c"), "open", 1, &["1".into()]);
        assert_eq!(scl.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(), ["PA15"]);
        let sda = profile.resource_completions(
            Some("i2c"),
            "open",
            2,
            &["1".into(), "'PA15'".into()],
        );
        assert_eq!(sda.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(), ["PA16"]);
        assert!(profile
            .validate_call(
                Some("i2c"),
                "open",
                &["1".into(), "'PA15'".into(), "'PA16'".into(), "100000".into()],
            )
            .is_empty());
    }

    #[test]
    fn semantic_validation_rejects_unknown_board_pin_reference() {
        let root = examples();
        let chip: Value = serde_json::from_str(
            &fs::read_to_string(root.join("mspm0g3507-lqfp48.chip.json")).unwrap(),
        )
        .unwrap();
        let mut board: Value = serde_json::from_str(
            &fs::read_to_string(root.join("launchpad-mspm0g3507.board.json")).unwrap(),
        )
        .unwrap();
        let api: Value = serde_json::from_str(
            &fs::read_to_string(root.join("mspm0-lua.api.json")).unwrap(),
        )
        .unwrap();
        board["pins"][0]["chip_pin"] = Value::String("PA999".into());
        let error = build_profile(&chip, &board, &api).unwrap_err().to_string();
        assert!(error.contains("PA999"));
    }

    #[test]
    fn schema_rejects_unknown_core_fields() {
        let mut chip: Value = serde_json::from_str(
            &fs::read_to_string(examples().join("mspm0g3507-lqfp48.chip.json")).unwrap(),
        )
        .unwrap();
        chip["typo_field"] = Value::Bool(true);
        let schema: Value = serde_json::from_str(CHIP_SCHEMA).unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        assert!(validator.iter_errors(&chip).next().is_some());
    }

    #[test]
    fn project_registry_rejects_example_quality_for_production() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project = std::env::temp_dir().join(format!(
            "mspm0-metadata-test-{}-{nonce}",
            std::process::id()
        ));
        let registry = project.join("metadata");
        fs::create_dir_all(&registry).unwrap();
        let root = examples();
        for name in [
            "mspm0g3507-lqfp48.chip.json",
            "launchpad-mspm0g3507.board.json",
            "mspm0-lua.api.json",
        ] {
            fs::copy(root.join(name), registry.join(name)).unwrap();
        }
        let target = ProjectTarget {
            board: "ti.launchpad-mspm0g3507.rev-a".into(),
            board_version: "0.1.0".into(),
            api: "mspm0.lua-bytecode".into(),
            api_version: "0.1.0".into(),
        };
        let error = load_for_project(&project, &target).unwrap_err().to_string();
        assert!(error.contains("production metadata"));
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn production_profile_loads_iq_and_filters_board_routes() {
        let profile = load_board("LKDMX").unwrap();

        let iq = profile.member_completions("iq", "sin_");
        assert_eq!(
            iq.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(),
            ["sin_deg"]
        );

        let scl = profile.resource_completions(
            Some("i2c"),
            "write_on",
            1,
            &["1".into()],
        );
        let scl_names: HashSet<_> = scl.iter().map(|item| item.label.as_str()).collect();
        assert!(scl_names.contains("PA15"));
        assert!(scl_names.contains("PB2"));
        assert!(!scl_names.contains("PA10"));
        assert!(!scl_names.contains("PB14"));

        let pb2_sda = profile.resource_completions(
            Some("i2c"),
            "write_on",
            2,
            &["1".into(), "'PB2'".into()],
        );
        let pb2_sda_names: HashSet<_> = pb2_sda.iter().map(|item| item.label.as_str()).collect();
        assert!(pb2_sda_names.contains("PB3"));
        assert!(!pb2_sda_names.contains("PA1"));

        let valid = profile.validate_call(
            Some("i2c"),
            "write_on",
            &[
                "1".into(),
                "'PA15'".into(),
                "'PA16'".into(),
                "60".into(),
                "'x'".into(),
                "100000".into(),
            ],
        );
        assert!(valid.is_empty(), "{valid:?}");

        let imu_bus = profile.validate_call(
            Some("i2c"),
            "write_on",
            &[
                "1".into(), "'PB2'".into(), "'PB3'".into(),
                "0x68".into(), "'x'".into(), "100000".into(),
            ],
        );
        assert!(imu_bus.is_empty(), "{imu_bus:?}");
        let wrong_imu_bus = profile.validate_call(
            Some("i2c"),
            "write_on",
            &[
                "0".into(), "'PB2'".into(), "'PB3'".into(),
                "0x68".into(), "'x'".into(), "100000".into(),
            ],
        );
        assert!(!wrong_imu_bus.is_empty(), "PB2/PB3 must not validate on I2C0");

        let uart2 = profile.validate_call(
            Some("uart"), "open",
            &["2".into(), "'PA23'".into(), "'PA24'".into(), "115200".into()],
        );
        assert!(uart2.is_empty(), "{uart2:?}");
        let wrong_uart = profile.validate_call(
            Some("uart"), "open",
            &["1".into(), "'PA23'".into(), "'PA24'".into(), "115200".into()],
        );
        assert!(!wrong_uart.is_empty(), "PA23/PA24 must not validate on UART1");

        for pin in ["PA27", "PA26", "PA25", "PA24"] {
            let adc = profile.validate_call(Some("adc"), "read", &[format!("'{pin}'")]);
            assert!(adc.is_empty(), "{pin}: {adc:?}");
        }
    }

}
