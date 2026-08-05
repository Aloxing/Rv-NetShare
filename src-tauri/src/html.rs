//! Tiny HTML rendering helpers used by the embedded HTTP server.
//!
//! We hand-roll a couple of templates (no template engine) and escape
//! every piece of dynamic content to prevent injection.

use crate::state::{ShareKind, ShareSession};

const BRAND_SVG: &str = "<svg viewBox=\"0 0 24 24\" width=\"22\" height=\"22\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.8\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><circle cx=\"18\" cy=\"5\" r=\"3\"/><circle cx=\"6\" cy=\"12\" r=\"3\"/><circle cx=\"18\" cy=\"19\" r=\"3\"/><path d=\"m8.6 13.5 6.8 4M15.4 6.5l-6.8 4\"/></svg>";

const FOLDER_SVG: &str = "<svg viewBox=\"0 0 24 24\" width=\"20\" height=\"20\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.8\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2Z\"/></svg>";

const FILE_SVG: &str = "<svg viewBox=\"0 0 24 24\" width=\"20\" height=\"20\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.8\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z\"/><path d=\"M14 2v4a2 2 0 0 0 2 2h4\"/></svg>";

const UP_SVG: &str = "<svg viewBox=\"0 0 24 24\" width=\"18\" height=\"18\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.8\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M7 17 17 7\"/><path d=\"M7 7h10v10\"/></svg>";

const CHEVRON_SVG: &str = "<svg viewBox=\"0 0 24 24\" width=\"14\" height=\"14\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"m9 18 6-6-6-6\"/></svg>";

const EMPTY_SVG: &str = "<svg viewBox=\"0 0 24 24\" width=\"34\" height=\"34\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M22 12h-6l-2 3h-4l-2-3H2\"/><path d=\"M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z\"/></svg>";

const ERROR_SVG: &str = "<svg viewBox=\"0 0 24 24\" width=\"26\" height=\"26\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.8\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><circle cx=\"12\" cy=\"12\" r=\"10\"/><path d=\"M12 8v4\"/><path d=\"M12 16h.01\"/></svg>";

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn fmt_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let fixed = if value >= 100.0 || unit == 0 {
        0
    } else if value >= 10.0 {
        1
    } else {
        2
    };
    format!("{:.fixed$} {}", value, UNITS[unit])
}

fn brand() -> String {
    format!(
        "<div class=\"brand\"><div class=\"brand-icon\">{}</div><div><h1>RV NetShare</h1><p>局域网文件分享</p></div></div>",
        BRAND_SVG
    )
}

pub fn render_index(shares: &[ShareSession]) -> String {
    let mut body = String::new();
    body.push_str(&brand());

    if shares.is_empty() {
        body.push_str(
            "<p class=\"page-title\">暂无分享</p><p class=\"page-sub\">打开本机应用添加文件或文件夹后，这里会显示可下载的内容。</p>",
        );
        body.push_str(&format!(
            "<div class=\"empty\">{}</div>",
            EMPTY_SVG
        ));
        return page("暂无分享 - RV NetShare", &body);
    }

    body.push_str(&format!(
        "<p class=\"page-title\">当前 {} 个分享</p><p class=\"page-sub\">以下内容由本机应用提供，仅限同一局域网内访问。</p>",
        shares.len()
    ));
    body.push_str("<div class=\"cards\">");
    for share in shares {
        let (kind_label, icon_class, icon, href) = match share.kind {
            ShareKind::File => ("文件", "icon-file", FILE_SVG, format!("/s/{}", share.id)),
            ShareKind::Folder => ("文件夹", "icon-folder", FOLDER_SVG, format!("/s/{}/", share.id)),
        };
        let size = fmt_size(share.total_bytes.max(share.size));
        body.push_str(&format!(
            "<a class=\"card\" href=\"{href}\"><div class=\"card-top\"><div class=\"card-icon {icon_class}\">{icon}</div><div><div class=\"card-name\">{name}</div><div class=\"card-meta\"><span>{kind}</span><span>{size}</span></div></div></div><div class=\"card-open\">进入分享</div></a>",
            href = href,
            icon_class = icon_class,
            icon = icon,
            name = escape(&share.name),
            kind = kind_label,
            size = size,
        ));
    }
    body.push_str("</div>");
    page(&format!("{} 个分享 - RV NetShare", shares.len()), &body)
}

