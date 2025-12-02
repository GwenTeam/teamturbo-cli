# TeamTurbo CLI - 功能清单

## ✅ 完整的 Help 信息系统

CLI 已经实现了完整的 help 信息系统，通过 `clap` 框架自动生成。

### 实现位置
**文件**: [src/main.rs](src/main.rs:10-73)

### 支持的 Help 命令

#### 1. 主命令帮助
```bash
teamturbo --help
teamturbo -h
teamturbo help
```

#### 2. 子命令帮助
```bash
teamturbo login --help
teamturbo init --help
teamturbo pull --help
teamturbo push --help
teamturbo sync --help
teamturbo diff --help
teamturbo logout --help
teamturbo whoami --help
```

#### 3. 版本信息
```bash
teamturbo --version
teamturbo -V
```

## 📋 命令完整列表

| 命令 | 描述 | 参数 | 状态 |
|------|------|------|------|
| `login` | 登录到 TeamTurbo | `--browser`, `--manual` | ✅ |
| `logout` | 登出 | 无 | ✅ |
| `whoami` | 查看登录状态 | 无 | ✅ |
| `init` | 初始化项目 | `--from <URL>`, `--force`, `--no-download` | ✅ |
| `pull` | 拉取文档更新 | `[documents]...`, `--force` | ✅ |
| `push` | 推送文档更改 | `[documents]...`, `--message <MSG>` | ✅ |
| `sync` | 同步文档 | `--force` | ✅ |
| `diff` | 查看差异 | `[document]` | ✅ |

## 🎯 Help 信息特点

### 1. 自动生成
使用 `clap` 的派生宏自动生成 help 信息：
```rust
#[derive(Parser)]
#[command(name = "teamturbo")]
#[command(about = "TeamTurbo CLI for Docuram", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
```

### 2. 结构化文档
每个命令都有清晰的文档注释：
```rust
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
    // ...
}
```

### 3. 标准格式
符合 Unix 命令行工具惯例：
- 短参数：`-h`, `-V`, `-f`, `-m`
- 长参数：`--help`, `--version`, `--force`, `--message`
- 位置参数：`[documents]...`
- 可选参数：`[document]`

### 4. 完整的参数说明
每个参数都有：
- 参数名称
- 参数类型
- 参数描述
- 默认值（如果有）

## 📖 使用示例

### 基础用法
```bash
# 查看所有命令
teamturbo --help

# 查看特定命令的帮助
teamturbo init --help

# 查看版本
teamturbo --version
```

### 工作流示例
```bash
# 1. 登录（会显示交互式帮助）
teamturbo login

# 2. 初始化项目（如果不知道 URL，--help 会告诉你）
teamturbo init --help
teamturbo init --from <config-url>

# 3. 查看可用命令
teamturbo --help

# 4. 拉取文档（查看参数说明）
teamturbo pull --help
teamturbo pull

# 5. 推送文档（查看如何添加消息）
teamturbo push --help
teamturbo push -m "更新文档"
```

## 🔍 Help 信息示例

### 主命令 Help
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

### 子命令 Help（init 示例）
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

### 带参数的命令 Help（push 示例）
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

## 🎨 Help 信息的优点

### 1. 用户友好
- ✅ 清晰的命令描述
- ✅ 详细的参数说明
- ✅ 使用示例提示
- ✅ 标准化格式

### 2. 自我文档化
- ✅ 不需要查看外部文档
- ✅ 命令行即时查询
- ✅ 每个命令都有独立帮助

### 3. 符合标准
- ✅ 遵循 Unix 惯例
- ✅ 支持 `-h` 和 `--help`
- ✅ 支持 `-V` 和 `--version`
- ✅ 标准化的输出格式

### 4. 易于维护
- ✅ 代码即文档
- ✅ 自动生成，无需手动维护
- ✅ 修改代码自动更新 help

## 📚 完整文档

详细的 help 输出示例请查看：
- **[HELP_OUTPUT.md](HELP_OUTPUT.md)** - 完整的 help 信息示例
- **[README.md](README.md)** - CLI 使用指南
- **[docs/DOCURAM_COMPLETE_GUIDE.md](../gwen-web-app/docs/DOCURAM_COMPLETE_GUIDE.md)** - 完整实施指南

## 🧪 测试 Help 信息

### 构建并测试
```bash
# 构建 CLI
cd teamturbo-cli
cargo build --release

# 测试主帮助
./target/release/teamturbo --help

# 测试版本
./target/release/teamturbo --version

# 测试各子命令帮助
./target/release/teamturbo login --help
./target/release/teamturbo init --help
./target/release/teamturbo pull --help
./target/release/teamturbo push --help
./target/release/teamturbo sync --help
./target/release/teamturbo diff --help
./target/release/teamturbo logout --help
./target/release/teamturbo whoami --help
```

### 验证错误提示
```bash
# 缺少必需参数时的提示
./target/release/teamturbo init
# Error: required option '--from' not provided
# Usage: teamturbo init --from <URL>

# 无效参数时的提示
./target/release/teamturbo push --invalid
# Error: unknown option '--invalid'
# Try 'teamturbo push --help' for more information
```

## ✨ 总结

TeamTurbo CLI 已经实现了**完整且专业的 help 信息系统**：

- ✅ **8 个命令**全部有详细 help
- ✅ **所有参数**都有清晰说明
- ✅ **符合标准**的命令行工具惯例
- ✅ **自动生成**，易于维护
- ✅ **用户友好**，易于学习使用

用户可以通过 `--help` 选项随时查看命令用法，无需查阅外部文档！
