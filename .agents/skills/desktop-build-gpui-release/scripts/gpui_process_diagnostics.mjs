/** 发布工具的失败诊断只保留公开步骤和脱敏输出，不输出签名认证值。 */
import path from 'node:path';

/** 同时遮盖已知认证环境值与可能被外部工具回显的完整或截断私钥。 */
export function redactDiagnostic(value, environment) {
  let text = String(value ?? '').replace(/-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?(?:-----END [A-Z ]*PRIVATE KEY-----|$)/gu, '[REDACTED PRIVATE KEY]');
  const secrets = [...new Set(Object.entries(environment).filter(([name,value]) => /APPLE_|LM_NOTARIZATION_|MACOS_CERTIFICATE_|TOKEN|SECRET|PASSWORD|PRIVATE_KEY/iu.test(name) && typeof value === 'string' && value.length).map(([,value]) => value))].sort((a,b) => b.length - a.length);
  for (const secret of secrets) text = text.split(secret).join('[REDACTED]');
  return text;
}

/** 白名单只允许显示 xcrun 的公开动作，不把认证参数或文件路径拼入命令标题。 */
export function processFailure(program, args, result, environment) {
  const name = path.basename(program);
  const knownAction = name === 'xcrun' && ((args[0] === 'notarytool' && ['submit','info','log','history','wait'].includes(args[1])) || (args[0] === 'stapler' && ['staple','validate'].includes(args[1])));
  const operation = knownAction ? `${name} ${args[0]} ${args[1]}` : name;
  const diagnostic = [result.error?.message,result.stderr?.trim(),result.stdout?.trim()].filter(Boolean).join('\n') || String(result.status);
  const status = Number.isInteger(result.status) ? ` (exit ${result.status})` : '';
  const error = new Error(`${operation} failed${status}: ${redactDiagnostic(diagnostic,environment).slice(-8000)}`);
  error.exitCode = result.status;
  return error;
}
