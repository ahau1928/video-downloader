# 视频下载器

面向 Windows 的视频下载与批量转码工具，使用 Vue 3 + Tauri 2 构建。

**当前版本：0.4.0 · Windows x64**

[下载安装包](https://github.com/ahau1928/video-downloader/releases/latest) · [转码使用说明](docs/transcoding.md) · [浏览器登录授权说明](docs/browser-helper.md)

## 下载与安装

1. 打开上方 Release 页面，在 Assets 中下载 `video-downloader_0.4.0_x64-setup.exe`。`Source code` 是开发源码，不是安装包。
2. 关闭旧版软件后运行安装包。升级后会沿用已有设置与任务历史。
3. 打开“设置 → 网络与账号”，按本机情况配置代理。**默认代理地址为 `http://127.0.0.1:7897`，没有该代理服务时请关闭代理或修改端口。**

安装包内置 yt-dlp、FFmpeg、FFprobe、Deno 和 YouTube EJS 依赖，不需要自行设置 PATH。首次使用需具备 Windows WebView2 运行环境；安装流程会在需要时处理运行时安装。

## 下载视频

1. 粘贴单个视频链接，软件自动解析标题、分辨率、视频编码和音频信息；解析不会自动开始下载。
2. 选择保存目录、清晰度及输出方式。高级设置中可手选视频流和音频流，信息栏会显示预计封装和是否转码。
3. 点击“下载”立即排队，或“仅加入队列”稍后开始。

- **批量下载**：点击“批量添加”，粘贴多个链接或导入 TXT，检查重复及无效链接后开始。
- **保留原始画质**：不重新编码视频，同等分辨率优先合适的 MP4 视频与音频组合；实际封装取决于所选编码。
- **自动转码 · H.264 MP4**：下载后转换为 H.264 MP4。
- **仅音频 · MP3**：提取音频并输出 MP3。
- 文件名默认保持干净，可勾选分辨率后缀，再选择是否附带编码。同名时提供处理选项，避免覆盖。
- 默认同时下载 2 项、转码 1 项。暂停队列会停止当前任务与后续调度；下载续传取决于源站支持，未完成转码需要重新开始。
- 退出后保留队列与历史，未完成任务在重启后由用户选择继续。清除记录不会删除已保存的视频。

## 浏览器登录状态

YouTube、Vimeo 等网站可能要求登录或验证。支持通过配套 **Edge / Chrome 浏览器助手**授权，无需更换 Firefox，也无需直接复制浏览器 Cookie 数据库。

进入“设置 → 网络与账号”点击“启用浏览器连接”，再打开配套扩展文件夹，在浏览器扩展管理页面加载该文件夹。完整步骤见 [浏览器助手使用说明](docs/browser-helper.md)。安装包已包含扩展，Release 中的扩展 ZIP 是备用分发文件。

在网站正常登录后，点击扩展“授权并发送视频”或“仅更新登录信息”。授权在当前电脑加密保存。不要把 Cookie 文件或账号凭据提交到仓库或 Issue。

## 批量转码

在“转码”页面拖入文件，选择参数并查看逐个文件的输出预览，然后点击“开始转码”。添加区支持继续添加、逐项移除和一键清空。

| 功能 | 支持的设置 |
| --- | --- |
| 视频编码 | H.264、H.265 / HEVC，软件编码 |
| 压制方式 | CRF 或按每个文件的目标大小进行两遍编码 |
| 压制预设 | 极速、很快、快速、中等、慢速、很慢 |
| 音频 | AAC 压制、复制音频流、移除音频 |
| 音频码率 | 参考源音频、96 / 128 / 160 / 192 / 256 / 320 kbps、自定义 |
| 分辨率 | 保持原尺寸、自定义宽高、锁定宽高比、补边 / 裁剪 / 拉伸 |
| 文件名 | 自动后缀、手动后缀、不加后缀；同名自动编号 |
| 封装 | MP4、MKV、MOV |
| HDR / 色深 | 显式选择 H.265 10 位保留或转换为 SDR 8 位 |

默认 H.264、CRF 23、中等预设、AAC 192 kbps、原分辨率、MP4、保留源文件。H.265 的默认 CRF 为 28。目标大小会扣除音频占用并预留封装空间，但允许偏差，不能作为严格大小上限。

详细行为、限制及测试范围见 [转码使用说明](docs/transcoding.md)。

## 工具更新与常见问题

- 在“设置 → 工具与更新”检查依赖更新或回滚。默认在启动后检查更新，运行任务期间等待空闲后处理。
- 更新安装在用户数据目录，失败保留当前版本。
- 浏览器可播放并不保证下载器能访问。检查两者代理线路、登录授权和工具版本；网站接口与验证规则可能变化。
- MP4 是封装格式，AV1 / H.265 是否可编辑取决于剪辑软件支持。
- 当前转码输出选取第一条普通视频流和第一条音轨，不保留其他音轨、字幕和章节。HDR 动态元数据保留尚不支持，静态元数据不保证完整保留，建议保留原文件。

反馈问题请附软件版本、Windows 版本、操作步骤和错误文字，并遮盖个人路径、Cookie、账号等敏感信息。

## 开发与构建

需要 Node.js、Rust、MSVC C++ 构建工具以及 WebView2。

```powershell
npm ci
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/prepare-tools.ps1 -Refresh
npm run tauri:dev
```

仓库不提交依赖可执行文件和构建产物。准备脚本从发布渠道获取并校验工具；已有有效缓存时可省略 `-Refresh`。网络需要代理时查看脚本的 `Proxy` 参数。

```powershell
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
node scripts/test-extension.mjs
python scripts/test-browser-bridge.py
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-updater.ps1
npm run tauri:build
```

安装包位于 `src-tauri/target/release/bundle/nsis/`。`npm run dev` 仅预览界面，网页不能直接调用桌面下载与文件接口。

调试构建支持用 `VIDEOTOOL_SELF_TEST=1` 和 `VIDEOTOOL_TEST_DIR` 指定独立目录运行真实工具集成测试。诊断入口不编译进发布安装包。原生消息测试需要先编译调试 EXE。扩展公开身份由 `extension/manifest.json` 的公钥固定，升级时不要随意更换。

## 第三方组件

0.4.0 安装包内置以下已核验版本，软件可继续检查更新：

| 组件 | 版本 |
| --- | --- |
| yt-dlp | 2026.08.19 |
| FFmpeg / FFprobe | 9.0.1 |
| Deno | 2.9.6 |
| EJS（yt-dlp 内置） | 0.8.0 |

来源、校验值及第三方许可见 [第三方说明](src-tauri/resources/bin/THIRD-PARTY-NOTICES.txt)、[工具清单](src-tauri/resources/bin/bundled-tools.json) 和 [FFmpeg 许可](src-tauri/resources/bin/LICENSE-FFmpeg.txt)。第三方程序保留其各自许可。
