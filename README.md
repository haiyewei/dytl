# DYTL

`dytl` 是一个基于 Rust 的多平台直播工具箱，当前支持抖音、快手和 Twitter/X，主要用于直播间信息获取、用户信息抓取、开播监控与自动录制。

> **免责声明**：本项目仅供学习与个人归档使用。请遵守各平台服务条款与当地法律法规；请勿将 Cookie、账号凭证提交到公开仓库。

## 源码结构

```text
src/
├── main.rs           # 进程入口
├── cli/              # 命令行解析与子命令
├── config/           # YAML 配置加载与校验
│   ├── platform.rs   # 平台枚举
│   ├── model.rs      # 运行时配置类型
│   └── load.rs       # 解析与规范化
├── core/             # 错误、日志、路径、时间、信号、JSON 工具
├── media/            # 录制 / 播放 / 救援封装
│   ├── ffmpeg/       # ffmpeg/ffplay/hls 实现
│   ├── live_stream.rs
│   └── rescue.rs
├── monitor/          # 统一开播监控与录制子进程生命周期
└── platform/         # 抖音 / 快手 / Twitter 客户端（经 amagi）
```

依赖方向（上层可依赖下层，反向禁止）：

```text
cli → config | monitor | media | platform | core
monitor → config | media | platform | core
media → config | core
platform → core
config → core
```

## 功能概览

- 抖音：
  `live` 获取直播间信息、录制直播、使用 `ffplay` 预览播放
- 抖音：
  `user` 获取用户主页数据和视频列表
- 抖音：
  `rescue` 手动修复异常中断后遗留的 TS 分片并封装 MP4
- 快手：
  `live` 获取直播间信息、录制直播、使用 `ffplay` 预览播放
- 快手：
  `user` 获取用户主页数据和作品列表
- Twitter/X：
  `live` 获取直播信息、默认解析最高画质 HLS、录制直播、使用 `ffplay` 预览播放
- Twitter/X：
  `download` 下载已结束且可回放的 broadcast，默认解析最高画质 HLS
- Twitter/X：
  `user` 获取用户主页数据
- 统一监控：
  轮询 `config.yaml` 中的监控目标，开播后自动拉起独立录制子进程

## 运行要求

### 系统要求

- Rust 工具链
- `cargo`
- `ffmpeg`
- 可选：
  `ffplay`，用于本地预览播放
- 外部依赖：
  `amagi` 命令行工具，项目通过它获取抖音/快手/Twitter 接口数据
- 外部依赖：
  `curl`，用于解析 Twitter/X HLS master 中的最高画质 variant

### 配置要求

- 默认读取当前工作目录下的 `config.yaml`
- 也可以通过全局参数 `--config /abs/path/config.yaml` 显式指定配置文件
- 可参考 [config.example.yaml](./config.example.yaml)
- 如果要使用抖音能力，必须配置 `douyin.cookies`
- 如果要使用快手能力，必须配置 `kuaishou.cookies`
- 如果要使用 Twitter/X 能力，必须配置 `twitter.cookies`，公开直播通常可留空
- 如果要使用统一监控，必须配置 `monitor.targets`
- 可选配置 `logging.success_log_path` / `logging.failure_log_path` 用于持久记录关键成功和失败事件
- 可选配置 `time.utc_offset_hours` 用于控制终端日志和录制文件名中的时间偏移

## 构建与启动

### 调试运行

```bash
cargo run -- monitor
```

如果配置文件不在当前目录：

```bash
cargo run -- --config /home/app/dytl/config.yaml monitor
```

### 编译

```bash
cargo build --release
```

编译后的二进制默认位于：

- 调试版：
  `target/debug/dytl`
- 发布版：
  `target/release/dytl`

### 发布正式版（GitHub Actions）

推送符合 `v*` 的 tag 会触发 [`.github/workflows/release.yml`](./.github/workflows/release.yml)，多平台交叉编译并上传到 GitHub Release（流程对齐 `amagi-rs`）：

```bash
git tag v1.0.0
git push origin v1.0.0
```

产物示例：

| 文件 | 平台 |
|------|------|
| `dytl-x86_64-unknown-linux-gnu.tar.gz` | Linux x86_64 (glibc) |
| `dytl-aarch64-unknown-linux-gnu.tar.gz` | Linux aarch64 (glibc) |
| `dytl-x86_64-unknown-linux-musl.tar.gz` | Linux x86_64 (musl，适合 Alpine) |
| `dytl-aarch64-unknown-linux-musl.tar.gz` | Linux aarch64 (musl) |
| `dytl-x86_64-apple-darwin.tar.gz` | macOS Intel |
| `dytl-aarch64-apple-darwin.tar.gz` | macOS Apple Silicon |
| `dytl-x86_64-pc-windows-msvc.zip` | Windows x86_64 |
| `SHA256SUMS.txt` | 校验和 |

也可在 Actions 里用 `workflow_dispatch` 手动触发；若填写已有 tag，会基于该 tag 打包并发布。

## 配置说明

监控配置采用统一格式：

