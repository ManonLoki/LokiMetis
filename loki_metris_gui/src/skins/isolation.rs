// 本文件收纳：外层词法函数供内置运行时捕获；在namespace转换后拼装，旧归属键不得被重写；明确归属冲突不能被解释成兼容规则部分跳过或普通注入成功；稳定领域映射只携带操作指引，不暴露宿主脚本与私有页面内容。
// （本文件由主文件 include! 并入，与主文件共用同一个模块作用域。）

// 每次页面写入在同一JavaScript求值中复核归属，避免异步预检与写入间被其它应用接管。

/// 外层词法函数供内置运行时捕获；在namespace转换后拼装，旧归属键不得被重写。
fn guarded_skin_script(body: &str) -> String {
    format!(
        r#"(() => {{
      const __lokiMetrisHasForeignOwner = () => Boolean(
        window.__CODEX_DREAM_SKIN_STATE__ || window.__BIFANG_CODEX_SKIN_COMPAT__);
      if (__lokiMetrisHasForeignOwner()) return {{ kind: 'foreignOwner' }};
      const value = (() => {{ {body}
      }})();
      return {{ kind: 'applied', value }};
    }})()"#
    )
}

/// 明确归属冲突不能被解释成兼容规则部分跳过或普通注入成功。
fn skin_owner_result(value: serde_json::Value) -> Result<Option<serde_json::Value>, AppError> {
    match value.get("kind").and_then(serde_json::Value::as_str) {
        Some("foreignOwner") => Err(foreign_skin_owner_error()),
        Some("applied") => Ok(value.get("value").cloned()),
        _ => Err(cdp_response_error()),
    }
}

/// 稳定领域映射只携带操作指引，不暴露宿主脚本与私有页面内容。
fn foreign_skin_owner_error() -> AppError {
    AppError::new(
        "skin.foreign_owner",
        "其它应用正在维护宿主皮肤，请先在原应用停止皮肤后重试。",
    )
}

/// CSS只替换独占ID选择器，保留注释、字符串、URL及更长的用户标识。
fn runtime_css_namespace(css: &str) -> String {
    let mut output = String::with_capacity(css.len());
    let mut position = 0;
    let mut quote = None;
    let mut comment = false;
    while position < css.len() {
        let tail = &css[position..];
        let ch = tail
            .chars()
            .next()
            .expect("position is at a character boundary");
        if comment {
            if tail.starts_with("*/") {
                output.push_str("*/");
                position += 2;
                comment = false;
                continue;
            }
        } else if let Some(end) = quote {
            if ch == '\\' {
                output.push(ch);
                position += 1;
                if let Some(next) = css[position..].chars().next() {
                    output.push(next);
                    position += next.len_utf8();
                }
                continue;
            }
            if ch == end {
                quote = None;
            }
        } else if tail.starts_with("/*") {
            output.push_str("/*");
            position += 2;
            comment = true;
            continue;
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
        } else if ch == '\\' {
            output.push(ch); position += 1;
            if let Some(next) = css[position..].chars().next() { output.push(next); position += next.len_utf8(); }
            continue;
        } else if tail
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("url("))
        {
            let mut escaped = false;
            let mut end_quote = None;
            let mut end = 4;
            for (index, next) in tail[4..].char_indices() {
                end = 4 + index + next.len_utf8();
                if escaped {
                    escaped = false;
                    continue;
                }
                if next == '\\' {
                    escaped = true;
                    continue;
                }
                if let Some(quote) = end_quote {
                    if next == quote {
                        end_quote = None;
                    }
                } else if matches!(next, '\'' | '"') {
                    end_quote = Some(next);
                } else if next == ')' {
                    break;
                }
            }
            output.push_str(&tail[..end]);
            position += end;
            continue;
        } else if ch == '#' {
            let replacement = ["style", "chrome"].into_iter().find_map(|kind| {
                let old = format!("#codex-dream-skin-{kind}");
                let rest = tail.strip_prefix(&old)?;
                let extends_id = rest
                    .chars()
                    .next()
                    .is_some_and(|next| !next.is_ascii() || next.is_alphanumeric() || matches!(next, '-' | '_' | '\\'));
                (!extends_id).then(|| (old.len(), format!("#loki-metris-dream-skin-{kind}")))
            });
            if let Some((length, value)) = replacement {
                output.push_str(&value);
                position += length;
                continue;
            }
        }
        output.push(ch);
        position += ch.len_utf8();
    }
    output
}
