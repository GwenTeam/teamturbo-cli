# TeamTurbo CLI - Help 信息

## 主命令 Help

```
$ teamturbo --help

TeamTurbo CLI for Docuram

Usage: teamturbo <COMMAND>

Commands:
  login   Login to TeamTurbo
  logout  Logout from TeamTurbo
  whoami  Show current login status
  init    Initialize docuram project
  pull    Pull document updates from server
  push    Push new documents to server
  sync    Sync documents (pull then push)
  diff    Show diff between local and remote
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## 各子命令的 Help

### login

```
$ teamturbo login --help

Login to TeamTurbo

Usage: teamturbo login [OPTIONS]

Options:
      --browser  Force browser authorization mode
      --manual   Force manual token input mode
  -h, --help     Print help
```

**使用示例:**
```bash
# 自动检测模式（默认）
teamturbo login

# 强制使用浏览器模式
teamturbo login --browser

# 强制使用手动输入模式（离线）
teamturbo login --manual
```

### logout

```
$ teamturbo logout --help

Logout from TeamTurbo

Usage: teamturbo logout

Options:
  -h, --help  Print help
```

**使用示例:**
```bash
teamturbo logout
```

### whoami

```
$ teamturbo whoami --help

Show current login status

Usage: teamturbo whoami

Options:
  -h, --help  Print help
```

**使用示例:**
```bash
teamturbo whoami
```

### init

```
$ teamturbo init --help

Initialize docuram project

Usage: teamturbo init [OPTIONS]

Options:
      --from <FROM>   Download config from URL
  -f, --force         Force overwrite existing files
      --no-download   Skip downloading documents
  -h, --help          Print help
```

**使用示例:**
```bash
# 从 URL 初始化
teamturbo init --from https://example.com/api/v1/docuram/categories/1/generate_config

# 强制覆盖现有文件
teamturbo init --from <url> --force

# 只下载配置，不下载文档
teamturbo init --from <url> --no-download
```

### pull

```
$ teamturbo pull --help

Pull document updates from server

Usage: teamturbo pull [OPTIONS] [DOCUMENTS]...

Arguments:
  [DOCUMENTS]...  Specific documents to pull (by slug)

Options:
  -f, --force  Force overwrite local changes
  -h, --help   Print help
```

**使用示例:**
```bash
# 拉取所有文档
teamturbo pull

# 拉取特定文档
teamturbo pull doc-slug-1 doc-slug-2

# 强制覆盖本地修改
teamturbo pull --force

# 强制拉取特定文档
teamturbo pull --force doc-slug-1
```

### push

```
$ teamturbo push --help

Push new documents to server

Usage: teamturbo push [OPTIONS] [DOCUMENTS]...

Arguments:
  [DOCUMENTS]...  Specific documents to push (by path)

Options:
  -m, --message <MESSAGE>  Commit message
  -h, --help               Print help
```

**使用示例:**
```bash
# 推送所有修改
teamturbo push

# 推送特定文档
teamturbo push doc-slug-1 doc-slug-2

# 指定变更说明
teamturbo push -m "更新了文档内容"

# 推送特定文档并指定说明
teamturbo push -m "修复错别字" doc-slug-1
```

### sync

```
$ teamturbo sync --help

Sync documents (pull then push)

Usage: teamturbo sync [OPTIONS]

Options:
  -f, --force  Force overwrite conflicts
  -h, --help   Print help
```

**使用示例:**
```bash
# 同步文档（先 pull 再 push）
teamturbo sync

# 强制同步（覆盖冲突）
teamturbo sync --force
```

### diff

```
$ teamturbo diff --help

Show diff between local and remote

Usage: teamturbo diff [DOCUMENT]

Arguments:
  [DOCUMENT]  Specific document to diff (by slug)

Options:
  -h, --help  Print help
```

**使用示例:**
```bash
# 查看所有文档的差异
teamturbo diff

# 查看特定文档的差异
teamturbo diff doc-slug-1
```

## 版本信息

```
$ teamturbo --version
teamturbo 1.0.0
```

## 完整的工作流程示例

### 1. 首次使用

```bash
# 1. 安装 CLI（已通过安装脚本完成）
# 2. 登录认证
teamturbo login

# 3. 查看登录状态
teamturbo whoami

# 4. 初始化项目
teamturbo init --from https://example.com/api/v1/docuram/categories/1/generate_config

# 5. 查看下载的文档
ls -R docuram/
```

### 2. 日常使用

```bash
# 早上开始工作 - 拉取最新文档
teamturbo pull

# 编辑文档
vim docuram/技术架构/系统设计.md

# 查看修改
teamturbo diff

# 推送修改
teamturbo push -m "更新系统设计文档"

# 或者一步到位同步
teamturbo sync
```

### 3. 协作场景

```bash
# 场景：团队成员 A 和 B 同时修改文档

# 成员 A
teamturbo pull
vim docuram/api/接口文档.md
teamturbo push -m "添加新接口"

# 成员 B（稍后）
teamturbo pull  # 拉取成员 A 的更新
vim docuram/api/接口文档.md  # 基于最新版本修改
teamturbo push -m "完善接口说明"
```

### 4. 冲突处理

```bash
# 如果本地有未提交的修改，pull 会提示冲突
teamturbo pull
# ⚠ 2 document(s) have local modifications:
#   - api-docs
# Use --force to overwrite local changes

# 选择 1: 保存本地修改
teamturbo push -m "保存本地修改"
teamturbo pull

# 选择 2: 放弃本地修改
teamturbo pull --force
```

## Help 信息的实现

CLI 的 help 信息是通过 `clap` crate 自动生成的。在 `main.rs` 中：

```rust
#[derive(Parser)]
#[command(name = "teamturbo")]
#[command(about = "TeamTurbo CLI for Docuram", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Login to TeamTurbo
    Login {
        /// Force browser authorization mode
        #[arg(long)]
        browser: bool,
        /// Force manual token input mode
        #[arg(long)]
        manual: bool,
    },
    // ... 其他命令
}
```

每个命令的文档注释（`///`）会自动显示在 help 信息中。

## 特点

✅ **自动生成** - 通过 clap 派生宏自动生成
✅ **完整文档** - 每个命令和参数都有描述
✅ **标准格式** - 符合 Unix 命令行工具惯例
✅ **版本信息** - 支持 `--version` 查看版本
✅ **子命令帮助** - 每个子命令都有独立的 help
✅ **参数说明** - 所有参数都有详细说明

## 相关资源

- **Clap 文档**: https://docs.rs/clap/
- **命令行工具设计**: https://clig.dev/
- **完整实现**: [src/main.rs](src/main.rs)
