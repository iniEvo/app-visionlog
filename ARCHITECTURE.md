# ARCHITECTURE — visionlog

> 对应 PRD v0.1。技术架构、数据流、模块边界。

## 1. 总体架构

```
┌──────────────────────────────────────────────────────┐
│  Tauri v2 Shell                                       │
│  ┌────────────────────────────────────────────────┐  │
│  │  Frontend (React + TS, ~轻量 UI)               │  │
│  │  · 拖拽源 DCP / RAW                             │  │
│  │  · 机型选择（自动识别 + 下拉）                    │  │
│  │  · 移植进度 + ΔE 报告                            │  │
│  │  · 「安装到 Photoshop」                          │  │
│  └───────────────┬────────────────────────────────┘  │
│                  │ Tauri IPC (invoke / events)       │
│  ┌───────────────▼────────────────────────────────┐  │
│  │  Rust Core (src-tauri/src/)                    │  │
│  │  · dcp: DCP 解析/写入（TIFF-IFD，纯 Rust）      │  │
│  │  · matrix: 4 层矩阵 fallback 链                 │  │
│  │  · install: Adobe 目录探测 + 安装               │  │
│  │  · rawmeta: EXIF UniqueCameraModel 读取         │  │
│  │  · sidecar: Python 引擎进程管理                 │  │
│  └───────────────┬────────────────────────────────┘  │
│                  │ stdin/stdout JSON 协议             │
│  ┌───────────────▼────────────────────────────────┐  │
│  │  Python Engine (engine/ · fork of extra-sauce)  │  │
│  │  · transfer: LookTable 重算（numpy）             │  │
│  │  · 自检 ΔE76                                   │  │
│  └────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────┘
```

**为什么 Python sidecar 而不是全 Rust**：移植数学是 200k 采样 ×（Newton 迭代反解 + 最小二乘拟合），extra-sauce 的 numpy 实现已验证（ΔE≈1）。第一版先 vendor 进来跑通，M3 后再评估 PyO3/纯 Rust 重写。

## 2. 数据流（核心路径）

```
[源 DCP] ──Rust 解析──► { CM1/2, FM1/2, HSM, LookTable, ToneCurve, name }
                         │
[目标机型] ──4 层矩阵链──► { CM1/2, FM1/2, Illum1/2 }（目标 Adobe Standard 的）
                         │
                         ▼
              Python transfer.build_look()
              T = srcLook ∘ srcHSM ∘ (srcStdHSM)⁻¹
              （若源/目标 FM 不同：预叠 M = P⁻¹·FM_src·FM_std⁻¹·P）
                         │
                         ▼
              新 LookTable (90×25×16) + 显式 ToneCurve
                         │
                         ▼
              Rust 写 DCP：目标 Adobe Standard 全部矩阵 tag 原样保留
                        + 重算的 LookTable/ToneCurve/Encoding=1
                        + ProfileName=源 look 名 + EmbedPolicy=0
                         │
                         ▼
              install() → {macOS,Win} 用户级 CameraProfiles/<Group>/
```

## 3. 模块规格

### 3.1 `dcp`（Rust · src-tauri/src/dcp/）

DCP = TIFF（II/MM + magic 0x4352 'CR'）+ 单 IFD + DNG profile tags。

```rust
pub struct Dcp {
    pub entries: BTreeMap<u16, TagEntry>,   // tag → (type, count, bytes)
    pub byte_order: ByteOrder,
}
pub struct TagEntry { pub typ: u16, pub count: u32, pub data: Vec<u8> }

impl Dcp {
    pub fn parse(bytes: &[u8]) -> Result<Self>;
    pub fn get_str(&self, tag: u16) -> Option<String>;        // ASCII/BYTE-UTF8
    pub fn get_srational9(&self, tag: u16) -> Option<[f64; 9]>;
    pub fn get_floats(&self, tag: u16) -> Option<Vec<f32>>;
    pub fn get_long(&self, tag: u16) -> Option<u32>;
    pub fn get_short(&self, tag: u16) -> Option<u16>;
    pub fn set(&mut self, tag: u16, entry: TagEntry);
    pub fn serialize(&self) -> Vec<u8>;   // 重排 IFD，数据偏移重算
}
```

**关键 tag 表**（已交叉验证 ExifTool / RawTherapee / dcptool / dcamprof 四源）：

