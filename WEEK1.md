# 第一周开发计划

> 目标：M0 脚手架可运行 + M1 引擎 MVP 跑通一个真实移植案例

## Day 1（周一）：脚手架跑通

- [ ] `cd src-tauri && cargo check` 通过（修 tauri.conf.json 路径/插件依赖）
- [ ] `pnpm install && pnpm tauri dev` 起界面
- [ ] 验证 `inspect_dcp` 命令在前端能调通（临时按钮触发）
- [ ] 跑通 `dcp.rs` 三个单测：
  ```
  cargo test --package visionlog dcp::tests
  ```

**验收**：GUI 打开，能选 A7M5 DCP，前端显示 `profile_name: VisionLOG Sony A7M5`

## Day 2（周二）：矩阵 fallback 链

- [ ] 跑通 `matrix.rs` 单测（A7M5 命中层 ②）
- [ ] 实现 `find_adobe_standard_dcp` 模糊匹配（处理 " v2" 后缀）
- [ ] 前端下拉框 `list_installed_cameras` 填充 ≥ 1000 个机型
- [ ] 在 About/EULA 文档写明数据源与许可

**验收**：选 A7M5 时 UI 显示"数据源：本机 Adobe Standard"，拿到 CM1/FM1

## Day 3（周三）：vendor extra-sauce

- [ ] `git clone https://github.com/wrkxflw/extra-sauce` 到 /tmp
- [ ] 拷贝模块到 `engine/`：dcp.py / pipeline.py / transfer.py / converter.py / accuracy.py / acr3_curve.txt
- [ ] 保留原版权头，加 NOTICE 说明 fork 来源
- [ ] 在 engine/ 下跑通：`python -c "from dcp import read_dcp; print(read_dcp('../Sony ILCE-7M5 Camera VisionLOG.dcp').name)"`
- [ ] 安装 numpy：`pip install numpy`

**验收**：`engine/main.py` 能 import 所有模块

## Day 4（周四）：引擎端到端

- [ ] 拿一份真实的 A7M4 RAW 转 DNG，生成 Adobe Standard 参考
- [ ] 写临时脚本 `engine/manual_test.py`：`A7M5 VisionLOG → A7M4 机型` 移植
- [ ] 输出 `out/A7M4-VisionLOG.dcp`，验证：
  - Adobe Camera Raw 能读到 profile 列表里
  - ΔE 自检 ≤ 2
- [ ] 如 extra-sauce 接口与假设不符，调整 main.py 协议

**验收**：手工跑 `cat req.json | python3 engine/main.py` 产出合法 DCP

## Day 5（周五）：sidecar 接入前端

- [ ] 跑通 `run_transfer` Tauri 命令（从 GUI 触发）
- [ ] 进度事件流在 UI 显示
- [ ] 「安装到 Photoshop」按钮写入 `~/Library/.../CameraProfiles/`
- [ ] 「打开 Adobe Profile 文件夹」能弹 Finder

**验收**：从 GUI 拖入 → 选机型 → 生成 → 安装 → 重开 PS 看到新 profile

## Week 2 规划（预告）

- Day 6-7：rawmeta 从 RAW 读 UniqueCameraModel（去掉手填）
- Day 8-9：ΔE 报告 UI 美化 + 色卡预览（可选）
- Day 10：打 macOS 安装包，notarize
- Day 11-12：Windows 构建（NSIS installer），签名测试
- Day 13-14：README + About 署名页 + GitHub release

## 风险预案

| 阻塞点 | 预案 |
|---|---|
| extra-sauce 接口和假设不一致 | 直接读它的 `transfer.py::build_look` 源码，1:1 改 main.py 协议 |
| Python sidecar 打包体积 > 200MB | 用 `--onefile` + `--strip`；或 M2 后评估 PyO3 |
| Windows SmartScreen 拦截 | 申请 Microsoft 代码签名证书（约 $400/年）；MVP 阶段可先发未签名版 |
| Adobe 改路径约定 | 在 `matrix.rs::adobe_standard_search_roots()` 加多个候选 |