pub fn render_folder_listing(
    share: &ShareSession,
    subpath: &str,
    entries: &[(String, bool, u64)],
) -> String {
    let mut body = String::new();
    body.push_str(&brand());

    body.push_str("<nav class=\"crumbs\"><a href=\"/\">主页</a>");
    body.push_str(&format!(
        "<span class=\"sep\">{chev}</span><a href=\"/s/{id}/\">{name}</a>",
        chev = CHEVRON_SVG,
        id = share.id,
        name = escape(&share.name)
    ));
    if !subpath.is_empty() {
        let mut acc = String::new();
        for seg in subpath.split('/').filter(|s| !s.is_empty()) {
            acc.push_str(seg);
            acc.push('/');
            body.push_str(&format!(
                "<span class=\"sep\">{chev}</span><a href=\"/s/{id}/{acc}\">{seg}</a>",
                chev = CHEVRON_SVG,
                id = share.id,
                acc = acc,
                seg = escape(seg)
            ));
        }
    }
    body.push_str("</nav>");

    body.push_str(&format!(
        "<div class=\"bar\"><h2>{name}</h2><a class=\"back\" href=\"{up}\">返回上级</a></div>",
        name = escape(&share.name),
        up = parent_link(share, subpath)
    ));

    body.push_str("<div class=\"list\">");
    body.push_str(&format!(
        "<a class=\"row row-up\" href=\"{up}\"><div class=\"row-icon\">{up_svg}</div><div class=\"row-name\">返回上级</div></a>",
        up = parent_link(share, subpath),
        up_svg = UP_SVG
    ));
    if entries.is_empty() {
        body.push_str("<div class=\"row\"><div class=\"row-name\">此文件夹为空</div></div>");
    }
    for (name, is_dir, size) in entries {
        let href = if subpath.is_empty() {
            format!("{}/", escape(name))
        } else {
            format!("{}/{}/", subpath, escape(name))
        };
        let link = format!("/s/{id}/{href}", id = share.id, href = href);
        if *is_dir {
            body.push_str(&format!(
                "<a class=\"row\" href=\"{link}\"><div class=\"row-icon icon-folder\">{icon}</div><div class=\"row-name\">{name}</div><div class=\"row-size\">文件夹</div></a>",
                link = link,
                icon = FOLDER_SVG,
                name = escape(name)
            ));
        } else {
            body.push_str(&format!(
                "<a class=\"row\" href=\"{link}\"><div class=\"row-icon icon-file\">{icon}</div><div class=\"row-name\">{name}</div><div class=\"row-size\">{size}</div></a>",
                link = link,
                icon = FILE_SVG,
                name = escape(name),
                size = fmt_size(*size)
            ));
        }
    }
    body.push_str("</div>");
    page(&format!("{} - {}", share.name, subpath), &body)
}

pub fn render_error(code: u16, reason: &str, msg: &str) -> String {
    let body = format!(
        "<div class=\"error\"><div class=\"brand-icon\">{icon}</div><h1>{code} {reason}</h1><p>{msg}</p></div>",
        icon = ERROR_SVG,
        code = code,
        reason = escape(reason),
        msg = escape(msg)
    );
    page(&format!("{code} {reason}"), &body)
}

fn parent_link(share: &ShareSession, subpath: &str) -> String {
    let p = subpath.trim_end_matches('/');
    let parent = match p.rfind('/') {
        Some(idx) => &p[..idx],
        None => "",
    };
    if parent.is_empty() {
        format!("/s/{}/", share.id)
    } else {
        format!("/s/{}/{}/", share.id, parent)
    }
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title}</title><style>{css}</style></head><body><main class=\"wrap\">{body}</main></body></html>",
        title = escape(title),
        css = CSS,
        body = body,
    )
}