| Tag | Hex | 用途 |
|---|---|---|
| 50708 | 0xC614 | UniqueCameraModel（ASCII）· 匹配键 |
| 50721/22 | 0xC621/22 | ColorMatrix1/2（SRATIONAL×9） |
| 50723/24 | 0xC623/24 | CameraCalibration1/2 |
| 50725/26 | 0xC725/26 | ReductionMatrix1/2（4 通道机身） |
| 50778/79 | 0xC65A/5B | CalibrationIlluminant1/2 |
| 50932 | 0xC6F4 | ProfileCalibrationSignature |
| 50936 | 0xC6F8 | ProfileName |
| 50937 | 0xC6F9 | ProfileHueSatMapDims |
| 50938/39 | 0xC6FA/FB | ProfileHueSatMapData1/2（FLOAT×3n） |
| 50940 | 0xC6FC | ProfileToneCurve（FLOAT×2n） |
| 50941 | 0xC6FD | ProfileEmbedPolicy |
| 50942 | 0xC6FE | ProfileCopyright |
| 50964/65 | 0xC714/15 | ForwardMatrix1/2（SRATIONAL×9） |
| 50981 | 0xC725 | ProfileLookTableDims（LONG×3） |
| 50982 | 0xC726 | ProfileLookTableData（FLOAT×3n） |
| 51107 | 0xC7A3 | ProfileHueSatMapEncoding（0=Linear,1=sRGB） |
| 51108 | 0xC7A4 | ProfileLookTableEncoding |
| 51109 | 0xC7A5* | BaselineExposureOffset（SRATIONAL） |
| 52552 | 0xCD28 | ProfileGroupName |

> 注意：0xC7A6=51110 是 DefaultBlackRender。BaselineExposureOffset = 51109 (0xC7A5)。写入时**逐字节保留**源文件的该 tag。

**未建模 tag 的 passthrough**：解析时把不认识的 tag 原样存进 entries，写回时按序输出——RGBTables / BigTable* / GainTableMap 等 DNG 1.6 tag 得以幸存（参考 dcptool-python 的 passthrough 机制）。

### 3.2 `matrix`（Rust · 4 层 fallback 链）

```rust
pub enum MatrixSource { Dng, AdobeStandard, LibRaw, RawspeedXml }

pub fn resolve_camera(make: &str, model: &str) -> Result<CameraMatrices, MatrixSource> {
    // ① 输入是 .dng → 读 DNG tag
    // ② 本机 ACR：扫描 CameraProfiles 目录树，匹配 "<Make Model> Adobe Standard.dcp"
    //    macOS: /Library/.../CameraProfiles/Adobe Standard/  +  ~/Library/...
    //    Win:   %ProgramData%\Adobe\CameraRaw\CameraProfiles\Adobe Standard\
    //           + %APPDATA%\Adobe\CameraRaw\CameraProfiles\Adobe Standard\
    // ③ libraw colordata 表（打包成 Rust 常量表，CDDL-1.0）
    // ④ rawspeed cameras.xml（打包进资源，CC-BY-SA 3.0 → About 署名）
}

pub struct CameraMatrices {
    pub cm1: [f64; 9], pub cm2: Option<[f64; 9]>,
    pub fm1: [f64; 9], pub fm2: Option<[f64; 9]>,
    pub ill1: u16, pub ill2: Option<u16>,
    pub source: MatrixSource,     // UI 显示数据可信度
}
```

各层路径候选（探测顺序）：

```
macOS:
  /Library/Application Support/Adobe/CameraRaw/CameraProfiles/
  ~/Library/Application Support/Adobe/CameraRaw/CameraProfiles/
  /Applications/Adobe Photoshop*/Adobe Photoshop*.app/Contents/Resources/CameraProfiles/
Windows:
  %ProgramData%\Adobe\CameraRaw\CameraProfiles\
  %APPDATA%\Adobe\CameraRaw\CameraProfiles\
  %ProgramFiles%\Adobe\Adobe Photoshop*\Required\CameraProfiles\   (探测)
```

匹配规则：`Adobe Standard/<model 字符串> Adobe Standard.dcp`（大小写不敏感，v2 后缀取最新）。**匹配键 = UniqueCameraModel 字符串，逐字节一致**。

### 3.3 `engine/`（Python · fork of extra-sauce）

vendor 进来的模块，改造点：

| 原文件 | 改造 |
|---|---|
| `app/dcp.py` | 删除 GUI 相关；保留 reader |
| `app/pipeline.py` | 保留（渲染管线：FM 插值→HSM→LookTable→ToneCurve） |
| `app/transfer.py` | 保留核心 `build_look` / `fit_look` / `fill_unreachable` |
| `app/converter.py` | 重写为 CLI 入口：`engine/main.py`，stdin JSON → stdout JSON |
| `app/acr3_curve.txt` | 保留（ACR3 默认 ToneCurve） |

