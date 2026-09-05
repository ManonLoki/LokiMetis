import { appI18n } from './i18n';

/** 把任意 Tauri 或运行时错误折叠为当前语言的安全说明，不显示原始拒绝文本。 */
// 参数名前的下划线（`_error`）是约定俗成的写法，表示“这个参数故意不用”
// （配合 eslint 的未使用变量规则，避免报警告）；这里是刻意设计——
// Tauri IPC 失败时的原始错误可能带有系统路径、堆栈或其他实现细节，
// 一律不展示给用户，只用固定的通用文案，把“错误具体是什么”这类排查
// 信息留给开发者用日志渠道查看，不经过界面泄露。
export function visibleErrorMessage(_error: unknown, fallback?: string): string {
  return fallback ?? appI18n.t('common.unknownError');
}