const CSS: &str = "
  :root { color-scheme: light dark; --bg:#ffffff; --surface:#ffffff; --surface-2:#fafafa; --border:#e4e4e7; --border-strong:#d4d4d8; --text:#09090b; --muted:#71717a; --subtle:#a1a1aa; --accent:#09090b; --accent-fg:#ffffff; --accent-soft:#f4f4f5; --shadow:0 1px 2px rgba(0,0,0,.05),0 8px 24px -12px rgba(0,0,0,.12); }
  @media (prefers-color-scheme: dark) { :root { --bg:#09090b; --surface:#18181b; --surface-2:#131316; --border:#27272a; --border-strong:#3f3f46; --text:#fafafa; --muted:#a1a1aa; --subtle:#71717a; --accent:#fafafa; --accent-fg:#09090b; --accent-soft:#27272a; --shadow:0 1px 2px rgba(0,0,0,.4),0 12px 32px -16px rgba(0,0,0,.7); } }
  * { box-sizing:border-box; }
  body { margin:0; font:14px/1.6 system-ui,-apple-system,\"Segoe UI\",Roboto,\"PingFang SC\",\"Microsoft YaHei\",sans-serif; background:var(--bg); color:var(--text); -webkit-font-smoothing:antialiased; }
  a { color:inherit; text-decoration:none; }
  .wrap { max-width:880px; margin:0 auto; padding:40px 20px 64px; }
  .brand { display:flex; align-items:center; gap:12px; margin-bottom:28px; }
  .brand-icon { width:44px; height:44px; border-radius:12px; background:var(--accent); color:var(--accent-fg); display:flex; align-items:center; justify-content:center; box-shadow:0 10px 24px -10px var(--accent); flex:none; }
  .brand h1 { margin:0; font-size:20px; letter-spacing:0; }
  .brand p { margin:2px 0 0; font-size:12px; color:var(--muted); }
  .page-title { margin:0 0 6px; font-size:26px; letter-spacing:0; }
  .page-sub { margin:0 0 24px; color:var(--muted); font-size:13.5px; }
  .badge { display:inline-flex; align-items:center; gap:6px; padding:5px 12px; border-radius:999px; background:var(--accent-soft); color:var(--accent); font-size:12.5px; font-weight:600; }
  .cards { display:grid; grid-template-columns:repeat(auto-fill,minmax(260px,1fr)); gap:14px; }
  .card { background:var(--surface); border:1px solid var(--border); border-radius:14px; padding:16px; box-shadow:var(--shadow); transition:border-color .15s ease,transform .15s ease; display:flex; flex-direction:column; gap:14px; }
  .card:hover { border-color:var(--accent); transform:translateY(-1px); }
  .card-top { display:flex; align-items:center; gap:12px; }
  .card-icon { width:40px; height:40px; border-radius:10px; display:flex; align-items:center; justify-content:center; flex:none; }
  .icon-folder { background:var(--surface-2); color:#b45309; }
  .icon-file { background:var(--accent-soft); color:#2563eb; }
  .card-name { font-weight:600; font-size:14.5px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .card-meta { display:flex; align-items:center; justify-content:space-between; color:var(--muted); font-size:12px; margin-top:2px; }
  .card-open { display:inline-flex; align-items:center; justify-content:center; gap:6px; padding:8px 0; border-radius:9px; background:var(--accent-soft); color:var(--accent); font-size:12.5px; font-weight:600; margin-top:auto; }
  .card-open:hover { background:var(--accent); color:var(--accent-fg); }
  .empty { text-align:center; padding:64px 20px; color:var(--subtle); background:var(--surface); border:1px dashed var(--border-strong); border-radius:14px; }
  .empty svg { margin-bottom:10px; opacity:.8; }
  .crumbs { display:flex; align-items:center; gap:8px; flex-wrap:wrap; font-size:13px; color:var(--muted); margin-bottom:18px; }
  .crumbs a { color:var(--accent); font-weight:600; }
  .crumbs .sep { color:var(--border-strong); display:flex; }
  .bar { display:flex; align-items:center; justify-content:space-between; gap:12px; margin-bottom:18px; flex-wrap:wrap; }
  .bar h2 { margin:0; font-size:17px; letter-spacing:0; }
  .back { display:inline-flex; align-items:center; padding:5px 12px; border-radius:999px; background:var(--accent-soft); color:var(--accent); font-size:12.5px; font-weight:600; }
  .back:hover { background:var(--accent); color:var(--accent-fg); }
  .list { background:var(--surface); border:1px solid var(--border); border-radius:14px; overflow:hidden; box-shadow:var(--shadow); }
  .row { display:flex; align-items:center; gap:12px; padding:12px 16px; border-top:1px solid var(--border); transition:background .12s ease; }
  .row:first-child { border-top:0; }
  .row:hover { background:var(--surface-2); }
  .row-icon { width:34px; height:34px; border-radius:9px; display:flex; align-items:center; justify-content:center; flex:none; }
  .row-name { flex:1; min-width:0; font-weight:500; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .row:hover .row-name { color:var(--accent); }
  .row-size { color:var(--subtle); font-size:12px; font-variant-numeric:tabular-nums; flex:none; }
  .row-up { color:var(--muted); }
  .error { max-width:480px; margin:10vh auto 0; text-align:center; background:var(--surface); border:1px solid var(--border); border-radius:16px; padding:36px 28px; box-shadow:var(--shadow); }
  .error .brand-icon { margin:0 auto 16px; }
  .error h1 { margin:0 0 6px; font-size:18px; letter-spacing:0; }
  .error p { margin:0; color:var(--muted); font-size:13.5px; }
  @media (max-width:640px) { .wrap { padding:24px 14px 48px; } .cards { grid-template-columns:1fr; } }
";
