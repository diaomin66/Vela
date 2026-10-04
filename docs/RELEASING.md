# 发布 AhaX Windows 版本

仓库与下载：`diaomin66/Vela`。公开 Release 的 `latest.json` 是软件内置更新源，固定指向具体版本的安装包，不读取任意渠道 API。

0.7.0 起产品名称为 AhaX，安装包前缀为 `AhaX`；仓库地址和既有更新公钥不随品牌变更。历史 Release 附件保留原名。

## 更新信任链

Tauri updater 验证安装包的 Minisign 签名及签名绑定的版本。公钥位于 `src-tauri/tauri.conf.json`；私钥不进入仓库。更新签名与 Windows Authenticode 代码签名是不同机制，当前安装器尚未配置 Authenticode 证书。

发布用 GitHub Actions secrets：

- `TAURI_SIGNING_PRIVATE_KEY`：Tauri 加密私钥全文。
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：对应密码。

这些 secrets 只由发布工作流使用，支持推送标签触发及指定已有标签重试。不要在日志或 Release 附件中上传私钥、密码、本机配置、渠道 Key、测试数据目录。必须独立安全备份签名密钥；丢失后已安装版本不能信任新公钥，需要用户手动安装迁移版本。

## 发布流程

1. 同步修改 `package.json` / `package-lock.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` / `Cargo.lock` 中的应用版本。
2. 在 `docs/releases/X.Y.Z.md` 编写更新说明。运行前后端测试和 Windows 原生验收，将结果、范围与尚未执行的项目记录在 `docs/validation/X.Y.Z.md`。
3. 提交源码，创建对应 `vX.Y.Z` 标签并推送。工作流使用 Windows MSVC 构建，校验各处版本一致，生成签名安装包、`latest.json` 和 SHA-256，然后发布为最新 Release。
4. 检查公开 Release 的四个文件：`AhaX_X.Y.Z_x64-setup.exe`、同名 `.sig`、`latest.json`、`SHA256SUMS-X.Y.Z.txt`。验证应用能读取新版本，下载校验通过。附件名称由 `tauri.conf.json` 的 `productName` 生成，元数据必须引用这一版本的实际文件。

失败后可通过工作流手动输入已有标签重试；不要覆盖已经公开的版本。修复已发布版本时发布新的版本号。自动更新源只发布正式语义版本标签，不将预发行版本设置为 latest。

## 本地生成发布附件

在当前进程设置 `TAURI_SIGNING_PRIVATE_KEY` 与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 后运行 `npm run desktop:build`。Tauri 会签署 NSIS 安装包，并把版本写入签名的 trusted comment。

```powershell
node scripts/release-manifest.mjs --tag v0.7.0 --repository diaomin66/Vela --notes-file docs/releases/0.7.0.md
```

脚本校验版本一致、Windows 可执行文件和签名存在，输出至被 Git 忽略的 `release/`。脚本的元数据检查不能代替 updater 的密码学签名验证。

更新默认在启动后延迟检查，并周期检查；有新版时自动下载并验证，保存在进程内存中。安装必须由用户在设置里点击触发，界面会提醒先结束正在运行的任务。退出应用会释放下载内容，再次启动会重新获取。关闭自动下载后，后台仍会检查版本，下载由用户手动开始。

## 品牌变更与旧版兼容

产品名用于窗口、托盘、快捷方式、安装包和新导出文件。以下已有标识保持不变：

| 项目 | 兼容值 |
| --- | --- |
| GitHub 仓库与更新源 | `diaomin66/Vela` |
| Tauri 应用标识 | `app.vela.desktop` |
| 程序文件名 | `vela.exe` |
| 默认数据目录 | `%LOCALAPPDATA%\Vela\data` |
| 渠道凭据命名空间 | `Vela/connection/<UUID>` |
| 独立数据目录覆盖项 | `VELA_DATA_DIR` |
| 默认服务商显示名称 | `Vela`，用户可在设置中修改 |

现有 provider 键、路由 ID、主题偏好键、题库版本与历史导出名称继续保留。发布时不要批量替换源码或用户数据中的 `Vela`，也不要换签名公钥来配合产品更名。

安装器迁移逻辑应识别旧版注册的安装路径，在原位置更新程序，再按实际拥有的快捷方式和注册项迁移产品名称，避免已有凭据命令引用失效。品牌迁移的源代码检查、隔离安装器验证与真实安装升级是不同层次；分别记录结果，未运行实际升级时不得宣称已经验收用户升级路径。不得为了验收覆盖开发者正在使用的安装、桌面快捷方式或真实渠道资料。

## 0.7.0 验收重点

- 单次与定时入口、记录来源隔离、超时兼容、取消与后台调度。
- 单条与批量整轮删除、运行中保护、索引及报告恢复、导出文件保留。
- 自动动画与重播、作品预览源和实际 WebView2 权限边界。
- 浅色、深色、跟随系统、主题持久化与窄窗口可访问性。
- 本地诊断的脱敏输出、受管目录修复、恢复点预检与修复后重新检查，范围见 [诊断文档](DIAGNOSTICS.md)。
- 使用公开发行附件复核签名、版本、SHA-256 与更新下载；更新测试只下载和验签，不自动运行安装器。

源代码测试的完成不代表公开附件或旧版安装迁移已经验收。当前完成情况以 [0.7.0 验收记录](validation/0.7.0.md) 为准。
