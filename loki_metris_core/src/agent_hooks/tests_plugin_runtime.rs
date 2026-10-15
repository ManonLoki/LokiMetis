//! 执行真实生成的插件，使用宿主运行时验证回调和最小信封，不运行外部Agent。

use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use serde_json::{Value, json};

use super::{AiTool, generate_hook_config};

/// 用已有宿主运行时执行生成器输出，标准输入传代码且捕获失败诊断。
fn run_runtime(runtime: &str, args: &[&str], script: &str) -> Value {
    let mut child = Command::new(runtime)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("插件回归需要对应宿主运行时");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// OpenCode具名回调实际执行，并正确转发callID和决策requestID而不泄露参数/答案。
#[test]
fn opencode_generated_plugin_executes_native_callbacks_and_replies() {
    let preview = generate_hook_config(AiTool::OpenCode, Path::new("/unused/loki")).unwrap();
    let source = serde_json::to_string(&preview.content).unwrap();
    let harness = r#"
import childProcess from 'node:child_process'
import { syncBuiltinESMExports } from 'node:module'
import { EventEmitter } from 'node:events'
const captures = []
childProcess.spawn = (_executable, args) => {
  const child = new EventEmitter()
  child.stdin = new EventEmitter()
  child.stdin.end = body => {
    captures.push({ args, body: JSON.parse(body) })
    queueMicrotask(() => child.emit('exit', 0))
  }
  child.kill = () => {}
  return child
}
syncBuiltinESMExports()
const { LokiMetrisPlugin } = await import('data:text/javascript;base64,' + Buffer.from(source).toString('base64'))
const plugin = await LokiMetrisPlugin()
await plugin['tool.execute.before']({sessionID:'s',callID:'call',tool:'bash'}, {args:{command:'private'}})
await plugin['tool.execute.after']({sessionID:'s',callID:'call',tool:'bash'}, {output:'private'})
for (const event of [
  {type:'question.asked',properties:{sessionID:'s',id:'q',questions:['private'],tool:{callID:'question-call'}}},
  {type:'permission.asked',properties:{sessionID:'s',id:'p',patterns:['private']}},
  {type:'question.replied',properties:{sessionID:'s',requestID:'q',answers:['private']}},
  {type:'permission.replied',properties:{sessionID:'s',requestID:'p',reply:'once'}},
  {type:'question.rejected',properties:{sessionID:'s',requestID:'closed'}},
]) await plugin.event({event})
console.log(JSON.stringify(captures))
"#;
    let result = run_runtime(
        "node",
        &["--input-type=module"],
        &format!("const source = {source};\n{harness}"),
    );
    let events = result.as_array().unwrap();
    assert_eq!(events.len(), 7);
    for (event, name, id) in [
        (&events[0], "tool.execute.before", "call"),
        (&events[1], "tool.execute.after", "call"),
        (&events[2], "question.asked", "q"),
        (&events[3], "permission.asked", "p"),
        (&events[4], "question.replied", "q"),
        (&events[5], "permission.replied", "p"),
        (&events[6], "question.rejected", "closed"),
    ] {
        assert_eq!(event["args"][1], "opencode");
        assert_eq!(event["body"]["hook_event_name"], name);
        assert_eq!(event["body"]["session_id"], "s");
        assert_eq!(event["body"]["interaction_id"], id);
        assert!(event["body"].get("turn_id").is_none());
        assert!(!event.to_string().contains("private"));
    }
}

/// Hermes生成的Python插件确实读取完成/中断和smart审批标志，不靠字符串断言。
#[test]
fn hermes_generated_plugin_normalizes_real_observer_flags() {
    let preview = generate_hook_config(AiTool::Hermes, Path::new("/unused/loki")).unwrap();
    let source = serde_json::to_string(&preview.content).unwrap();
    let harness = r#"
import json, subprocess
captures = []
subprocess.run = lambda args, **kwargs: captures.append(json.loads(kwargs['input']))
namespace = {}
exec(SOURCE, namespace)
hooks = {}
class Context:
    def register_hook(self, event, callback): hooks[event] = callback
namespace['register'](Context())
hooks['pre_approval_request'](session_id='s', surface='smart', command='private')
hooks['on_session_end'](session_id='s', completed=True, response='private')
hooks['on_session_end'](session_id='s', completed=False, failed=True, interrupted=False)
hooks['on_session_end'](session_id='s', completed=True, interrupted=True)
hooks['pre_api_request'](session_id='s', messages=['private'])
hooks['on_session_end'](session_id='s', completed=False, failed=False, interrupted=False)
print(json.dumps(captures))
"#;
    // JSON字符串是Python兼容的字符串字面量，此处只插入生成源码的字符串。
    let result = run_runtime("python3", &["-"], &format!("SOURCE = {source}\n{harness}"));
    let events = result.as_array().unwrap();
    assert_eq!(events.len(), 6);
    for (event, status) in events.iter().zip([
        json!("smart"),
        json!("completed"),
        json!("failed"),
        json!("cancelled"),
        Value::Null,
        json!("cancelled"),
    ]) {
        assert_eq!(event["status"], status);
        assert_eq!(event["session_id"], "s");
        assert!(!event.to_string().contains("private"));
    }
}
