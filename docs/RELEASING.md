# 发布 Windows 版本

仓库与下载：`diaomin66/Vela`。公开 Release 的 `latest.json` 是软件内置更新源，固定指向具体版本的安装包，不读取任意渠道 API。

## 更新信任链

Tauri updater 验证安装包的 Minisign 签名及签名绑定的版本。公钥位于 `src-tauri/tauri.conf.json`；私钥不进入仓库。更新签名与 Windows Authenticode 代码签名是不同机制，当前安装器尚未配置 Authenticode 证书。

发布用 GitHub Actions secrets：

- `TAURI_SIGNING_PRIVATE_KEY`：Tauri 加密私钥全文。
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：对应密码。

这些 secrets 只由打标签触发的发布工作流使用。不要在日志或 Release 附件中上传私钥、密码、本机配置、渠道 Key、测试数据目录。必须独立安全备份签名密钥；丢失后已安装版本不能信任新公钥，需要用户手动安装迁移版本。

## 发布流程

1. 同步修改 `package.json` / `package-lock.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` / `Cargo.lock` 中的应用版本。
2. 在 `docs/releases/X.Y.Z.md` 编写更新说明。运行前后端测试和 Windows 原生验收。
3. 提交源码，创建对应 `vX.Y.Z` 标签并推送。工作流使用 Windows MSVC 构建，校验各处版本一致，生成签名安装包、`latest.json` 和 SHA-256，然后发布为最新 Release。
4. 检查公开 Release 的四个文件：`Vela_X.Y.Z_x64-setup.exe`、同名 `.sig`、`latest.json`、`SHA256SUMS-X.Y.Z.txt`。验证应用能读取新版本，下载校验通过。

失败后可通过工作流手动输入已有标签重试；不要覆盖已经公开的版本。修复已发布版本时发布新的版本号。自动更新源只发布正式语义版本标签，不将预发行版本设置为 latest。

## 本地生成发布附件

在当前进程设置 `TAURI_SIGNING_PRIVATE_KEY` 与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 后运行 `npm run desktop:build`。Tauri 会签署 NSIS 安装包，并把版本写入签名的 trusted comment。

```powershell
node scripts/release-manifest.mjs --tag v0.4.0 --repository diaomin66/Vela --notes-file docs/releases/0.4.0.md
```

脚本校验版本一致、Windows 可执行文件和签名存在，输出至被 Git 忽略的 `release/`。脚本的元数据检查不能代替 updater 的密码学签名验证。

更新默认在启动后延迟检查，并周期检查；有新版时自动下载并验证，保存在进程内存中。安装必须由用户在设置里点击触发，界面会提醒先结束正在运行的任务。退出应用会释放下载内容，再次启动会重新获取。关闭自动下载后，后台仍会检查版本，下载由用户手动开始。