**sidecar 协议**（Rust ↔ Python，一行 JSON 每条）：

```jsonc
// Rust → Python (stdin)
{"cmd": "transfer",
 "src_dcp": "/path/to/look.dcp",
 "std_dcp": "/path/to/target Adobe Standard.dcp",   // 或矩阵参数
 "out_dcp": "/path/to/out.dcp",
 "src_name": "VisionLOG Sony A7M5"}
// Python → Rust (stdout, 逐事件)
{"event": "progress", "stage": "fit", "pct": 42}
{"event": "done", "avg_de": 1.03, "max_de": 2.7, "unreached": 3}
{"event": "error", "msg": "..."}
```

打包：`python-build-standalone`（cosmo 或 indygreg 发行版）+ numpy wheel，打进 Tauri `resources/`。目标 sidecar 总体积 < 60 MB。

### 3.4 `install`（Rust）

```rust
pub fn adobe_profile_dirs() -> Vec<PathBuf>;   // 按平台枚举候选（用户级优先）
pub fn install(dcp: &Path, group: &str) -> Result<InstalledPath>;
// 行为：建 <dir>/<Group>/，拷贝 .dcp，返回最终路径
// 冲突：同名文件先备份为 .bak（可回滚）
// 完成后返回提示文案：「重启 Photoshop / Lightroom 后生效」
```

Windows 写用户级 `%APPDATA%` 不需要 UAC。**不碰** `C:\ProgramData`（系统级，需提权 + 污染所有账户）。

### 3.5 `rawmeta`（Rust）

- `.dng`：直接走 `dcp` 模块读 tag
- 厂商 RAW（.arw/.cr3/.nef/.raf）：读 TIFF IFD0 的 `Make` + `Model`，映射到 Adobe 的 UniqueCameraModel 字符串（映射表打包自 libraw 的模型清单 + 本机 ACR 实测索引）

## 4. 前端（React）

三个屏 + 一个结果页，无路由库：

```
[导入] 源 DCP 拖拽区 + RAW 拖拽区/机型下拉
[生成] 进度（stage/pct 事件流）+ 取消
[结果] ΔE 报告 + 「安装到 Photoshop」 + 「打开文件夹」
```

状态管理 zustand（或纯 hooks），UI 不引入重组件库——用 Tailwind + 少量手写组件，控制体积。

## 5. 测试策略

| 层 | 测试 |
|---|---|
| dcp | A7M5 样例 DCP round-trip：parse→serialize 字节一致（passthrough tag 幸存）；42 台本机 Adobe Standard 全部解析通过 |
| matrix | A7M5 四层全命中断言；未知机型逐层降级断言 |
| engine | self-check：源机型自移植 ΔE≈0；A7M5→A7M4 移植 ΔE 均值 ≤ 2 |
| install | 干跑目录（tempdir）断言；幂等重装 |
| E2E | 生成 DCP 丢进 ACR 目录后，PS 2026 的 profile 列表出现（手工验收） |

## 6. 目录结构

```
visionlog/
├── PRD.md
├── ARCHITECTURE.md
├── package.json            # 前端
├── src/                    # React
│   ├── App.tsx
│   ├── screens/
│   └── ipc.ts
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── capabilities/
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── dcp/            # 解析+写入+测试
│       ├── matrix/
│       ├── rawmeta/
│       ├── install.rs
│       └── sidecar.rs
├── engine/                 # Python fork (extra-sauce)
│   ├── main.py             # stdin/stdout 协议
│   ├── dcp.py              # reader (vendor)
│   ├── pipeline.py
│   ├── transfer.py
│   ├── converter.py
│   └── acr3_curve.txt
└── assets/
    └── cameras.xml         # rawspeed（CC-BY-SA 署名进 About）
```

## 7. 许可合规清单

| 组件 | 许可 | 动作 |
|---|---|---|
| extra-sauce | MIT | vendor 进 engine/，保留版权头 |
| libraw colordata | LGPL-2.1 OR CDDL-1.0 | 只取矩阵数值表，选 CDDL，NOTICE 声明 |
| rawspeed cameras.xml | CC-BY-SA 3.0 | 打包 + About 页署名（SA 只约束 XML） |
| Adobe Standard DCP | Adobe EULA | **只运行时读用户本机，绝不分发** |
