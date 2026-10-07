# 发布 ahaX Windows 测试版

仓库与更新源为 `diaomin66/ahaX`。Beta 1 使用版本 `0.14.0`；版本号沿用已有安装序列，避免已有用户无法升级。发布标题标明 Beta 1，文档保留测试阶段说明。历史标签和附件保持原样，旧仓库地址继续跳转。

## 发布流程

1. 同步 package、Tauri、Cargo 与 lockfile 的版本，准备 `docs/releases/X.Y.Z.md`。
2. 执行前端类型、单元与交互回归；串行运行 Rust 测试和 Windows 构建，验证隔离迁移、原生功能和更新验签。
3. 按用户要求使用 Conventional Commit 提交，创建不可变的 `vX.Y.Z` 标签。提交信息与 PR 文本遵守仓库的身份标识禁用规则。
4. 推送标签触发 Windows MSVC 发布工作流。CI 检查版本、前后端测试、签名打包与隔离安装迁移，生成更新清单和 SHA-256。
5. 从公开地址重新下载四个资源，核对 GitHub digest、大小、清单版本及独立签名。运行只下载和验签的更新探针。
6. 提取公开包，在独立临时目录运行原生界面与数据功能验收；记录实际结果至 `docs/validation/X.Y.Z.md`。

公开资源固定为 `ahaX_X.Y.Z_x64-setup.exe`、同名 `.sig`、`latest.json`、`SHA256SUMS-X.Y.Z.txt`。更新安装需由用户在软件中触发；验收不运行真实安装器，不覆盖现用配置、凭据、线程或快捷方式。

发布失败可通过 workflow_dispatch 指定原标签重试。已发布附件和标签不覆盖，修复使用新版本号。

## 签名与更新

Tauri updater 验证 Minisign 签名和签名绑定的版本。公钥随程序打包，更名继续使用既有公钥；私钥与密码只从发布密钥存储或 GitHub Actions secrets 加载。

- `TAURI_SIGNING_PRIVATE_KEY`：加密私钥。
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：对应密码。

不得将签名材料、渠道 Key 或用户数据写入日志和附件。更新签名与 Windows Authenticode 不同；当前未配置 Authenticode 证书。

本地准备附件：

```powershell
node scripts/release-manifest.mjs --tag v0.14.0 --repository diaomin66/ahaX --notes-file docs/releases/0.14.0.md
```

后台检查并按偏好下载新版；安装必须由用户点击触发。验证探针只调用下载与验签，不安装。发布后还要验证旧仓库的更新地址可以通过跳转获取新清单。

## 品牌与兼容边界

| 当前标识 | 值 |
| --- | --- |
| 产品名与默认服务商 | `ahaX` |
| 进程与程序文件 | `ahax.exe` |
| 应用标识 | `app.ahax.desktop` |
| 默认应用数据 | `%LOCALAPPDATA%\ahaX\data` |
| 渠道凭据 | `ahaX/connection/<UUID>` |
| 独立数据覆盖 | `AHAX_DATA_DIR` |
| 新模型路由 | `ahax-<hash>` |
| 主题偏好 | `ahax:appearance:v1` |

旧目录、凭据、加密封套、模型编号和主题偏好只用于兼容读取与迁移。用户自定义位置、服务商名称、历史评测标识和会话内容不批量替换。历史版本的发布说明和验收记录保留当时名称。

安装器验证旧注册信息后沿用已有安装位置，更新目标程序与图标，保护指向其他程序的同名快捷方式。保留旧数据源用于恢复，不把源目录删除算作品牌清理。文件名变化后，受管凭据命令需重绑到新程序；外部服务商配置必须保持原样。

已有直连会话仍绑定原服务商，不能仅通过切换模型将其迁移至本机网关。将此行为写入帮助与验收，不能宣称更名前缀解决了旧会话的 404。
