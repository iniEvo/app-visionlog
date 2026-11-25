# PRD — visionlog · DCP Look 移植工具

> 版本 0.1 · 2026-11-23 · 状态：定稿（三 A 决策）

## 1. 一句话

把任意相机 look 配置文件（如 Sony VisionLOG `.dcp`）**真色彩移植**到任意目标相机机型，生成新的 `.dcp` 并一键安装进 Photoshop Camera Raw，macOS + Windows 双平台。

## 2. 用户与场景

### 目标用户
- **摄影后期用户**：买了 VisionLOG（或任何 DCP look），换了新相机机身，官方还没出对应机型的 DCP
- **调色师**：想让一套 look 覆盖手头多台机身（多机位一致性）
- **教程作者**：给粉丝一键生成对应机型的配置文件

### 核心场景（User Story）
1. 用户拖入一份 `.dcp` look 源文件（VisionLOG / 富士模拟 / 任何 DCP）
2. 用户拖入一张目标相机的 RAW 样片（或从下拉列表选机型）
3. 点「生成」→ 软件用该机型的真实色彩矩阵重新运算 LookTable，输出新 `.dcp`
4. 点「安装到 Photoshop」→ 写入 Adobe CameraRaw profile 目录，重开 PS 即见

## 3. 功能需求

### F1 — Look 源导入（P0）
- 支持拖拽任意 `.dcp` 文件
- 解析并展示：ProfileName、UniqueCameraModel、LookTable 维度、ToneCurve 点数
- 校验：缺 LookTable 的 DCP 给出警告（仍可走矩阵直换路径）

### F2 — 目标机型选择（P0）
- **主路径**：拖入目标相机 RAW 样片 → 读 EXIF `UniqueCameraModel` → 自动填入
- **备路径**：内置机型下拉列表（搜索过滤），数据来自 4 层矩阵库（见 ARCHITECTURE）
- **兜底**：高级模式手填字符串（提示逐字符匹配要求）

### F3 — 色彩移植引擎（P0 · 核心）
- fork extra-sauce 算法（MIT）为内部模块 `engine/`
- 数学：`新LookTable = srcLook ∘ srcHSM ∘ (srcStdHSM)⁻¹`，再叠矩阵差 `M = P⁻¹·FM_src·FM_std⁻¹·P`
- 200k 采样点最小二乘拟合 + 未达节点 Laplacian 平滑
- 自检：移植后跑 ΔE76 验证，UI 显示平均 ΔE
- 输出 tag 集合：目标机型 Adobe Standard 的全部矩阵 tag + 重算的 LookTable/ToneCurve + `ProfileLookTableEncoding=1`（sRGB）+ 显式 ToneCurve + 逐字节保留 BaselineExposureOffset

### F4 — 生成与安装（P0）
- 生成到用户选择的输出目录，文件名 `<Make Model> <LookName>.dcp`
- 「安装到 Photoshop」按钮：
  - macOS：`~/Library/Application Support/Adobe/CameraRaw/CameraProfiles/<Group>/`
  - Windows：`%APPDATA%\Adobe\CameraRaw\CameraProfiles\<Group>\`
  - 自动建目录，无需管理员权限
  - 安装后提示「重启 Photoshop / Lightroom 生效」
- 「打开 Adobe Profile 文件夹」按钮（Explorer/Finder）

### F5 — 质量报告（P1）
- 移植完成后显示：平均 ΔE、最大 ΔE、未达节点数、源/目标矩阵来源层
- 生成对比预览图（P2，可用固定色卡渲染）

### F6 — 批量机型（P2）
- 一次选多个目标机型，队列式逐个移植生成

### F7 — LUT 输入（P3，暂缓）
- 接受 `.cube` LUT 作为 look 源（反向 DCP 化）

## 4. 非功能需求

| 项 | 要求 |
|---|---|
| 平台 | macOS 12+ / Windows 10 21H2+ |
| 架构 | Apple Silicon + Intel（universal），Windows x64 |
| 体积 | 安装包 < 40 MB（含 Python sidecar） |
| 移植耗时 | 单机型 ≤ 30 s（M1 基准） |
| 离线 | 全离线运行，无遥测 |
| 开源 | MIT，公开依赖声明（extra-sauce / rawspeed / libraw 署名） |

## 5. 边界与不做

- 不做 RAW 解码渲染预览（不做 mini-Lightroom）
- 不内置、不再分发 Adobe Standard DCP 字节（只在运行时读用户本机已装的）
- 不做视频 LUT 导出（P3 后议）
- 不替用户绕过任何 look 的版权——工具只做转换，不提供盗版 look 下载

## 6. 里程碑

| 里程碑 | 内容 | 验收标准 |
|---|---|---|
| M0 脚手架 | Tauri 项目跑起来，Rust DCP 解析器单测过 | `cargo test` 绿；读 A7M5 样例 DCP 输出全 tag |
| M1 引擎 MVP | fork extra-sauce，CLI 跑通 A7M5→虚拟机型的移植 | 生成的 DCP 被 ACR 识别并列出 |
| M2 GUI | 拖拽 + 机型选择 + 生成 + 安装按钮 | 从拖入到 ACR 里看到新 profile ≤ 2 分钟 |
| M3 质量报告 | ΔE 自检 + UI 报告 | VisionLOG 源移植平均 ΔE ≤ 2 |
| M4 打包发布 | 双平台安装包 + 签名公证 | macOS notarize 过，Win SmartScreen 不红 |

## 7. 风险

| 风险 | 缓解 |
|---|---|
| extra-sauce 算法在某些 look 上不收敛 | 保留字符串直换作为降级路径（同 VisionLOG 官方做法） |
| Adobe 改 profile 目录约定 | 路径探测做成配置，多个候选路径依次尝试 |
| Python sidecar 体积 | 用 python-build-standalone 或转 PyO3 后期把 numpy 部分搬进 Rust |
| 用户 RAW 非 DNG 无矩阵 | 4 层 fallback 链保证覆盖；UI 显示当前用的是第几层数据 |