```yaml
logging:
  success_log_path: ./content/logs/success.log
  failure_log_path: ./content/logs/failure.log

time:
  utc_offset_hours: 8

monitor:
  poll_interval_sec: 60
  restart_interval_hours: 6
  auto_rescue:
    enabled: true
    on_startup: true
    interval_minutes: 10
    min_age_minutes: 5
  targets:
    - platform: douyin
      account: "test_douyin_sec_uid"
      alias: "测试抖音账号"
      enabled: true
    - platform: kuaishou
      account: "test_ks_account"
      alias: "测试快手账号"
      enabled: false
    - platform: twitter
      account: "test_screen_user"
      alias: "测试推特账号"
      enabled: false
```

字段说明：

- `platform`
  可选值：`douyin`、`dy`、`kuaishou`、`ks`、`twitter`、`x`
- `account`
  当 `platform=douyin` 时填写 `sec_uid`
  当 `platform=kuaishou` 时填写 `principal_id`
  当 `platform=twitter` 时填写 `screen_name`（不带 `@`）
- `alias`
  可选，自定义显示名称
- `enabled`
  可选，`true` / `false`
  默认值为 `true`
  设为 `false` 时保留在配置中，但不会参与监控
- `poll_interval_sec`
  监控轮询间隔，单位秒
  建议不低于 `30`
- `restart_interval_hours`
  定时重启间隔，单位小时
  适合长期运行时配合服务模式使用
- `auto_rescue`
  自动巡航封装异常中断遗留的 `temp_record_*` 分片目录
  `min_age_minutes` 用于跳过仍可能正在写入的录制目录
- `success_log_path`
  可选，关键成功事件日志路径，例如用户数据保存、录制封装完成
- `failure_log_path`
  可选，关键失败/告警事件日志路径，例如封装失败、缓存清理失败
- `utc_offset_hours`
  可选，终端日志时间和录制文件名时间使用的 UTC 偏移，范围 `-23` 到 `23`

## 常用命令

### 根级命令

```bash
dytl monitor
```

```bash
dytl --config /home/app/dytl/config.yaml monitor
```

作用：

- 按 `config.yaml` 中的 `monitor.targets` 统一监控账号
- 检测到开播后自动启动录制
- 默认按分片录制，结束后自动封装为 MP4
- 服务启动和运行中会自动巡航封装异常中断遗留的 `temp_record_*` 分片目录

### 抖音命令

```bash
dytl douyin live <url 或 web_rid>
dytl douyin live <url 或 web_rid> -r
dytl douyin live <url 或 web_rid> -p
dytl douyin user <sec_uid 或 url> --videos
dytl douyin rescue
dytl rescue
```

### 快手命令

```bash
dytl kuaishou live <principal_id 或 url>
dytl kuaishou live <principal_id 或 url> -r
dytl kuaishou live <principal_id 或 url> -p
dytl kuaishou user <principal_id 或 url> --works
dytl kuaishou rescue
```

### Twitter/X 命令

```bash
dytl twitter live <broadcast_url/broadcast_id/screen_name/url>
dytl twitter live <broadcast_url/broadcast_id/screen_name/url> -r
dytl twitter live <broadcast_url/broadcast_id/screen_name/url> -p
dytl twitter download <broadcast_url/broadcast_id>
dytl twitter download <broadcast_url/broadcast_id> --jobs 8
dytl twitter download <broadcast_url/broadcast_id> -o ./content/twitter/download/replay.mp4
dytl twitter user <screen_name 或 url>
dytl twitter rescue
```

Twitter/X 直播录制和回放下载默认都会解析 HLS master，并选择分辨率最高、同分辨率下码率最高的 variant。
`download` 只面向已结束且平台标记为可回放的 broadcast；仍在直播中的目标请使用 `twitter live -r`。
回放下载默认会读取机器可用并行度，并使用其一半作为分片下载并发；可通过 `--jobs N` 手动指定。

## 输出目录

项目默认输出到 `content/`：

- 抖音直播录制：
  `content/douyin/live/`
- 快手直播录制：
  `content/kuaishou/live/`
- Twitter/X 直播录制：
  `content/twitter/live/`
- Twitter/X 回放下载：
  `content/twitter/download/`
- 抖音用户数据：
  `content/douyin/user/`
- 快手用户数据：
  `content/kuaishou/user/`
- Twitter/X 用户数据：
  `content/twitter/user/`
- 统一监控停止标记：
  `content/monitor/`

录制过程会先生成临时 TS 分片目录，再在结束后自动封装成 MP4。
如果服务异常中断遗留了 `temp_record_*` 目录，监控模式会按 `monitor.auto_rescue`
配置自动巡航封装；也可以手动执行 `dytl rescue`。

当前录制文件命名格式为：

```text
record_<Unix毫秒时间戳>_<系统时间到秒>_<直播间>.mp4
```

如果配置了 `time.utc_offset_hours`，文件名中的日期时间按该偏移计算。

例如：

```text
record_1000000000001_2026-05-12_16-31-45_test_room_id.mp4
```

## 服务模式

长期运行可把 `dytl monitor` 交给 `systemd`（或其它进程管理）托管。启动命令示例：

```bash
dytl --config /abs/path/config.yaml monitor
```

## 注意事项

- 监控功能依赖平台接口可访问；需要 Cookie 的平台在 Cookie 失效后需要手动更新 `config.yaml`
- `ffplay` 只影响预览播放，不影响录制
- `rescue` 处理各平台 `content/<platform>/live/` 下符合命名规则的临时录制目录
- 如果要长期运行，优先用进程管理器托管 `dytl monitor`，而不是手工常驻终端
