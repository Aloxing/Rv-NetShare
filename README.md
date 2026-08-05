# RV NetShare

基于 Tauri 2 + Vue 3 + Vite 6 + Tailwind CSS v4 的局域网文件分享工具。
把本地的文件 / 文件夹生成局域网下载链接，同一时间支持多个分享并存，全部数据保存在本机。

## 怎么用

1. 启动应用后，点击「选文件夹」或「选文件」按钮选择要分享的内容。
2. 系统会立刻生成一个形如 http://192.168.x.x:48721/s/<id> 的链接。
3. 把链接发给同一局域网里的其他人，他们在浏览器中打开就能下载（或浏览文件夹）。
4. 多个分享可以并存；在卡片右上角点 × 即可随时停用某个分享。

## 功能概览

- 分享：选文件夹 / 选文件 / 粘贴路径创建分享
- 下载记录：所有对端下载行为都会记下来，仅保存在本地
- 设置：本机 IP / 端口 / 字体大小 / 自定义保存目录

## 技术栈

- 前端：Vue 3 + TypeScript + Vite 6 + Tailwind CSS v4.3
- 后端：Rust + Tauri 2（仅依赖 tauri / serde / serde_json / dirs / tauri-plugin-opener）
- 通信：Tauri invoke() + 事件总线
- 文件分享：内嵌一个 std::net::TcpListener 实现的 HTTP/1.1 服务器，
  大文件按 256 KiB 流式分块发送，TCP_NODELAY 已启用

## 开发

```bash
npm install
npm run tauri dev
```

> 当前机器的 crates.io 网络受限，请使用 cargo check --offline。
> Rust 端没有引入任何额外 crate，保持最小依赖。

## 构建

```bash
npm run build
npm run tauri build
```

## 本地数据

- 默认保存目录：%LOCALAPPDATA%\rv-netshare\（可在设置页修改，仅影响下载记录）
- 下载记录：history.json（每次下载都会追加，重启后保留）
- 分享列表：shares.json（与配置同目录，重启后自动恢复，旧链接继续有效）

## 默认端口

接收服务监听 0.0.0.0:48721，被占用时自动 +1（最多到 48799）。
